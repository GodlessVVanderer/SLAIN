// Driver Hot Path Analysis
// 
// NVIDIA driver is ~800MB installed. What do we ACTUALLY use?
// 
// BLOAT (not needed for video playback):
// ├── GeForce Experience (~200MB) - telemetry, game optimization
// ├── NSight (~150MB) - debugging tools
// ├── PhysX (~50MB) - game physics (not video)
// ├── 3D Vision (~30MB) - dead technology
// ├── HD Audio (~20MB) - HDMI audio (use system audio)
// ├── Ansel (~15MB) - screenshot tool
// ├── ShadowPlay (~40MB) - recording (we use AMD VCN instead!)
// └── Telemetry services (~10MB)
//
// ACTUALLY NEEDED (~50MB):
// ├── nvlddmkm.sys - kernel mode driver (display)
// ├── nvd3dumx.dll - Direct3D user mode
// ├── nvcuda.dll - CUDA runtime
// ├── nvEncodeAPI.dll - NVENC (but we offload to AMD!)
// ├── nvDecodeAPI.dll - NVDEC (video decode)
// └── nvapi64.dll - low-level API
//
// Strategy: Don't rewrite the driver. Bypass the bloat layers.

use std::collections::HashMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

// ============================================================================
// Driver Component Analysis
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverComponent {
    pub name: String,
    pub path: PathBuf,
    pub size_mb: f32,
    pub category: ComponentCategory,
    pub required_for: Vec<UseCase>,
    pub can_disable: bool,
    pub replacement: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComponentCategory {
    Kernel,         // Must have - kernel mode driver
    Display,        // Must have - basic display output
    Cuda,           // Need for GPU compute
    VideoCodec,     // NVENC/NVDEC
    Telemetry,      // BLOAT - phone home
    GameFeatures,   // BLOAT - GeForce Experience stuff
    Debug,          // BLOAT - NSight, debugging
    Legacy,         // BLOAT - old tech (3D Vision, etc)
    Audio,          // Optional - HDMI audio
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UseCase {
    VideoPlayback,
    VideoUpscaling,
    Gaming,
    MachineLearning,
    Streaming,
    ScreenRecording,
}

/// Analyze NVIDIA driver installation
pub fn analyze_nvidia_driver() -> DriverAnalysis {
    let components = vec![
        // KERNEL - Required
        DriverComponent {
            name: "nvlddmkm.sys".to_string(),
            path: PathBuf::from("C:\\Windows\\System32\\drivers\\nvlddmkm.sys"),
            size_mb: 25.0,
            category: ComponentCategory::Kernel,
            required_for: vec![UseCase::VideoPlayback, UseCase::Gaming, UseCase::MachineLearning],
            can_disable: false,
            replacement: None,
        },
        
        // DISPLAY - Required
        DriverComponent {
            name: "nvd3dumx.dll".to_string(),
            path: PathBuf::from("C:\\Windows\\System32\\nvd3dumx.dll"),
            size_mb: 15.0,
            category: ComponentCategory::Display,
            required_for: vec![UseCase::VideoPlayback, UseCase::Gaming],
            can_disable: false,
            replacement: None,
        },
        
        // CUDA - Required for AI
        DriverComponent {
            name: "nvcuda.dll".to_string(),
            path: PathBuf::from("C:\\Windows\\System32\\nvcuda.dll"),
            size_mb: 8.0,
            category: ComponentCategory::Cuda,
            required_for: vec![UseCase::MachineLearning, UseCase::VideoUpscaling],
            can_disable: false,
            replacement: None,
        },
        
        // NVDEC - Required for video decode
        DriverComponent {
            name: "nvDecodeAPI64.dll".to_string(),
            path: PathBuf::from("C:\\Windows\\System32\\nvDecodeAPI64.dll"),
            size_mb: 2.0,
            category: ComponentCategory::VideoCodec,
            required_for: vec![UseCase::VideoPlayback],
            can_disable: false,
            replacement: None,
        },
        
        // NVENC - Can offload to AMD!
        DriverComponent {
            name: "nvEncodeAPI64.dll".to_string(),
            path: PathBuf::from("C:\\Windows\\System32\\nvEncodeAPI64.dll"),
            size_mb: 3.0,
            category: ComponentCategory::VideoCodec,
            required_for: vec![UseCase::Streaming, UseCase::ScreenRecording],
            can_disable: true,
            replacement: Some("AMD VCN via amf_encoder.rs".to_string()),
        },
        
        // BLOAT: GeForce Experience
        DriverComponent {
            name: "GeForce Experience".to_string(),
            path: PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\NVIDIA GeForce Experience"),
            size_mb: 200.0,
            category: ComponentCategory::GameFeatures,
            required_for: vec![],
            can_disable: true,
            replacement: Some("Not needed - use SLAIN instead".to_string()),
        },
        
        // BLOAT: ShadowPlay
        DriverComponent {
            name: "ShadowPlay".to_string(),
            path: PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\ShadowPlay"),
            size_mb: 40.0,
            category: ComponentCategory::GameFeatures,
            required_for: vec![UseCase::ScreenRecording],
            can_disable: true,
            replacement: Some("AMD VCN recording - frees NVIDIA for rendering".to_string()),
        },
        
        // BLOAT: NSight
        DriverComponent {
            name: "NSight".to_string(),
            path: PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\Nsight"),
            size_mb: 150.0,
            category: ComponentCategory::Debug,
            required_for: vec![],
            can_disable: true,
            replacement: Some("Not needed for end users".to_string()),
        },
        
        // BLOAT: Telemetry
        DriverComponent {
            name: "NvTelemetry".to_string(),
            path: PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\NvTelemetry"),
            size_mb: 10.0,
            category: ComponentCategory::Telemetry,
            required_for: vec![],
            can_disable: true,
            replacement: Some("DISABLE THIS - spyware".to_string()),
        },
        
        // BLOAT: 3D Vision
        DriverComponent {
            name: "3D Vision".to_string(),
            path: PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\3D Vision"),
            size_mb: 30.0,
            category: ComponentCategory::Legacy,
            required_for: vec![],
            can_disable: true,
            replacement: Some("Dead technology - remove".to_string()),
        },
        
        // BLOAT: PhysX (unless gaming)
        DriverComponent {
            name: "PhysX".to_string(),
            path: PathBuf::from("C:\\Program Files\\NVIDIA Corporation\\PhysX"),
            size_mb: 50.0,
            category: ComponentCategory::GameFeatures,
            required_for: vec![UseCase::Gaming],
            can_disable: true,
            replacement: Some("Only needed for specific games".to_string()),
        },
    ];
    
    let total_size: f32 = components.iter().map(|c| c.size_mb).sum();
    let bloat_size: f32 = components.iter()
        .filter(|c| c.can_disable)
        .map(|c| c.size_mb)
        .sum();
    let required_size = total_size - bloat_size;
    
    DriverAnalysis {
        components,
        total_size_mb: total_size,
        bloat_size_mb: bloat_size,
        required_size_mb: required_size,
        bloat_percentage: (bloat_size / total_size) * 100.0,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverAnalysis {
    pub components: Vec<DriverComponent>,
    pub total_size_mb: f32,
    pub bloat_size_mb: f32,
    pub required_size_mb: f32,
    pub bloat_percentage: f32,
}

// ============================================================================
// Minimal Driver Interface
// ============================================================================

/// What we ACTUALLY call in the driver
pub struct MinimalNvidiaInterface {
    // Display
    pub set_display_mode: bool,      // D3DKMTSetDisplayMode
    pub present: bool,               // D3DKMTPresent
    
    // CUDA
    pub cuda_malloc: bool,           // cuMemAlloc
    pub cuda_memcpy: bool,           // cuMemcpy
    pub cuda_launch_kernel: bool,    // cuLaunchKernel
    pub cuda_synchronize: bool,      // cuStreamSynchronize
    
    // Video decode
    pub create_decoder: bool,        // NvDecCreateDecoder
    pub decode_picture: bool,        // NvDecDecodePicture
    pub map_video_frame: bool,       // NvDecMapVideoFrame
    
    // That's it. Everything else is bloat.
}

impl MinimalNvidiaInterface {
    /// List the ~20 functions we actually need from ~2000 exported
    pub fn required_functions() -> Vec<&'static str> {
        vec![
            // CUDA Runtime (10 functions)
            "cuInit",
            "cuDeviceGet",
            "cuCtxCreate",
            "cuMemAlloc",
            "cuMemFree",
            "cuMemcpyDtoH",
            "cuMemcpyHtoD",
            "cuModuleLoad",
            "cuLaunchKernel",
            "cuStreamSynchronize",
            
            // NVDEC (6 functions)
            "NvDecCreateDecoder",
            "NvDecDestroyDecoder",
            "NvDecDecodePicture",
            "NvDecMapVideoFrame",
            "NvDecUnmapVideoFrame",
            "NvDecGetDecoderCaps",
            
            // Display (4 functions)
            "D3DKMTOpenAdapterFromLuid",
            "D3DKMTCreateDevice",
            "D3DKMTPresent",
            "D3DKMTSetDisplayMode",
        ]
    }
    
    /// Calculate what percentage of driver we use
    pub fn usage_analysis() -> DriverUsage {
        DriverUsage {
            total_exports: 2847,        // Approximate exports in nvcuda.dll + nvd3dum
            functions_we_use: 20,
            percentage_used: 0.7,       // Less than 1%!
            
            // What the bloat does
            unused_categories: vec![
                "Game telemetry".to_string(),
                "Shader cache management".to_string(),
                "Profile management".to_string(),
                "Ansel screenshot hooks".to_string(),
                "ShadowPlay hooks".to_string(),
                "3D Vision stereo".to_string(),
                "SLI/NVLink management".to_string(),
                "G-SYNC control".to_string(),
                "Ray tracing setup".to_string(),
                "DLSS model loading".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverUsage {
    pub total_exports: u32,
    pub functions_we_use: u32,
    pub percentage_used: f32,
    pub unused_categories: Vec<String>,
}

// ============================================================================
// Direct Hardware Access (bypass driver bloat)
// ============================================================================

/// Minimal Rust wrapper that calls ONLY what we need
pub mod direct_gpu {
    use super::*;
    
    /// Load only the DLLs we need
    pub struct MinimalGpuContext {
        cuda_lib: Option<libloading::Library>,
        nvdec_lib: Option<libloading::Library>,
        // Skip: GeForce Experience, NSight, Telemetry, etc.
    }
    
    impl MinimalGpuContext {
        pub fn new() -> Result<Self, String> {
            // Load ONLY nvcuda.dll - nothing else
            let cuda_lib = unsafe {
                libloading::Library::new("nvcuda.dll")
                    .map_err(|e| format!("CUDA not available: {}", e))?
            };
            
            // Load ONLY nvDecodeAPI64.dll
            let nvdec_lib = unsafe {
                libloading::Library::new("nvDecodeAPI64.dll").ok()
            };
            
            Ok(Self {
                cuda_lib: Some(cuda_lib),
                nvdec_lib,
            })
        }
        
        /// We skip:
        /// - nvEncodeAPI64.dll (use AMD VCN instead)
        /// - nvcuvid.dll (legacy, use nvdec directly)
        /// - nvapi64.dll (telemetry, profiles, bloat)
        /// - Everything in GeForce Experience
        pub fn what_we_skip() -> Vec<&'static str> {
            vec![
                "nvapi64.dll - telemetry and profiles",
                "NvFBC64.dll - frame buffer capture (use AMD)",
                "nvGamepad.dll - controller bloat",
                "NvCamera64.dll - Ansel",
                "nvSCPAPI64.dll - ShadowPlay",
                "nvEncodeAPI64.dll - encoding (AMD does this)",
                "GFSDK_*.dll - GameWorks bloat",
            ]
        }
    }
}

// ============================================================================
// Public Rust API
// ============================================================================




pub async fn driver_analyze() -> DriverAnalysis {
    analyze_nvidia_driver()
}


pub async fn driver_usage() -> DriverUsage {
    MinimalNvidiaInterface::usage_analysis()
}


pub async fn driver_required_functions() -> Vec<&'static str> {
    MinimalNvidiaInterface::required_functions()
}


pub async fn driver_skip_list() -> Vec<&'static str> {
    direct_gpu::MinimalGpuContext::what_we_skip()
}
