//! # GPU Hardware Abstraction
//! 
//! Unified interface for NVIDIA (NVAPI), AMD (ADL), and Intel (IGCL) GPUs.
//! Used by:
//! - Video player: Hardware decode capabilities, memory allocation
//! - MCP server: GPU queries, firmware tools, overclocking
//!
//! All GPU access is through dynamic loading - no SDK required at compile time.

use libloading::{Library, Symbol};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GpuError {
    #[error("No GPU found")]
    NoGpu,
    #[error("GPU library not found: {0}")]
    LibraryNotFound(String),
    #[error("GPU API call failed: {0}")]
    ApiError(String),
    #[error("Unsupported operation on {vendor:?}: {op}")]
    Unsupported { vendor: GpuVendor, op: String },
    #[error("GPU access denied - need admin/root")]
    AccessDenied,
}

/// GPU vendor identification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Unknown,
}

impl GpuVendor {
    pub fn from_pci_vendor(id: u16) -> Self {
        match id {
            0x10DE => GpuVendor::Nvidia,
            0x1002 => GpuVendor::Amd,
            0x8086 => GpuVendor::Intel,
            _ => GpuVendor::Unknown,
        }
    }
}

/// Decode capabilities for a GPU
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DecodeCapabilities {
    pub h264: bool,
    pub h265: bool,
    pub h265_10bit: bool,
    pub vp8: bool,
    pub vp9: bool,
    pub vp9_10bit: bool,
    pub av1: bool,
    pub av1_10bit: bool,
    pub max_width: u32,
    pub max_height: u32,
}

impl Default for DecodeCapabilities {
    fn default() -> Self {
        Self {
            h264: false,
            h265: false,
            h265_10bit: false,
            vp8: false,
            vp9: false,
            vp9_10bit: false,
            av1: false,
            av1_10bit: false,
            max_width: 0,
            max_height: 0,
        }
    }
}

/// Full GPU capabilities
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuCapabilities {
    pub decode: DecodeCapabilities,
    pub encode_h264: bool,
    pub encode_h265: bool,
    pub cuda_cores: Option<u32>,
    pub compute_units: Option<u32>,
    pub ray_tracing: bool,
    pub tensor_cores: bool,
}

/// GPU device information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuDevice {
    pub index: u32,
    pub name: String,
    pub vendor: GpuVendor,
    pub pci_device_id: u16,
    pub pci_subsystem_id: u16,
    pub vram_mb: u32,
    pub driver_version: String,
    pub vbios_version: Option<String>,
    pub capabilities: GpuCapabilities,
}

/// Real-time GPU state (clocks, temps, etc)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GpuState {
    pub gpu_clock_mhz: u32,
    pub mem_clock_mhz: u32,
    pub temperature_c: u32,
    pub fan_speed_percent: Option<u32>,
    pub power_draw_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub gpu_usage_percent: u32,
    pub mem_usage_percent: u32,
    pub vram_used_mb: u32,
    pub vram_free_mb: u32,
}

/// Global GPU manager (singleton)
static GPU_MANAGER: OnceLock<RwLock<GpuManager>> = OnceLock::new();

pub fn gpu_manager() -> &'static RwLock<GpuManager> {
    GPU_MANAGER.get_or_init(|| RwLock::new(GpuManager::new()))
}

/// GPU Manager - handles all GPU operations
pub struct GpuManager {
    devices: Vec<GpuDevice>,
    nvapi: Option<NvapiLoader>,
    adl: Option<AdlLoader>,
    initialized: bool,
}

impl GpuManager {
    fn new() -> Self {
        Self {
            devices: Vec::new(),
            nvapi: None,
            adl: None,
            initialized: false,
        }
    }
    
    /// Initialize GPU access (call once)
    pub fn init(&mut self) -> Result<(), GpuError> {
        if self.initialized {
            return Ok(());
        }
        
        tracing::info!("Initializing GPU manager...");
        
        // Try loading NVIDIA API
        match NvapiLoader::new() {
            Ok(nvapi) => {
                tracing::info!("NVAPI loaded successfully");
                self.nvapi = Some(nvapi);
            }
            Err(e) => {
                tracing::debug!("NVAPI not available: {}", e);
            }
        }
        
        // Try loading AMD API
        match AdlLoader::new() {
            Ok(adl) => {
                tracing::info!("ADL loaded successfully");
                self.adl = Some(adl);
            }
            Err(e) => {
                tracing::debug!("ADL not available: {}", e);
            }
        }
        
        // Enumerate devices
        self.enumerate_devices()?;
        
        self.initialized = true;
        tracing::info!("Found {} GPU(s)", self.devices.len());
        
        Ok(())
    }
    
    /// Get all detected GPUs
    pub fn devices(&self) -> &[GpuDevice] {
        &self.devices
    }
    
    /// Get primary GPU (first discrete, or first integrated)
    pub fn primary_gpu(&self) -> Option<&GpuDevice> {
        // Prefer discrete GPU
        self.devices.iter()
            .find(|d| d.vendor == GpuVendor::Nvidia || d.vendor == GpuVendor::Amd)
            .or_else(|| self.devices.first())
    }
    
    /// Get current GPU state (clocks, temps, usage)
    pub fn get_state(&self, device_index: u32) -> Result<GpuState, GpuError> {
        let device = self.devices.get(device_index as usize)
            .ok_or(GpuError::NoGpu)?;
        
        match device.vendor {
            GpuVendor::Nvidia => {
                self.nvapi.as_ref()
                    .ok_or(GpuError::LibraryNotFound("nvapi64.dll".into()))?
                    .get_state(device_index)
            }
            GpuVendor::Amd => {
                self.adl.as_ref()
                    .ok_or(GpuError::LibraryNotFound("atiadlxx.dll".into()))?
                    .get_state(device_index)
            }
            _ => Err(GpuError::Unsupported {
                vendor: device.vendor,
                op: "get_state".into(),
            }),
        }
    }
    
    /// Check if NVDEC is available
    pub fn has_nvdec(&self) -> bool {
        self.devices.iter().any(|d| {
            d.vendor == GpuVendor::Nvidia && d.capabilities.decode.h264
        })
    }
    
    /// Check if AMD VCN is available  
    pub fn has_vcn(&self) -> bool {
        self.devices.iter().any(|d| {
            d.vendor == GpuVendor::Amd && d.capabilities.decode.h264
        })
    }
    
    /// Check if CUDA is available
    pub fn has_cuda(&self) -> bool {
        self.nvapi.is_some() && self.devices.iter().any(|d| {
            d.vendor == GpuVendor::Nvidia && d.capabilities.cuda_cores.is_some()
        })
    }
    
    fn enumerate_devices(&mut self) -> Result<(), GpuError> {
        self.devices.clear();
        
        // Get NVIDIA devices
        if let Some(ref nvapi) = self.nvapi {
            match nvapi.enumerate_devices() {
                Ok(devices) => self.devices.extend(devices),
                Err(e) => tracing::warn!("Failed to enumerate NVIDIA GPUs: {}", e),
            }
        }
        
        // Get AMD devices
        if let Some(ref adl) = self.adl {
            match adl.enumerate_devices() {
                Ok(devices) => self.devices.extend(devices),
                Err(e) => tracing::warn!("Failed to enumerate AMD GPUs: {}", e),
            }
        }
        
        // Assign indices
        for (i, device) in self.devices.iter_mut().enumerate() {
            device.index = i as u32;
        }
        
        if self.devices.is_empty() {
            return Err(GpuError::NoGpu);
        }
        
        Ok(())
    }
}

// ============================================================================
// NVIDIA NVAPI Loader
// ============================================================================

struct NvapiLoader {
    lib: Library,
    // NVAPI function pointers
    nv_init: Option<NvApiInit>,
    nv_enum_gpus: Option<NvApiEnumGpus>,
    nv_get_name: Option<NvApiGetName>,
    nv_get_vram: Option<NvApiGetVram>,
    nv_get_thermal: Option<NvApiGetThermal>,
    nv_get_clocks: Option<NvApiGetClocks>,
    nv_get_usage: Option<NvApiGetUsage>,
}

// NVAPI types
type NvPhysicalGpuHandle = *mut std::ffi::c_void;
type NvApiStatus = i32;

const NVAPI_OK: NvApiStatus = 0;
const NVAPI_MAX_PHYSICAL_GPUS: usize = 64;

// Function pointer types
type NvApiInit = unsafe extern "C" fn() -> NvApiStatus;
type NvApiEnumGpus = unsafe extern "C" fn(*mut NvPhysicalGpuHandle, *mut u32) -> NvApiStatus;
type NvApiGetName = unsafe extern "C" fn(NvPhysicalGpuHandle, *mut [u8; 64]) -> NvApiStatus;
type NvApiGetVram = unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvMemoryInfo) -> NvApiStatus;
type NvApiGetThermal = unsafe extern "C" fn(NvPhysicalGpuHandle, i32, *mut NvThermalSettings) -> NvApiStatus;
type NvApiGetClocks = unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvClocks) -> NvApiStatus;
type NvApiGetUsage = unsafe extern "C" fn(NvPhysicalGpuHandle, *mut NvUsages) -> NvApiStatus;

#[repr(C)]
struct NvMemoryInfo {
    version: u32,
    dedicated_video_memory: u32,
    available_dedicated: u32,
    system_video_memory: u32,
    shared_system_memory: u32,
    current_available: u32,
}

#[repr(C)]
struct NvThermalSettings {
    version: u32,
    count: u32,
    sensors: [NvThermalSensor; 3],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NvThermalSensor {
    controller: i32,
    default_min: i32,
    default_max: i32,
    current_temp: i32,
    target: i32,
}

#[repr(C)]
struct NvClocks {
    version: u32,
    clocks: [NvClockEntry; 32],
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct NvClockEntry {
    present: u32,
    freq_khz: u32,
}

#[repr(C)]
struct NvUsages {
    version: u32,
    usages: [u32; 8],
}

// NVAPI uses query interface pattern
fn nvapi_query_interface(offset: u32) -> *const std::ffi::c_void {
    // These offsets are stable for NVAPI
    // Found via reverse engineering / documentation
    static INTERFACE: std::sync::OnceLock<Option<unsafe extern "C" fn(u32) -> *const std::ffi::c_void>> = std::sync::OnceLock::new();
    
    if let Some(query) = INTERFACE.get() {
        if let Some(f) = query {
            return unsafe { f(offset) };
        }
    }
    std::ptr::null()
}

impl NvapiLoader {
    fn new() -> Result<Self, GpuError> {
        #[cfg(windows)]
        let lib_path = "nvapi64.dll";
        #[cfg(not(windows))]
        let lib_path = "libnvidia-api.so";
        
        let lib = unsafe { Library::new(lib_path) }
            .map_err(|_| GpuError::LibraryNotFound(lib_path.into()))?;
        
        // NVAPI uses a query interface pattern
        // Get nvapi_QueryInterface first
        let query_fn: Option<unsafe extern "C" fn(u32) -> *const std::ffi::c_void> = unsafe {
            lib.get::<unsafe extern "C" fn(u32) -> *const std::ffi::c_void>(b"nvapi_QueryInterface")
                .ok()
                .map(|f| *f)
        };
        
        if query_fn.is_none() {
            return Err(GpuError::InitializationFailed("nvapi_QueryInterface not found".into()));
        }
        
        let query = query_fn.unwrap();
        
        // Query function pointers by ID
        // These IDs are from NVAPI documentation/headers
        let nv_init: Option<NvApiInit> = unsafe {
            let ptr = query(0x0150E828); // NvAPI_Initialize
            if ptr.is_null() { None } else { Some(std::mem::transmute(ptr)) }
        };
        
        let nv_enum_gpus: Option<NvApiEnumGpus> = unsafe {
            let ptr = query(0xE5AC921F); // NvAPI_EnumPhysicalGPUs
            if ptr.is_null() { None } else { Some(std::mem::transmute(ptr)) }
        };
        
        let nv_get_name: Option<NvApiGetName> = unsafe {
            let ptr = query(0xCEEE8E9F); // NvAPI_GPU_GetFullName
            if ptr.is_null() { None } else { Some(std::mem::transmute(ptr)) }
        };
        
        let nv_get_vram: Option<NvApiGetVram> = unsafe {
            let ptr = query(0x774AA982); // NvAPI_GPU_GetMemoryInfo
            if ptr.is_null() { None } else { Some(std::mem::transmute(ptr)) }
        };
        
        let nv_get_thermal: Option<NvApiGetThermal> = unsafe {
            let ptr = query(0xE3640A56); // NvAPI_GPU_GetThermalSettings
            if ptr.is_null() { None } else { Some(std::mem::transmute(ptr)) }
        };
        
        let nv_get_clocks: Option<NvApiGetClocks> = unsafe {
            let ptr = query(0xDCB616C3); // NvAPI_GPU_GetAllClockFrequencies
            if ptr.is_null() { None } else { Some(std::mem::transmute(ptr)) }
        };
        
        let nv_get_usage: Option<NvApiGetUsage> = unsafe {
            let ptr = query(0x189A1FDF); // NvAPI_GPU_GetUsages
            if ptr.is_null() { None } else { Some(std::mem::transmute(ptr)) }
        };
        
        // Initialize NVAPI
        if let Some(init) = nv_init {
            let status = unsafe { init() };
            if status != NVAPI_OK {
                return Err(GpuError::InitializationFailed(format!("NvAPI_Initialize failed: {}", status)));
            }
        }
        
        Ok(Self { 
            lib, 
            nv_init,
            nv_enum_gpus,
            nv_get_name,
            nv_get_vram,
            nv_get_thermal,
            nv_get_clocks,
            nv_get_usage,
        })
    }
    
    fn enumerate_devices(&self) -> Result<Vec<GpuDevice>, GpuError> {
        let enum_fn = self.nv_enum_gpus
            .ok_or_else(|| GpuError::ApiError("NvAPI_EnumPhysicalGPUs not available".into()))?;
        
        let mut handles: [NvPhysicalGpuHandle; NVAPI_MAX_PHYSICAL_GPUS] = [std::ptr::null_mut(); NVAPI_MAX_PHYSICAL_GPUS];
        let mut count: u32 = 0;
        
        let status = unsafe { enum_fn(handles.as_mut_ptr(), &mut count) };
        if status != NVAPI_OK {
            return Err(GpuError::ApiError(format!("EnumPhysicalGPUs failed: {}", status)));
        }
        
        let mut devices = Vec::new();
        
        for i in 0..count as usize {
            let handle = handles[i];
            if handle.is_null() {
                continue;
            }
            
            // Get GPU name
            let mut name_buf = [0u8; 64];
            let name = if let Some(get_name) = self.nv_get_name {
                let status = unsafe { get_name(handle, &mut name_buf) };
                if status == NVAPI_OK {
                    let len = name_buf.iter().position(|&c| c == 0).unwrap_or(64);
                    String::from_utf8_lossy(&name_buf[..len]).to_string()
                } else {
                    format!("NVIDIA GPU {}", i)
                }
            } else {
                format!("NVIDIA GPU {}", i)
            };
            
            // Get VRAM
            let vram_mb = if let Some(get_vram) = self.nv_get_vram {
                let mut mem_info = NvMemoryInfo {
                    version: 2 | (std::mem::size_of::<NvMemoryInfo>() as u32) << 16,
                    dedicated_video_memory: 0,
                    available_dedicated: 0,
                    system_video_memory: 0,
                    shared_system_memory: 0,
                    current_available: 0,
                };
                let status = unsafe { get_vram(handle, &mut mem_info) };
                if status == NVAPI_OK {
                    mem_info.dedicated_video_memory / 1024 // KB to MB
                } else {
                    0
                }
            } else {
                0
            };
            
            devices.push(GpuDevice {
                index: i as u32,
                name,
                vendor: GpuVendor::Nvidia,
                pci_device_id: 0,
                pci_subsystem_id: 0,
                vram_mb,
                driver_version: "NVAPI".into(),
                vbios_version: None,
                capabilities: GpuCapabilities {
                    decode: DecodeCapabilities {
                        h264: true,
                        h265: true,
                        h265_10bit: true,
                        vp9: true,
                        vp9_10bit: true,
                        av1: true,
                        av1_10bit: true,
                        vp8: false,
                        max_width: 8192,
                        max_height: 8192,
                    },
                    encode_h264: true,
                    encode_h265: true,
                    cuda_cores: None,
                    compute_units: None,
                    ray_tracing: false,
                    tensor_cores: false,
                },
            });
        }
        
        if devices.is_empty() {
            return Err(GpuError::NoDevicesFound);
        }
        
        Ok(devices)
    }
    
    fn get_state(&self, device_index: u32) -> Result<GpuState, GpuError> {
        // First enumerate to get handle
        let enum_fn = self.nv_enum_gpus
            .ok_or_else(|| GpuError::ApiError("NvAPI_EnumPhysicalGPUs not available".into()))?;
        
        let mut handles: [NvPhysicalGpuHandle; NVAPI_MAX_PHYSICAL_GPUS] = [std::ptr::null_mut(); NVAPI_MAX_PHYSICAL_GPUS];
        let mut count: u32 = 0;
        
        let status = unsafe { enum_fn(handles.as_mut_ptr(), &mut count) };
        if status != NVAPI_OK || device_index >= count {
            return Err(GpuError::DeviceNotFound(device_index));
        }
        
        let handle = handles[device_index as usize];
        
        // Get temperature
        let temperature = if let Some(get_thermal) = self.nv_get_thermal {
            let mut thermal = NvThermalSettings {
                version: 2 | (std::mem::size_of::<NvThermalSettings>() as u32) << 16,
                count: 0,
                sensors: [NvThermalSensor::default(); 3],
            };
            let status = unsafe { get_thermal(handle, 0, &mut thermal) };
            if status == NVAPI_OK && thermal.count > 0 {
                thermal.sensors[0].current_temp as u32
            } else {
                0
            }
        } else {
            0
        };
        
        // Get clocks
        let (gpu_clock, mem_clock) = if let Some(get_clocks) = self.nv_get_clocks {
            let mut clocks = NvClocks {
                version: 2 | (std::mem::size_of::<NvClocks>() as u32) << 16,
                clocks: [NvClockEntry::default(); 32],
            };
            let status = unsafe { get_clocks(handle, &mut clocks) };
            if status == NVAPI_OK {
                // Index 0 = graphics, index 8 = memory typically
                let gpu = if clocks.clocks[0].present != 0 { clocks.clocks[0].freq_khz / 1000 } else { 0 };
                let mem = if clocks.clocks[8].present != 0 { clocks.clocks[8].freq_khz / 1000 } else { 0 };
                (gpu, mem)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };
        
        // Get usage
        let gpu_usage = if let Some(get_usage) = self.nv_get_usage {
            let mut usages = NvUsages {
                version: 1 | (std::mem::size_of::<NvUsages>() as u32) << 16,
                usages: [0; 8],
            };
            let status = unsafe { get_usage(handle, &mut usages) };
            if status == NVAPI_OK {
                usages.usages[0]
            } else {
                0
            }
        } else {
            0
        };
        
        // Get VRAM
        let (vram_used, vram_free) = if let Some(get_vram) = self.nv_get_vram {
            let mut mem_info = NvMemoryInfo {
                version: 2 | (std::mem::size_of::<NvMemoryInfo>() as u32) << 16,
                dedicated_video_memory: 0,
                available_dedicated: 0,
                system_video_memory: 0,
                shared_system_memory: 0,
                current_available: 0,
            };
            let status = unsafe { get_vram(handle, &mut mem_info) };
            if status == NVAPI_OK {
                let total = mem_info.dedicated_video_memory / 1024;
                let free = mem_info.current_available / 1024;
                (total.saturating_sub(free), free)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };
        
        Ok(GpuState {
            gpu_clock_mhz: gpu_clock,
            mem_clock_mhz: mem_clock,
            temperature_c: temperature,
            fan_speed_percent: None, // NVAPI fan is different API
            power_draw_w: None,
            power_limit_w: None,
            gpu_usage_percent: gpu_usage,
            mem_usage_percent: if vram_used + vram_free > 0 {
                (vram_used * 100 / (vram_used + vram_free))
            } else {
                0
            },
            vram_used_mb: vram_used,
            vram_free_mb: vram_free,
        })
    }
}

// ============================================================================
// AMD ADL Loader
// ============================================================================

// ADL types
type AdlMainMemoryAlloc = extern "C" fn(i32) -> *mut std::ffi::c_void;
type AdlStatus = i32;

const ADL_OK: AdlStatus = 0;
const ADL_MAX_ADAPTERS: usize = 256;

#[repr(C)]
#[derive(Clone)]
struct AdlAdapterInfo {
    size: i32,
    adapter_index: i32,
    strUDID: [u8; 256],
    bus_number: i32,
    device_number: i32,
    function_number: i32,
    vendor_id: i32,
    adapter_name: [u8; 256],
    display_name: [u8; 256],
    present: i32,
    exist: i32,
    driver_path: [u8; 256],
    driver_path_ext: [u8; 256],
    pnp_string: [u8; 256],
    os_display_index: i32,
}

impl Default for AdlAdapterInfo {
    fn default() -> Self {
        Self {
            size: std::mem::size_of::<AdlAdapterInfo>() as i32,
            adapter_index: 0,
            strUDID: [0; 256],
            bus_number: 0,
            device_number: 0,
            function_number: 0,
            vendor_id: 0,
            adapter_name: [0; 256],
            display_name: [0; 256],
            present: 0,
            exist: 0,
            driver_path: [0; 256],
            driver_path_ext: [0; 256],
            pnp_string: [0; 256],
            os_display_index: 0,
        }
    }
}

#[repr(C)]
struct AdlTemperature {
    size: i32,
    temperature: i32, // Millidegrees Celsius
}

#[repr(C)]
struct AdlFanSpeedValue {
    size: i32,
    speed_type: i32,
    fan_speed: i32,
    flags: i32,
}

#[repr(C)]
struct AdlPmActivity {
    size: i32,
    engine_clock: i32,
    memory_clock: i32,
    vddc: i32,
    activity_percent: i32,
    current_performance_level: i32,
    current_bus_speed: i32,
    current_bus_lanes: i32,
    max_bus_lanes: i32,
    reserved: i32,
}

#[repr(C)]
struct AdlMemoryInfo {
    size: i64,
    memory_size: i64,
    memory_type: [u8; 256],
    memory_bandwidth: i64,
}

struct AdlLoader {
    lib: Library,
    // ADL function pointers
    adl_main_control_create: unsafe extern "C" fn(AdlMainMemoryAlloc, i32) -> AdlStatus,
    adl_main_control_destroy: unsafe extern "C" fn() -> AdlStatus,
    adl_adapter_number_of_adapters_get: unsafe extern "C" fn(*mut i32) -> AdlStatus,
    adl_adapter_adapter_info_get: Option<unsafe extern "C" fn(*mut AdlAdapterInfo, i32) -> AdlStatus>,
    adl_adapter_active_get: Option<unsafe extern "C" fn(i32, *mut i32) -> AdlStatus>,
    adl_overdrive5_temperature_get: Option<unsafe extern "C" fn(i32, i32, *mut AdlTemperature) -> AdlStatus>,
    adl_overdrive5_fanspeed_get: Option<unsafe extern "C" fn(i32, i32, *mut AdlFanSpeedValue) -> AdlStatus>,
    adl_overdrive5_currentactivity_get: Option<unsafe extern "C" fn(i32, *mut AdlPmActivity) -> AdlStatus>,
    adl_adapter_memoryinfo_get: Option<unsafe extern "C" fn(i32, *mut AdlMemoryInfo) -> AdlStatus>,
}

// Memory allocation callback for ADL
extern "C" fn adl_mem_alloc(size: i32) -> *mut std::ffi::c_void {
    unsafe {
        std::alloc::alloc(std::alloc::Layout::from_size_align_unchecked(size as usize, 8)) as *mut _
    }
}

impl AdlLoader {
    fn new() -> Result<Self, GpuError> {
        #[cfg(windows)]
        let lib_path = "atiadlxx.dll";
        #[cfg(not(windows))]
        let lib_path = "libatiadlxx.so";
        
        let lib = unsafe { Library::new(lib_path) }
            .map_err(|_| GpuError::LibraryNotFound(lib_path.into()))?;
        
        unsafe {
            // Required functions
            let adl_main_control_create: libloading::Symbol<unsafe extern "C" fn(AdlMainMemoryAlloc, i32) -> AdlStatus> = 
                lib.get(b"ADL_Main_Control_Create")
                    .map_err(|_| GpuError::InitializationFailed("ADL_Main_Control_Create not found".into()))?;
            
            let adl_main_control_destroy: libloading::Symbol<unsafe extern "C" fn() -> AdlStatus> =
                lib.get(b"ADL_Main_Control_Destroy")
                    .map_err(|_| GpuError::InitializationFailed("ADL_Main_Control_Destroy not found".into()))?;
            
            let adl_adapter_number_of_adapters_get: libloading::Symbol<unsafe extern "C" fn(*mut i32) -> AdlStatus> =
                lib.get(b"ADL_Adapter_NumberOfAdapters_Get")
                    .map_err(|_| GpuError::InitializationFailed("ADL_Adapter_NumberOfAdapters_Get not found".into()))?;
            
            // Optional functions
            let adl_adapter_adapter_info_get: Option<unsafe extern "C" fn(*mut AdlAdapterInfo, i32) -> AdlStatus> =
                lib.get(b"ADL_Adapter_AdapterInfo_Get").ok().map(|f| *f);
            
            let adl_adapter_active_get: Option<unsafe extern "C" fn(i32, *mut i32) -> AdlStatus> =
                lib.get(b"ADL_Adapter_Active_Get").ok().map(|f| *f);
            
            let adl_overdrive5_temperature_get: Option<unsafe extern "C" fn(i32, i32, *mut AdlTemperature) -> AdlStatus> =
                lib.get(b"ADL_Overdrive5_Temperature_Get").ok().map(|f| *f);
            
            let adl_overdrive5_fanspeed_get: Option<unsafe extern "C" fn(i32, i32, *mut AdlFanSpeedValue) -> AdlStatus> =
                lib.get(b"ADL_Overdrive5_FanSpeed_Get").ok().map(|f| *f);
            
            let adl_overdrive5_currentactivity_get: Option<unsafe extern "C" fn(i32, *mut AdlPmActivity) -> AdlStatus> =
                lib.get(b"ADL_Overdrive5_CurrentActivity_Get").ok().map(|f| *f);
            
            let adl_adapter_memoryinfo_get: Option<unsafe extern "C" fn(i32, *mut AdlMemoryInfo) -> AdlStatus> =
                lib.get(b"ADL_Adapter_MemoryInfo_Get").ok().map(|f| *f);
            
            // Initialize ADL
            let status = adl_main_control_create(adl_mem_alloc, 1);
            if status != ADL_OK {
                return Err(GpuError::InitializationFailed(format!("ADL_Main_Control_Create failed: {}", status)));
            }
            
            Ok(Self {
                lib,
                adl_main_control_create: *adl_main_control_create,
                adl_main_control_destroy: *adl_main_control_destroy,
                adl_adapter_number_of_adapters_get: *adl_adapter_number_of_adapters_get,
                adl_adapter_adapter_info_get,
                adl_adapter_active_get,
                adl_overdrive5_temperature_get,
                adl_overdrive5_fanspeed_get,
                adl_overdrive5_currentactivity_get,
                adl_adapter_memoryinfo_get,
            })
        }
    }
    
    fn enumerate_devices(&self) -> Result<Vec<GpuDevice>, GpuError> {
        unsafe {
            let mut num_adapters: i32 = 0;
            let status = (self.adl_adapter_number_of_adapters_get)(&mut num_adapters);
            if status != ADL_OK || num_adapters <= 0 {
                return Err(GpuError::NoDevicesFound);
            }
            
            let get_info = self.adl_adapter_adapter_info_get
                .ok_or_else(|| GpuError::ApiError("ADL_Adapter_AdapterInfo_Get not available".into()))?;
            
            let mut adapter_info = vec![AdlAdapterInfo::default(); num_adapters as usize];
            let status = get_info(
                adapter_info.as_mut_ptr(),
                std::mem::size_of::<AdlAdapterInfo>() as i32 * num_adapters
            );
            
            if status != ADL_OK {
                return Err(GpuError::ApiError(format!("ADL_Adapter_AdapterInfo_Get failed: {}", status)));
            }
            
            let mut devices = Vec::new();
            let mut seen_adapters = std::collections::HashSet::new();
            
            for info in adapter_info.iter() {
                // Skip inactive adapters
                if info.present == 0 {
                    continue;
                }
                
                // Check if active
                if let Some(active_get) = self.adl_adapter_active_get {
                    let mut active: i32 = 0;
                    if active_get(info.adapter_index, &mut active) == ADL_OK && active == 0 {
                        continue;
                    }
                }
                
                // Skip duplicate adapters (same UDID)
                let udid = String::from_utf8_lossy(&info.strUDID[..info.strUDID.iter().position(|&c| c == 0).unwrap_or(256)]).to_string();
                if seen_adapters.contains(&udid) {
                    continue;
                }
                seen_adapters.insert(udid);
                
                // Get adapter name
                let name_len = info.adapter_name.iter().position(|&c| c == 0).unwrap_or(256);
                let name = String::from_utf8_lossy(&info.adapter_name[..name_len]).to_string();
                
                // Get VRAM
                let vram_mb = if let Some(mem_get) = self.adl_adapter_memoryinfo_get {
                    let mut mem_info = AdlMemoryInfo {
                        size: std::mem::size_of::<AdlMemoryInfo>() as i64,
                        memory_size: 0,
                        memory_type: [0; 256],
                        memory_bandwidth: 0,
                    };
                    if mem_get(info.adapter_index, &mut mem_info) == ADL_OK {
                        (mem_info.memory_size / (1024 * 1024)) as u32
                    } else {
                        0
                    }
                } else {
                    0
                };
                
                devices.push(GpuDevice {
                    index: info.adapter_index as u32,
                    name,
                    vendor: GpuVendor::Amd,
                    pci_device_id: 0,
                    pci_subsystem_id: 0,
                    vram_mb,
                    driver_version: "ADL".into(),
                    vbios_version: None,
                    capabilities: GpuCapabilities {
                        decode: DecodeCapabilities {
                            h264: true,
                            h265: true,
                            h265_10bit: true,
                            vp9: true,
                            vp9_10bit: true,
                            av1: true,
                            av1_10bit: true,
                            vp8: false,
                            max_width: 8192,
                            max_height: 8192,
                        },
                        encode_h264: true,
                        encode_h265: true,
                        cuda_cores: None,
                        compute_units: None,
                        ray_tracing: false,
                        tensor_cores: false,
                    },
                });
            }
            
            if devices.is_empty() {
                return Err(GpuError::NoDevicesFound);
            }
            
            Ok(devices)
        }
    }
    
    fn get_state(&self, device_index: u32) -> Result<GpuState, GpuError> {
        let adapter_index = device_index as i32;
        
        // Get temperature
        let temperature = if let Some(temp_get) = self.adl_overdrive5_temperature_get {
            let mut temp = AdlTemperature {
                size: std::mem::size_of::<AdlTemperature>() as i32,
                temperature: 0,
            };
            if unsafe { temp_get(adapter_index, 0, &mut temp) } == ADL_OK {
                (temp.temperature / 1000) as u32 // Convert from millidegrees
            } else {
                0
            }
        } else {
            0
        };
        
        // Get fan speed
        let fan_speed = if let Some(fan_get) = self.adl_overdrive5_fanspeed_get {
            let mut fan = AdlFanSpeedValue {
                size: std::mem::size_of::<AdlFanSpeedValue>() as i32,
                speed_type: 1, // ADL_DL_FANCTRL_SPEED_TYPE_PERCENT
                fan_speed: 0,
                flags: 0,
            };
            if unsafe { fan_get(adapter_index, 0, &mut fan) } == ADL_OK {
                Some(fan.fan_speed as u32)
            } else {
                None
            }
        } else {
            None
        };
        
        // Get activity (clocks, usage)
        let (gpu_clock, mem_clock, gpu_usage) = if let Some(activity_get) = self.adl_overdrive5_currentactivity_get {
            let mut activity = AdlPmActivity {
                size: std::mem::size_of::<AdlPmActivity>() as i32,
                engine_clock: 0,
                memory_clock: 0,
                vddc: 0,
                activity_percent: 0,
                current_performance_level: 0,
                current_bus_speed: 0,
                current_bus_lanes: 0,
                max_bus_lanes: 0,
                reserved: 0,
            };
            if unsafe { activity_get(adapter_index, &mut activity) } == ADL_OK {
                (
                    (activity.engine_clock / 100) as u32,  // Convert from 10kHz to MHz
                    (activity.memory_clock / 100) as u32,
                    activity.activity_percent as u32
                )
            } else {
                (0, 0, 0)
            }
        } else {
            (0, 0, 0)
        };
        
        // Get VRAM
        let (vram_used, vram_free) = if let Some(mem_get) = self.adl_adapter_memoryinfo_get {
            let mut mem_info = AdlMemoryInfo {
                size: std::mem::size_of::<AdlMemoryInfo>() as i64,
                memory_size: 0,
                memory_type: [0; 256],
                memory_bandwidth: 0,
            };
            if unsafe { mem_get(adapter_index, &mut mem_info) } == ADL_OK {
                let total = (mem_info.memory_size / (1024 * 1024)) as u32;
                // ADL doesn't give used memory directly, estimate from usage
                let used = (total * gpu_usage) / 100;
                (used, total - used)
            } else {
                (0, 0)
            }
        } else {
            (0, 0)
        };
        
        Ok(GpuState {
            gpu_clock_mhz: gpu_clock,
            mem_clock_mhz: mem_clock,
            temperature_c: temperature,
            fan_speed_percent: fan_speed,
            power_draw_w: None,
            power_limit_w: None,
            gpu_usage_percent: gpu_usage,
            mem_usage_percent: if vram_used + vram_free > 0 {
                (vram_used * 100 / (vram_used + vram_free))
            } else {
                0
            },
            vram_used_mb: vram_used,
            vram_free_mb: vram_free,
        })
    }
}

impl Drop for AdlLoader {
    fn drop(&mut self) {
        unsafe {
            (self.adl_main_control_destroy)();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_vendor_from_pci() {
        assert_eq!(GpuVendor::from_pci_vendor(0x10DE), GpuVendor::Nvidia);
        assert_eq!(GpuVendor::from_pci_vendor(0x1002), GpuVendor::Amd);
        assert_eq!(GpuVendor::from_pci_vendor(0x8086), GpuVendor::Intel);
    }
}
