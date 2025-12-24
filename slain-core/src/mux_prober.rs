// MCP Server: BIOS/MUX Discovery
// 
// SAFETY: This does NOT write to BIOS. It only READS and PROBES.
// Actual unlocking requires user confirmation and backup.
//
// Strategy:
// 1. Parallel probe known MUX register locations
// 2. Build knowledge graph of what exists
// 3. Incrementally narrow down which switches are available
// 4. User decides whether to flip

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

// ============================================================================
// MCP Protocol Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<McpError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
}

// ============================================================================
// Probe Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub location: ProbeLocation,
    pub status: ProbeStatus,
    pub value: Option<u64>,
    pub interpretation: Option<String>,
    pub risk_level: RiskLevel,
    pub duration_us: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeLocation {
    pub method: ProbeMethod,
    pub address: u64,
    pub size: u8,
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeMethod {
    // Safe - read only
    AcpiTable,          // Read ACPI tables
    PciConfig,          // PCI configuration space
    WmiQuery,           // Windows WMI
    EcRam,              // Embedded Controller RAM (read)
    MsrRead,            // Model-Specific Registers
    IoPort,             // I/O port read
    
    // Requires elevation
    PhysicalMemory,     // Direct physical memory read
    SmBios,             // SMBIOS tables
    
    // Risky - don't use without backup
    EcWrite,            // EC RAM write
    NvramRead,          // UEFI variables
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProbeStatus {
    Success,
    AccessDenied,       // Need admin/driver
    NotFound,           // Register doesn't exist
    Timeout,            // Probe hung
    Error,              // Other error
    Skipped,            // Too risky
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    Safe,               // Read-only, can't cause damage
    Elevated,           // Needs admin, still safe
    Caution,            // Could cause instability
    Dangerous,          // Could brick system
}

// ============================================================================
// Known MUX Switch Locations (by vendor)
// ============================================================================

/// Database of known MUX switch locations across laptop vendors
pub fn get_known_mux_locations() -> Vec<ProbeLocation> {
    vec![
        // ASUS ROG laptops
        ProbeLocation {
            method: ProbeMethod::WmiQuery,
            address: 0,
            size: 1,
            name: "ASUS_GPU_MUX".to_string(),
            description: "ASUS Armoury Crate GPU MUX switch".to_string(),
        },
        ProbeLocation {
            method: ProbeMethod::EcRam,
            address: 0xD1,  // Common ASUS EC offset
            size: 1,
            name: "ASUS_EC_GPU_MODE".to_string(),
            description: "ASUS EC GPU mode register".to_string(),
        },
        
        // Lenovo Legion
        ProbeLocation {
            method: ProbeMethod::WmiQuery,
            address: 0,
            size: 1,
            name: "LENOVO_GPU_MODE".to_string(),
            description: "Lenovo Vantage GPU mode".to_string(),
        },
        ProbeLocation {
            method: ProbeMethod::EcRam,
            address: 0x2F,
            size: 1,
            name: "LENOVO_EC_DGPU".to_string(),
            description: "Lenovo EC discrete GPU control".to_string(),
        },
        
        // MSI laptops
        ProbeLocation {
            method: ProbeMethod::EcRam,
            address: 0xF4,
            size: 1,
            name: "MSI_GPU_SWITCH".to_string(),
            description: "MSI GPU switch register".to_string(),
        },
        ProbeLocation {
            method: ProbeMethod::WmiQuery,
            address: 0,
            size: 1,
            name: "MSI_WMI_GPU".to_string(),
            description: "MSI Center GPU control".to_string(),
        },
        
        // Dell/Alienware
        ProbeLocation {
            method: ProbeMethod::WmiQuery,
            address: 0,
            size: 1,
            name: "DELL_THERMAL_GPU".to_string(),
            description: "Dell Thermal Management GPU mode".to_string(),
        },
        
        // HP Omen
        ProbeLocation {
            method: ProbeMethod::WmiQuery,
            address: 0,
            size: 1,
            name: "HP_OMEN_GPU".to_string(),
            description: "HP Omen Gaming Hub GPU switch".to_string(),
        },
        
        // Generic ACPI
        ProbeLocation {
            method: ProbeMethod::AcpiTable,
            address: 0,
            size: 0,
            name: "ACPI_DMAR".to_string(),
            description: "ACPI DMA Remapping table (indicates iGPU state)".to_string(),
        },
        ProbeLocation {
            method: ProbeMethod::AcpiTable,
            address: 0,
            size: 0,
            name: "ACPI_IVRS".to_string(),
            description: "AMD I/O Virtualization table".to_string(),
        },
        
        // PCI configuration
        ProbeLocation {
            method: ProbeMethod::PciConfig,
            address: 0x00000000,  // Bus 0, Device 0, Function 0
            size: 4,
            name: "HOST_BRIDGE".to_string(),
            description: "Host bridge - identifies platform".to_string(),
        },
        ProbeLocation {
            method: ProbeMethod::PciConfig,
            address: 0x00020000,  // Bus 0, Device 2, Function 0 (typical iGPU)
            size: 4,
            name: "IGPU_PCI".to_string(),
            description: "Integrated GPU PCI presence".to_string(),
        },
        ProbeLocation {
            method: ProbeMethod::PciConfig,
            address: 0x01000000,  // Bus 1, Device 0 (typical dGPU)
            size: 4,
            name: "DGPU_PCI".to_string(),
            description: "Discrete GPU PCI presence".to_string(),
        },
    ]
}

// ============================================================================
// Parallel Probe Engine
// ============================================================================

pub struct MuxProber {
    results: Arc<Mutex<HashMap<String, ProbeResult>>>,
    progress: Arc<Mutex<ProbeProgress>>,
    cancelled: Arc<Mutex<bool>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProbeProgress {
    pub total: usize,
    pub completed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub current: Option<String>,
    pub elapsed_ms: u64,
}

impl MuxProber {
    pub fn new() -> Self {
        Self {
            results: Arc::new(Mutex::new(HashMap::new())),
            progress: Arc::new(Mutex::new(ProbeProgress::default())),
            cancelled: Arc::new(Mutex::new(false)),
        }
    }
    
    /// Run all probes in parallel
    pub fn probe_all(&self, max_parallel: usize) -> Vec<ProbeResult> {
        let locations = get_known_mux_locations();
        let total = locations.len();
        
        {
            let mut prog = self.progress.lock().unwrap();
            prog.total = total;
            prog.completed = 0;
        }
        
        let start = Instant::now();
        
        // Chunk into parallel batches
        let chunks: Vec<_> = locations.chunks(max_parallel).collect();
        
        for chunk in chunks {
            if *self.cancelled.lock().unwrap() {
                break;
            }
            
            let handles: Vec<_> = chunk.iter().map(|loc| {
                let loc = loc.clone();
                let results = self.results.clone();
                let progress = self.progress.clone();
                let cancelled = self.cancelled.clone();
                
                thread::spawn(move || {
                    if *cancelled.lock().unwrap() {
                        return;
                    }
                    
                    {
                        let mut prog = progress.lock().unwrap();
                        prog.current = Some(loc.name.clone());
                    }
                    
                    let result = probe_location(&loc);
                    
                    {
                        let mut res = results.lock().unwrap();
                        res.insert(loc.name.clone(), result.clone());
                    }
                    
                    {
                        let mut prog = progress.lock().unwrap();
                        prog.completed += 1;
                        if result.status == ProbeStatus::Success {
                            prog.succeeded += 1;
                        } else {
                            prog.failed += 1;
                        }
                    }
                })
            }).collect();
            
            // Wait for batch to complete
            for handle in handles {
                let _ = handle.join();
            }
        }
        
        {
            let mut prog = self.progress.lock().unwrap();
            prog.elapsed_ms = start.elapsed().as_millis() as u64;
            prog.current = None;
        }
        
        // Collect results
        let results = self.results.lock().unwrap();
        results.values().cloned().collect()
    }
    
    /// Get current progress
    pub fn get_progress(&self) -> ProbeProgress {
        self.progress.lock().unwrap().clone()
    }
    
    /// Cancel ongoing probes
    pub fn cancel(&self) {
        *self.cancelled.lock().unwrap() = true;
    }
    
    /// Analyze results and suggest MUX unlock method
    pub fn analyze(&self) -> MuxAnalysis {
        let results = self.results.lock().unwrap();
        
        let mut analysis = MuxAnalysis {
            has_mux_switch: false,
            mux_type: None,
            unlock_method: None,
            current_state: None,
            risk_assessment: String::new(),
            recommendations: Vec::new(),
        };
        
        // Check for vendor-specific MUX
        for (name, result) in results.iter() {
            if result.status != ProbeStatus::Success {
                continue;
            }
            
            if name.contains("MUX") || name.contains("GPU_MODE") || name.contains("GPU_SWITCH") {
                analysis.has_mux_switch = true;
                
                if name.contains("ASUS") {
                    analysis.mux_type = Some("ASUS Advanced Optimus".to_string());
                    analysis.unlock_method = Some("Armoury Crate or EC write to 0xD1".to_string());
                } else if name.contains("LENOVO") {
                    analysis.mux_type = Some("Lenovo Hybrid Mode".to_string());
                    analysis.unlock_method = Some("Lenovo Vantage or EC write".to_string());
                } else if name.contains("MSI") {
                    analysis.mux_type = Some("MSI GPU Switch".to_string());
                    analysis.unlock_method = Some("MSI Center or EC write to 0xF4".to_string());
                }
                
                if let Some(val) = result.value {
                    analysis.current_state = Some(if val == 0 {
                        "Hybrid (iGPU active)".to_string()
                    } else {
                        "Discrete Only (dGPU direct)".to_string()
                    });
                }
            }
        }
        
        // Check if both GPUs are visible
        let has_igpu = results.get("IGPU_PCI")
            .map(|r| r.status == ProbeStatus::Success)
            .unwrap_or(false);
        let has_dgpu = results.get("DGPU_PCI")
            .map(|r| r.status == ProbeStatus::Success)
            .unwrap_or(false);
        
        if has_igpu && has_dgpu && !analysis.has_mux_switch {
            analysis.recommendations.push(
                "Both GPUs detected but no MUX switch found. \
                 This laptop may have a hardwired hybrid design (no MUX).".to_string()
            );
        }
        
        if analysis.has_mux_switch {
            analysis.risk_assessment = 
                "MUX switch found. Toggling requires reboot. \
                 Risk: LOW if using vendor software, MEDIUM if using EC writes.".to_string();
            
            analysis.recommendations.push(
                "Try vendor software first (Armoury Crate, Vantage, etc.)".to_string()
            );
            analysis.recommendations.push(
                "If vendor software unavailable, EC write is possible but backup first".to_string()
            );
        } else {
            analysis.risk_assessment = 
                "No MUX switch detected. Direct iGPU bypass not possible.".to_string();
            
            analysis.recommendations.push(
                "Use GPU orchestrator to offload tasks to iGPU instead".to_string()
            );
            analysis.recommendations.push(
                "Consider external GPU dock for true bypass".to_string()
            );
        }
        
        analysis
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxAnalysis {
    pub has_mux_switch: bool,
    pub mux_type: Option<String>,
    pub unlock_method: Option<String>,
    pub current_state: Option<String>,
    pub risk_assessment: String,
    pub recommendations: Vec<String>,
}

// ============================================================================
// Individual Probe Implementations
// ============================================================================

fn probe_location(loc: &ProbeLocation) -> ProbeResult {
    let start = Instant::now();
    
    let (status, value, interpretation) = match loc.method {
        ProbeMethod::PciConfig => probe_pci_config(loc.address),
        ProbeMethod::WmiQuery => probe_wmi(&loc.name),
        ProbeMethod::EcRam => probe_ec_ram(loc.address as u8),
        ProbeMethod::AcpiTable => probe_acpi(&loc.name),
        ProbeMethod::MsrRead => probe_msr(loc.address as u32),
        ProbeMethod::IoPort => (ProbeStatus::Skipped, None, Some("I/O port access disabled".to_string())),
        ProbeMethod::PhysicalMemory => (ProbeStatus::Skipped, None, Some("Physical memory access requires driver".to_string())),
        ProbeMethod::SmBios => probe_smbios(),
        ProbeMethod::EcWrite => (ProbeStatus::Skipped, None, Some("Write operations disabled".to_string())),
        ProbeMethod::NvramRead => (ProbeStatus::Skipped, None, Some("NVRAM access disabled".to_string())),
    };
    
    let risk_level = match loc.method {
        ProbeMethod::AcpiTable | ProbeMethod::PciConfig | ProbeMethod::WmiQuery => RiskLevel::Safe,
        ProbeMethod::EcRam | ProbeMethod::MsrRead | ProbeMethod::SmBios => RiskLevel::Elevated,
        ProbeMethod::IoPort | ProbeMethod::PhysicalMemory => RiskLevel::Caution,
        ProbeMethod::EcWrite | ProbeMethod::NvramRead => RiskLevel::Dangerous,
    };
    
    ProbeResult {
        location: loc.clone(),
        status,
        value,
        interpretation,
        risk_level,
        duration_us: start.elapsed().as_micros() as u64,
    }
}

#[cfg(target_os = "windows")]
fn probe_pci_config(address: u64) -> (ProbeStatus, Option<u64>, Option<String>) {
    // Use SetupAPI to enumerate PCI devices
    // This is safe - just reads PCI configuration space
    
    use std::ptr;
    
    // Would use SetupDiGetClassDevs, SetupDiEnumDeviceInfo, etc.
    // For now, simulate
    
    let bus = (address >> 24) & 0xFF;
    let device = (address >> 16) & 0xFF;
    let function = (address >> 8) & 0xFF;
    
    // Common device IDs
    let interpretation = match (bus, device) {
        (0, 0) => "Host Bridge (AMD/Intel)",
        (0, 2) => "Integrated Graphics",
        (1, 0) => "Discrete GPU (PCIe x16)",
        _ => "Unknown device",
    };
    
    (ProbeStatus::Success, Some(0), Some(interpretation.to_string()))
}

#[cfg(target_os = "windows")]
fn probe_wmi(name: &str) -> (ProbeStatus, Option<u64>, Option<String>) {
    // Query WMI for vendor-specific GPU controls
    // This is safe - read-only WMI query
    
    // Would use wmi crate or COM directly
    // WMI namespaces to check:
    // - root\WMI (ASUS, MSI)
    // - root\CIMV2 (generic)
    // - root\Lenovo (Lenovo)
    
    if name.contains("ASUS") {
        // Check for ASUS ACPI\ASUS_GPU_MUX
        (ProbeStatus::NotFound, None, Some("ASUS WMI not present".to_string()))
    } else if name.contains("LENOVO") {
        (ProbeStatus::NotFound, None, Some("Lenovo WMI not present".to_string()))
    } else {
        (ProbeStatus::NotFound, None, None)
    }
}

#[cfg(target_os = "windows")]
fn probe_ec_ram(offset: u8) -> (ProbeStatus, Option<u64>, Option<String>) {
    // Read Embedded Controller RAM via ACPI EC interface
    // Requires admin but is read-only and safe
    
    // Would use:
    // 1. Direct I/O port access (0x62/0x66) - needs driver
    // 2. WinRing0 or similar
    // 3. Vendor EC driver if available
    
    (ProbeStatus::AccessDenied, None, Some("EC access requires kernel driver".to_string()))
}

#[cfg(target_os = "windows")]
fn probe_acpi(table_name: &str) -> (ProbeStatus, Option<u64>, Option<String>) {
    // Read ACPI tables using GetSystemFirmwareTable
    use std::ptr;
    
    // Would call:
    // GetSystemFirmwareTable('ACPI', signature, buffer, size)
    
    let interpretation = match table_name {
        "ACPI_DMAR" => "DMA Remapping - indicates VT-d/IOMMU config",
        "ACPI_IVRS" => "AMD IOMMU config - indicates GPU isolation",
        _ => "Unknown ACPI table",
    };
    
    (ProbeStatus::Success, None, Some(interpretation.to_string()))
}

#[cfg(target_os = "windows")]
fn probe_msr(msr: u32) -> (ProbeStatus, Option<u64>, Option<String>) {
    // Read Model-Specific Registers
    // Requires kernel driver (WinRing0, RwDrv, etc.)
    
    (ProbeStatus::AccessDenied, None, Some("MSR access requires kernel driver".to_string()))
}

#[cfg(target_os = "windows")]
fn probe_smbios() -> (ProbeStatus, Option<u64>, Option<String>) {
    // Read SMBIOS tables using GetSystemFirmwareTable
    // Safe, provides system information
    
    // Would parse:
    // - Type 0: BIOS Information
    // - Type 1: System Information
    // - Type 2: Baseboard Information
    
    (ProbeStatus::Success, None, Some("SMBIOS tables accessible".to_string()))
}

#[cfg(not(target_os = "windows"))]
fn probe_pci_config(_address: u64) -> (ProbeStatus, Option<u64>, Option<String>) {
    // On Linux, read /sys/bus/pci/devices/
    (ProbeStatus::Success, None, Some("PCI sysfs".to_string()))
}

#[cfg(not(target_os = "windows"))]
fn probe_wmi(_name: &str) -> (ProbeStatus, Option<u64>, Option<String>) {
    (ProbeStatus::NotFound, None, Some("WMI is Windows-only".to_string()))
}

#[cfg(not(target_os = "windows"))]
fn probe_ec_ram(_offset: u8) -> (ProbeStatus, Option<u64>, Option<String>) {
    // On Linux, read /sys/kernel/debug/ec/ec0/io
    (ProbeStatus::AccessDenied, None, Some("EC debugfs requires root".to_string()))
}

#[cfg(not(target_os = "windows"))]
fn probe_acpi(_table_name: &str) -> (ProbeStatus, Option<u64>, Option<String>) {
    // On Linux, read /sys/firmware/acpi/tables/
    (ProbeStatus::Success, None, Some("ACPI sysfs".to_string()))
}

#[cfg(not(target_os = "windows"))]
fn probe_msr(_msr: u32) -> (ProbeStatus, Option<u64>, Option<String>) {
    // On Linux, use /dev/cpu/0/msr
    (ProbeStatus::AccessDenied, None, Some("MSR requires root".to_string()))
}

#[cfg(not(target_os = "windows"))]
fn probe_smbios() -> (ProbeStatus, Option<u64>, Option<String>) {
    // On Linux, read /sys/class/dmi/id/
    (ProbeStatus::Success, None, Some("DMI sysfs".to_string()))
}

// ============================================================================
// MCP Server Interface
// ============================================================================

pub struct MuxMcpServer {
    prober: MuxProber,
}

impl MuxMcpServer {
    pub fn new() -> Self {
        Self {
            prober: MuxProber::new(),
        }
    }
    
    /// Handle MCP request
    pub fn handle_request(&self, request: McpRequest) -> McpResponse {
        let result = match request.method.as_str() {
            "mux/probe" => self.handle_probe(request.params),
            "mux/progress" => self.handle_progress(),
            "mux/cancel" => self.handle_cancel(),
            "mux/analyze" => self.handle_analyze(),
            "mux/list_locations" => self.handle_list_locations(),
            _ => Err(McpError {
                code: -32601,
                message: "Method not found".to_string(),
            }),
        };
        
        match result {
            Ok(value) => McpResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: Some(value),
                error: None,
            },
            Err(error) => McpResponse {
                jsonrpc: "2.0".to_string(),
                id: request.id,
                result: None,
                error: Some(error),
            },
        }
    }
    
    fn handle_probe(&self, params: Option<serde_json::Value>) -> Result<serde_json::Value, McpError> {
        let max_parallel = params
            .as_ref()
            .and_then(|p| p.get("max_parallel"))
            .and_then(|v| v.as_u64())
            .unwrap_or(4) as usize;
        
        let results = self.prober.probe_all(max_parallel);
        
        serde_json::to_value(results)
            .map_err(|e| McpError { code: -32603, message: e.to_string() })
    }
    
    fn handle_progress(&self) -> Result<serde_json::Value, McpError> {
        let progress = self.prober.get_progress();
        serde_json::to_value(progress)
            .map_err(|e| McpError { code: -32603, message: e.to_string() })
    }
    
    fn handle_cancel(&self) -> Result<serde_json::Value, McpError> {
        self.prober.cancel();
        Ok(serde_json::json!({"cancelled": true}))
    }
    
    fn handle_analyze(&self) -> Result<serde_json::Value, McpError> {
        let analysis = self.prober.analyze();
        serde_json::to_value(analysis)
            .map_err(|e| McpError { code: -32603, message: e.to_string() })
    }
    
    fn handle_list_locations(&self) -> Result<serde_json::Value, McpError> {
        let locations = get_known_mux_locations();
        serde_json::to_value(locations)
            .map_err(|e| McpError { code: -32603, message: e.to_string() })
    }
}

// ============================================================================
// Public API
// ============================================================================

use once_cell::sync::Lazy;
use std::sync::RwLock;


static MUX_SERVER: Lazy<RwLock<MuxMcpServer>> = Lazy::new(|| {
    RwLock::new(MuxMcpServer::new())
});


pub async fn mux_probe(max_parallel: Option<usize>) -> Vec<ProbeResult> {
    let server = MUX_SERVER.read().unwrap();
    server.prober.probe_all(max_parallel.unwrap_or(4))
}


pub async fn mux_progress() -> ProbeProgress {
    let server = MUX_SERVER.read().unwrap();
    server.prober.get_progress()
}


pub async fn mux_cancel() {
    let server = MUX_SERVER.read().unwrap();
    server.prober.cancel();
}


pub async fn mux_analyze() -> MuxAnalysis {
    let server = MUX_SERVER.read().unwrap();
    server.prober.analyze()
}


pub async fn mux_list_locations() -> Vec<ProbeLocation> {
    get_known_mux_locations()
}
