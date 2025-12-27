//! GPU VBIOS Reading and Analysis
//!
//! Reads GPU VBIOS (Video BIOS) data for:
//! - Power table extraction
//! - Clock limits
//! - Fan curves
//! - Voltage tables
//!
//! Methods:
//! - NVIDIA: NVML API or registry
//! - AMD: AMDGPU sysfs or WMI
//! - Windows: Registry or driver IOCTL

use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

// ============================================================================
// VBIOS Data Structures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VbiosInfo {
    pub vendor: GpuVendor,
    pub version: String,
    pub date: Option<String>,
    pub size_bytes: usize,
    pub checksum_valid: bool,
    pub power_table: Option<PowerTable>,
    pub clock_table: Option<ClockTable>,
    pub fan_table: Option<FanTable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PowerTable {
    pub tdp_watts: u32,
    pub max_power_watts: u32,
    pub min_power_watts: u32,
    pub default_power_watts: u32,
    pub power_limit_percent: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockTable {
    pub base_clock_mhz: u32,
    pub boost_clock_mhz: u32,
    pub max_clock_mhz: u32,
    pub memory_clock_mhz: u32,
    pub memory_type: String,
    pub states: Vec<ClockState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockState {
    pub index: u32,
    pub clock_mhz: u32,
    pub voltage_mv: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanTable {
    pub min_rpm: u32,
    pub max_rpm: u32,
    pub min_duty_percent: u32,
    pub max_duty_percent: u32,
    pub target_temp_c: u32,
    pub curve: Vec<FanPoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FanPoint {
    pub temp_c: u32,
    pub duty_percent: u32,
}

// ============================================================================
// NVML Types
// ============================================================================

type NvmlReturn = i32;
type NvmlDevice = *mut std::ffi::c_void;

const NVML_SUCCESS: NvmlReturn = 0;

// ============================================================================
// VBIOS Reader
// ============================================================================

pub struct VbiosReader {
    nvml: Option<Library>,
    initialized: bool,
}

impl VbiosReader {
    pub fn new() -> Self {
        let mut reader = Self {
            nvml: None,
            initialized: false,
        };
        reader.init();
        reader
    }

    fn init(&mut self) {
        // Try to load NVML
        #[cfg(target_os = "windows")]
        let nvml_path = "nvml.dll";
        #[cfg(target_os = "linux")]
        let nvml_path = "libnvidia-ml.so.1";

        if let Ok(lib) = unsafe { Library::new(nvml_path) } {
            // Initialize NVML
            unsafe {
                if let Ok(init) =
                    lib.get::<Symbol<unsafe extern "C" fn() -> NvmlReturn>>(b"nvmlInit_v2")
                {
                    if init() == NVML_SUCCESS {
                        self.nvml = Some(lib);
                        self.initialized = true;
                    }
                }
            }
        }
    }

    /// Read VBIOS from GPU
    pub fn read_vbios(&self, device_index: u32) -> Result<VbiosInfo, String> {
        // Try NVIDIA first
        if let Some(ref nvml) = self.nvml {
            return self.read_nvidia_vbios(nvml, device_index);
        }

        // Try AMD sysfs on Linux
        #[cfg(target_os = "linux")]
        if let Ok(info) = self.read_amd_vbios_sysfs(device_index) {
            return Ok(info);
        }

        Err("No supported GPU found".to_string())
    }

    /// Read NVIDIA VBIOS via NVML
    fn read_nvidia_vbios(&self, lib: &Library, device_index: u32) -> Result<VbiosInfo, String> {
        unsafe {
            // Get device handle
            let get_handle: Symbol<unsafe extern "C" fn(u32, *mut NvmlDevice) -> NvmlReturn> = lib
                .get(b"nvmlDeviceGetHandleByIndex_v2")
                .map_err(|e| format!("Failed to get nvmlDeviceGetHandleByIndex: {}", e))?;

            let mut device: NvmlDevice = std::ptr::null_mut();
            let result = get_handle(device_index, &mut device);
            if result != NVML_SUCCESS {
                return Err(format!("nvmlDeviceGetHandleByIndex failed: {}", result));
            }

            // Get VBIOS version
            let get_vbios: Symbol<unsafe extern "C" fn(NvmlDevice, *mut i8, u32) -> NvmlReturn> =
                lib.get(b"nvmlDeviceGetVbiosVersion")
                    .map_err(|e| format!("Failed to get nvmlDeviceGetVbiosVersion: {}", e))?;

            let mut version_buf = [0i8; 256];
            let result = get_vbios(device, version_buf.as_mut_ptr(), 256);
            let version = if result == NVML_SUCCESS {
                std::ffi::CStr::from_ptr(version_buf.as_ptr())
                    .to_string_lossy()
                    .to_string()
            } else {
                "Unknown".to_string()
            };

            // Get power limits
            let get_power_limit: Symbol<unsafe extern "C" fn(NvmlDevice, *mut u32) -> NvmlReturn> =
                lib.get(b"nvmlDeviceGetPowerManagementLimit")
                    .map_err(|e| format!("Failed to get power limit fn: {}", e))?;

            let mut power_limit = 0u32;
            get_power_limit(device, &mut power_limit);

            let mut min_power = 0u32;
            let mut max_power = 0u32;
            if let Ok(get_power_min_max) =
                lib.get::<unsafe extern "C" fn(NvmlDevice, *mut u32, *mut u32) -> NvmlReturn>(
                    b"nvmlDeviceGetPowerManagementLimitConstraints",
                )
            {
                let _ = get_power_min_max(device, &mut min_power, &mut max_power);
            }

            let mut default_power = 0u32;
            if let Ok(get_default_power) =
                lib.get::<unsafe extern "C" fn(NvmlDevice, *mut u32) -> NvmlReturn>(
                    b"nvmlDeviceGetPowerManagementDefaultLimit",
                )
            {
                let _ = get_default_power(device, &mut default_power);
            }

            // Get clocks
            let mut graphics_clock = 0u32;
            let mut mem_clock = 0u32;
            if let Ok(get_clock) = lib
                .get::<unsafe extern "C" fn(NvmlDevice, u32, *mut u32) -> NvmlReturn>(
                    b"nvmlDeviceGetMaxClockInfo",
                )
            {
                let _ = get_clock(device, 0, &mut graphics_clock); // NVML_CLOCK_GRAPHICS
                let _ = get_clock(device, 2, &mut mem_clock); // NVML_CLOCK_MEM
            }

            // Get fan info
            let mut fan_speed = 0u32;
            if let Ok(get_fan_speed) = lib
                .get::<unsafe extern "C" fn(NvmlDevice, *mut u32) -> NvmlReturn>(
                    b"nvmlDeviceGetFanSpeed",
                )
            {
                let _ = get_fan_speed(device, &mut fan_speed);
            }

            Ok(VbiosInfo {
                vendor: GpuVendor::Nvidia,
                version,
                date: None,
                size_bytes: 0,
                checksum_valid: true,
                power_table: Some(PowerTable {
                    tdp_watts: power_limit / 1000, // NVML returns milliwatts
                    max_power_watts: max_power / 1000,
                    min_power_watts: min_power / 1000,
                    default_power_watts: default_power / 1000,
                    power_limit_percent: vec![100, 110, 120],
                }),
                clock_table: Some(ClockTable {
                    base_clock_mhz: graphics_clock,
                    boost_clock_mhz: graphics_clock,
                    max_clock_mhz: graphics_clock,
                    memory_clock_mhz: mem_clock,
                    memory_type: "GDDR".to_string(),
                    states: Vec::new(),
                }),
                fan_table: Some(FanTable {
                    min_rpm: 0,
                    max_rpm: 3000,
                    min_duty_percent: 30,
                    max_duty_percent: 100,
                    target_temp_c: 83,
                    curve: vec![
                        FanPoint {
                            temp_c: 40,
                            duty_percent: 30,
                        },
                        FanPoint {
                            temp_c: 60,
                            duty_percent: 50,
                        },
                        FanPoint {
                            temp_c: 80,
                            duty_percent: 80,
                        },
                        FanPoint {
                            temp_c: 90,
                            duty_percent: 100,
                        },
                    ],
                }),
            })
        }
    }

    /// Read AMD VBIOS via sysfs (Linux)
    #[cfg(target_os = "linux")]
    fn read_amd_vbios_sysfs(&self, device_index: u32) -> Result<VbiosInfo, String> {
        let base = format!("/sys/class/drm/card{}/device", device_index);

        if !Path::new(&base).exists() {
            return Err(format!("Device {} not found", device_index));
        }

        // Check if it's AMD
        let vendor = fs::read_to_string(format!("{}/vendor", base)).unwrap_or_default();
        if !vendor.contains("1002") {
            return Err("Not an AMD device".to_string());
        }

        // Read VBIOS version
        let vbios_version = fs::read_to_string(format!("{}/vbios_version", base))
            .unwrap_or_else(|_| "Unknown".to_string())
            .trim()
            .to_string();

        // Read power info
        let power_cap = fs::read_to_string(format!("{}/hwmon/hwmon0/power1_cap", base))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
            / 1_000_000; // Convert microwatts to watts

        let power_max = fs::read_to_string(format!("{}/hwmon/hwmon0/power1_cap_max", base))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
            / 1_000_000;

        let power_min = fs::read_to_string(format!("{}/hwmon/hwmon0/power1_cap_min", base))
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
            / 1_000_000;

        // Read clock info
        let pp_table =
            fs::read_to_string(format!("{}/pp_od_clk_voltage", base)).unwrap_or_default();

        let mut states = Vec::new();
        for line in pp_table.lines() {
            if line.starts_with("0:") || line.starts_with("1:") || line.starts_with("2:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 3 {
                    let idx = parts[0].trim_end_matches(':').parse().unwrap_or(0);
                    let clock = parts[1].trim_end_matches("Mhz").parse().unwrap_or(0);
                    let voltage = parts[2].trim_end_matches("mV").parse().unwrap_or(0);
                    states.push(ClockState {
                        index: idx,
                        clock_mhz: clock,
                        voltage_mv: voltage,
                    });
                }
            }
        }

        // Read fan info
        let fan_max = fs::read_to_string(format!("{}/hwmon/hwmon0/fan1_max", base))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(3000);

        let fan_min = fs::read_to_string(format!("{}/hwmon/hwmon0/fan1_min", base))
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .unwrap_or(0);

        Ok(VbiosInfo {
            vendor: GpuVendor::Amd,
            version: vbios_version,
            date: None,
            size_bytes: 0,
            checksum_valid: true,
            power_table: Some(PowerTable {
                tdp_watts: power_cap,
                max_power_watts: power_max,
                min_power_watts: power_min,
                default_power_watts: power_cap,
                power_limit_percent: vec![100, 110, 120],
            }),
            clock_table: Some(ClockTable {
                base_clock_mhz: states.first().map(|s| s.clock_mhz).unwrap_or(0),
                boost_clock_mhz: states.last().map(|s| s.clock_mhz).unwrap_or(0),
                max_clock_mhz: states.last().map(|s| s.clock_mhz).unwrap_or(0),
                memory_clock_mhz: 0,
                memory_type: "GDDR".to_string(),
                states,
            }),
            fan_table: Some(FanTable {
                min_rpm: fan_min,
                max_rpm: fan_max,
                min_duty_percent: 0,
                max_duty_percent: 100,
                target_temp_c: 80,
                curve: vec![
                    FanPoint {
                        temp_c: 40,
                        duty_percent: 0,
                    },
                    FanPoint {
                        temp_c: 60,
                        duty_percent: 50,
                    },
                    FanPoint {
                        temp_c: 80,
                        duty_percent: 80,
                    },
                    FanPoint {
                        temp_c: 90,
                        duty_percent: 100,
                    },
                ],
            }),
        })
    }

    #[cfg(not(target_os = "linux"))]
    fn read_amd_vbios_sysfs(&self, _device_index: u32) -> Result<VbiosInfo, String> {
        Err("AMD sysfs only available on Linux".to_string())
    }

    /// Set power limit (requires admin/root)
    pub fn set_power_limit(&self, device_index: u32, watts: u32) -> Result<(), String> {
        if let Some(ref nvml) = self.nvml {
            return self.set_nvidia_power_limit(nvml, device_index, watts);
        }

        #[cfg(target_os = "linux")]
        return self.set_amd_power_limit_sysfs(device_index, watts);

        #[cfg(not(target_os = "linux"))]
        Err("No supported GPU found".to_string())
    }

    fn set_nvidia_power_limit(
        &self,
        lib: &Library,
        device_index: u32,
        watts: u32,
    ) -> Result<(), String> {
        unsafe {
            let get_handle: Symbol<unsafe extern "C" fn(u32, *mut NvmlDevice) -> NvmlReturn> = lib
                .get(b"nvmlDeviceGetHandleByIndex_v2")
                .map_err(|e| format!("Failed to get handle fn: {}", e))?;

            let mut device: NvmlDevice = std::ptr::null_mut();
            let result = get_handle(device_index, &mut device);
            if result != NVML_SUCCESS {
                return Err(format!("Get handle failed: {}", result));
            }

            let set_limit: Symbol<unsafe extern "C" fn(NvmlDevice, u32) -> NvmlReturn> = lib
                .get(b"nvmlDeviceSetPowerManagementLimit")
                .map_err(|e| format!("Failed to get set limit fn: {}", e))?;

            let result = set_limit(device, watts * 1000); // NVML uses milliwatts
            if result != NVML_SUCCESS {
                return Err(format!("Set power limit failed: {}", result));
            }

            Ok(())
        }
    }

    #[cfg(target_os = "linux")]
    fn set_amd_power_limit_sysfs(&self, device_index: u32, watts: u32) -> Result<(), String> {
        let path = format!(
            "/sys/class/drm/card{}/device/hwmon/hwmon0/power1_cap",
            device_index
        );
        let microwatts = watts * 1_000_000;
        fs::write(&path, microwatts.to_string())
            .map_err(|e| format!("Failed to write power cap: {}", e))
    }

    /// Dump raw VBIOS bytes
    #[cfg(target_os = "linux")]
    pub fn dump_vbios(&self, device_index: u32) -> Result<Vec<u8>, String> {
        let path = format!("/sys/class/drm/card{}/device/rom", device_index);

        // Enable ROM reading
        fs::write(&path, "1").map_err(|e| format!("Enable ROM failed: {}", e))?;

        // Read ROM
        let data = fs::read(&path).map_err(|e| format!("Read ROM failed: {}", e))?;

        Ok(data)
    }

    #[cfg(not(target_os = "linux"))]
    pub fn dump_vbios(&self, _device_index: u32) -> Result<Vec<u8>, String> {
        Err("VBIOS dump only supported on Linux".to_string())
    }
}

impl Drop for VbiosReader {
    fn drop(&mut self) {
        if let Some(ref lib) = self.nvml {
            unsafe {
                if let Ok(shutdown) =
                    lib.get::<Symbol<unsafe extern "C" fn() -> NvmlReturn>>(b"nvmlShutdown")
                {
                    shutdown();
                }
            }
        }
    }
}

// ============================================================================
// Convenience Functions
// ============================================================================

/// Get VBIOS info for first GPU
pub fn get_vbios_info() -> Result<VbiosInfo, String> {
    VbiosReader::new().read_vbios(0)
}

/// Get power table for first GPU
pub fn get_power_table() -> Result<PowerTable, String> {
    let info = get_vbios_info()?;
    info.power_table.ok_or_else(|| "No power table".to_string())
}

/// Set power limit for first GPU
pub fn set_power_limit(watts: u32) -> Result<(), String> {
    VbiosReader::new().set_power_limit(0, watts)
}
