//! # Quad Pipeline - In-Memory Video Processing
//!
//! Four parallel processing pipelines, all operating in memory without temp files:
//!
//! 1. **AviSynth** - FFI to C API, script strings passed directly
//! 2. **VapourSynth** - FFI to C API, Python scripts in memory
//! 3. **Vulkan** - wgpu compute shaders, native Rust
//! 4. **CUDA** - FFI to CUDA runtime, PTX kernels
//!
//! ## Key Design Principles
//! - NO intermediate files (.avs, .vpy, temp videos)
//! - All frame data stays in GPU/CPU memory
//! - Named pipes for legacy compatibility if needed
//! - Direct FFI calls for maximum performance

use libloading::Library;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PipelineError {
    #[error("Pipeline not available: {0}")]
    NotAvailable(String),
    #[error("Script error: {0}")]
    ScriptError(String),
    #[error("Frame processing failed: {0}")]
    ProcessingFailed(String),
    #[error("Library load failed: {0}")]
    LibraryError(String),
}

/// Pipeline types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum PipelineKind {
    /// Auto-detect best pipeline
    Auto,
    /// Hardware decode (GPU)
    Hardware,
    /// Software decode only (CPU)
    SoftwareOnly,
    /// Direct passthrough (no processing)
    Direct,
    /// AviSynth via C API FFI
    AviSynth,
    /// VapourSynth via C API FFI
    VapourSynth,
    /// Vulkan compute via wgpu
    Vulkan,
    /// CUDA compute via FFI
    Cuda,
    /// External sidecar process for heavy processing
    Sidecar,
}

impl Default for PipelineKind {
    fn default() -> Self {
        PipelineKind::SoftwareOnly
    }
}

/// A video frame in pipeline-native format
pub struct PipelineFrame {
    /// Raw frame data (format depends on pipeline)
    pub data: Vec<u8>,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Stride (bytes per row, may include padding)
    pub stride: u32,
    /// Pixel format identifier
    pub format: FrameFormat,
    /// Frame number
    pub frame_num: u64,
    /// Presentation timestamp (microseconds)
    pub pts_us: i64,
}

#[derive(Debug, Clone, Copy)]
pub enum FrameFormat {
    /// Planar YUV 4:2:0
    I420,
    /// Semi-planar YUV 4:2:0 (NVIDIA preferred)
    Nv12,
    /// Packed BGRA (AviSynth preferred)
    Bgra,
    /// Packed RGBA
    Rgba,
    /// Planar RGB (VapourSynth preferred)
    PlanarRgb,
}

/// Pipeline trait - all pipelines implement this
pub trait Pipeline: Send + Sync {
    /// Get pipeline type
    fn kind(&self) -> PipelineKind;
    
    /// Check if pipeline is available on this system
    fn is_available(&self) -> bool;
    
    /// Initialize with a filter script/configuration
    fn init(&mut self, config: &PipelineConfig) -> Result<(), PipelineError>;
    
    /// Process a frame through the pipeline
    fn process(&mut self, frame: PipelineFrame) -> Result<PipelineFrame, PipelineError>;
    
    /// Flush any buffered frames
    fn flush(&mut self) -> Result<Vec<PipelineFrame>, PipelineError>;
    
    /// Reset pipeline state
    fn reset(&mut self);
    
    /// Get pipeline name for display
    fn name(&self) -> &str;
}

/// Pipeline configuration
#[derive(Debug, Clone)]
pub struct PipelineConfig {
    /// Filter script (AVS script string, VS Python code, GLSL shader, CUDA kernel)
    pub script: String,
    /// Input width
    pub width: u32,
    /// Input height
    pub height: u32,
    /// Input frame rate (num/den)
    pub fps: (u32, u32),
    /// GPU device index (for Vulkan/CUDA)
    pub gpu_index: Option<u32>,
    /// Optional sidecar command override
    pub sidecar_command: Option<String>,
    /// Sidecar command arguments
    pub sidecar_args: Vec<String>,
    /// Sidecar environment variables
    pub sidecar_env: Vec<(String, String)>,
}

// ============================================================================
// AviSynth Pipeline (FFI to C API)
// ============================================================================

/// AviSynth pipeline - processes frames via in-memory script
/// 
/// Key: Uses avs_invoke("Eval", script_string) to evaluate scripts WITHOUT files
#[cfg(feature = "avisynth")]
pub struct AviSynthPipeline {
    lib: Option<Library>,
    env: usize,  // AVS_ScriptEnvironment* as opaque
    clip: usize, // AVS_Clip* as opaque
    initialized: bool,
}

#[cfg(feature = "avisynth")]
impl AviSynthPipeline {
    pub fn new() -> Self {
        Self {
            lib: None,
            env: 0,
            clip: 0,
            initialized: false,
        }
    }
}

#[cfg(feature = "avisynth")]
impl Pipeline for AviSynthPipeline {
    fn kind(&self) -> PipelineKind { PipelineKind::AviSynth }
    
    fn is_available(&self) -> bool {
        unsafe { Library::new("AviSynth.dll").is_ok() }
    }
    
    fn init(&mut self, config: &PipelineConfig) -> Result<(), PipelineError> {
        let lib = unsafe { Library::new("AviSynth.dll") }
            .map_err(|_| PipelineError::LibraryError(
                "AviSynth not found. Install AviSynth+ from https://avs-plus.net/".into()
            ))?;
        
        // The script is passed as a STRING to avs_invoke("Eval", script)
        // No .avs file created!
        let _full_script = format!(r#"
# SLAIN frame source injection point
global slain_width = {}
global slain_height = {}
# User filter chain:
{}
"#, config.width, config.height, config.script);
        
        // TODO: FFI calls to avs_create_script_environment, avs_invoke
        
        self.lib = Some(lib);
        self.initialized = true;
        Ok(())
    }
    
    fn process(&mut self, frame: PipelineFrame) -> Result<PipelineFrame, PipelineError> {
        if !self.initialized {
            return Err(PipelineError::NotAvailable("Not initialized".into()));
        }
        // TODO: avs_get_frame FFI
        Ok(frame)
    }
    
    fn flush(&mut self) -> Result<Vec<PipelineFrame>, PipelineError> { Ok(vec![]) }
    fn reset(&mut self) { self.initialized = false; }
    fn name(&self) -> &str { "AviSynth" }
}

// ============================================================================
// VapourSynth Pipeline (FFI to C API)
// ============================================================================

/// VapourSynth pipeline - Python scripts evaluated in memory
/// 
/// Key: Uses vsscript_evaluateBuffer() to run Python code WITHOUT files
#[cfg(feature = "vapoursynth")]
pub struct VapourSynthPipeline {
    lib: Option<Library>,
    initialized: bool,
}

#[cfg(feature = "vapoursynth")]
impl VapourSynthPipeline {
    pub fn new() -> Self {
        Self { lib: None, initialized: false }
    }
}

#[cfg(feature = "vapoursynth")]
impl Pipeline for VapourSynthPipeline {
    fn kind(&self) -> PipelineKind { PipelineKind::VapourSynth }
    
    fn is_available(&self) -> bool {
        #[cfg(windows)]
        { unsafe { Library::new("vapoursynth.dll").is_ok() } }
        #[cfg(not(windows))]
        { unsafe { Library::new("libvapoursynth.so").is_ok() } }
    }
    
    fn init(&mut self, config: &PipelineConfig) -> Result<(), PipelineError> {
        #[cfg(windows)]
        let lib_name = "vsscript.dll";
        #[cfg(not(windows))]
        let lib_name = "libvsscript.so";
        
        let lib = unsafe { Library::new(lib_name) }
            .map_err(|_| PipelineError::LibraryError(
                "VapourSynth not found. Install from https://vapoursynth.com/".into()
            ))?;
        
        // Script passed as buffer to vsscript_evaluateBuffer()
        let _script = format!(r#"
import vapoursynth as vs
core = vs.core
# User filters:
{}
"#, config.script);
        
        self.lib = Some(lib);
        self.initialized = true;
        Ok(())
    }
    
    fn process(&mut self, frame: PipelineFrame) -> Result<PipelineFrame, PipelineError> {
        Ok(frame)
    }
    
    fn flush(&mut self) -> Result<Vec<PipelineFrame>, PipelineError> { Ok(vec![]) }
    fn reset(&mut self) { self.initialized = false; }
    fn name(&self) -> &str { "VapourSynth" }
}

// ============================================================================
// Vulkan Compute Pipeline (wgpu - Pure Rust, No FFI)
// ============================================================================

/// Vulkan pipeline - Pure Rust via wgpu
/// 
/// WGSL shaders compiled at runtime, no external dependencies
#[cfg(feature = "vulkan-compute")]
pub struct VulkanPipeline {
    initialized: bool,
    // wgpu handles stored here
}

#[cfg(feature = "vulkan-compute")]
impl VulkanPipeline {
    pub fn new() -> Self {
        Self { initialized: false }
    }
    
    /// Default WGSL shader for color processing
    pub const DEFAULT_SHADER: &'static str = r#"
@group(0) @binding(0) var<storage, read> input: array<u32>;
@group(0) @binding(1) var<storage, read_write> output: array<u32>;

struct Params {
    width: u32,
    height: u32,
    brightness: f32,
    contrast: f32,
}
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.width || gid.y >= params.height) { return; }
    let idx = gid.y * params.width + gid.x;
    output[idx] = input[idx]; // TODO: Apply filters
}
"#;
}

#[cfg(feature = "vulkan-compute")]
impl Pipeline for VulkanPipeline {
    fn kind(&self) -> PipelineKind { PipelineKind::Vulkan }
    fn is_available(&self) -> bool { true } // wgpu always available
    
    fn init(&mut self, _config: &PipelineConfig) -> Result<(), PipelineError> {
        // TODO: wgpu device/queue/pipeline setup
        self.initialized = true;
        Ok(())
    }
    
    fn process(&mut self, frame: PipelineFrame) -> Result<PipelineFrame, PipelineError> {
        Ok(frame)
    }
    
    fn flush(&mut self) -> Result<Vec<PipelineFrame>, PipelineError> { Ok(vec![]) }
    fn reset(&mut self) { self.initialized = false; }
    fn name(&self) -> &str { "Vulkan Compute" }
}

// ============================================================================
// CUDA Pipeline (FFI to CUDA Driver API)
// ============================================================================

/// CUDA pipeline - PTX kernels loaded from strings
/// 
/// Key: cuModuleLoadData() takes PTX as a string, no .cu or .ptx files!
#[cfg(feature = "cuda")]
pub struct CudaPipeline {
    lib: Option<Library>,
    initialized: bool,
}

#[cfg(feature = "cuda")]
impl CudaPipeline {
    pub fn new() -> Self {
        Self { lib: None, initialized: false }
    }
    
    /// Default CUDA kernel (as PTX would be compiled from this)
    pub const DEFAULT_KERNEL: &'static str = r#"
extern "C" __global__ void process_frame(
    const unsigned char* input,
    unsigned char* output,
    int width, int height,
    float brightness, float contrast
) {
    int x = blockIdx.x * blockDim.x + threadIdx.x;
    int y = blockIdx.y * blockDim.y + threadIdx.y;
    if (x >= width || y >= height) return;
    
    int idx = y * width + x;
    float p = (float)input[idx];
    p = (p - 128.0f) * contrast + 128.0f + brightness;
    output[idx] = (unsigned char)fminf(255.0f, fmaxf(0.0f, p));
}
"#;
}

#[cfg(feature = "cuda")]
impl Pipeline for CudaPipeline {
    fn kind(&self) -> PipelineKind { PipelineKind::Cuda }
    
    fn is_available(&self) -> bool {
        #[cfg(windows)]
        { unsafe { Library::new("nvcuda.dll").is_ok() } }
        #[cfg(not(windows))]
        { unsafe { Library::new("libcuda.so").is_ok() } }
    }
    
    fn init(&mut self, _config: &PipelineConfig) -> Result<(), PipelineError> {
        #[cfg(windows)]
        let lib_name = "nvcuda.dll";
        #[cfg(not(windows))]
        let lib_name = "libcuda.so";
        
        let lib = unsafe { Library::new(lib_name) }
            .map_err(|_| PipelineError::LibraryError("CUDA not found".into()))?;
        
        // TODO: cuInit, cuDeviceGet, cuCtxCreate, cuModuleLoadData (PTX string!)
        
        self.lib = Some(lib);
        self.initialized = true;
        Ok(())
    }
    
    fn process(&mut self, frame: PipelineFrame) -> Result<PipelineFrame, PipelineError> {
        Ok(frame)
    }
    
    fn flush(&mut self) -> Result<Vec<PipelineFrame>, PipelineError> { Ok(vec![]) }
    fn reset(&mut self) { self.initialized = false; }
    fn name(&self) -> &str { "CUDA" }
}

// ============================================================================
// Sidecar Pipeline (External Process)
// ============================================================================

const DEFAULT_SIDECAR: &str = "slain-sidecar";

pub struct SidecarPipeline {
    command: Option<String>,
    args: Vec<String>,
    env: Vec<(String, String)>,
    child: Option<Child>,
}

impl SidecarPipeline {
    pub fn new() -> Self {
        Self {
            command: None,
            args: Vec::new(),
            env: Vec::new(),
            child: None,
        }
    }

    fn resolve_command(&self, config: &PipelineConfig) -> Option<String> {
        if let Some(command) = config.sidecar_command.clone() {
            Some(command)
        } else if is_executable_on_path(DEFAULT_SIDECAR) {
            Some(DEFAULT_SIDECAR.to_string())
        } else {
            None
        }
    }

    fn spawn_sidecar(&mut self, command: &str) -> Result<(), PipelineError> {
        let mut cmd = Command::new(command);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in &self.env {
            cmd.env(key, value);
        }

        let child = cmd.spawn().map_err(|e| {
            PipelineError::LibraryError(format!("Failed to start sidecar {}: {}", command, e))
        })?;
        self.child = Some(child);
        Ok(())
    }

    fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
        }
    }
}

impl Pipeline for SidecarPipeline {
    fn kind(&self) -> PipelineKind { PipelineKind::Sidecar }

    fn is_available(&self) -> bool {
        if let Some(command) = &self.command {
            return is_executable_on_path(command);
        }
        is_executable_on_path(DEFAULT_SIDECAR)
    }

    fn init(&mut self, config: &PipelineConfig) -> Result<(), PipelineError> {
        self.command = self.resolve_command(config);
        self.args = config.sidecar_args.clone();
        self.env = config.sidecar_env.clone();

        let Some(command) = self.command.clone() else {
            return Err(PipelineError::NotAvailable(
                "Sidecar not found. Provide sidecar_command or install slain-sidecar.".into(),
            ));
        };

        if !is_executable_on_path(&command) {
            return Err(PipelineError::NotAvailable(format!(
                "Sidecar command not found: {}",
                command
            )));
        }

        self.spawn_sidecar(&command)?;
        Ok(())
    }

    fn process(&mut self, frame: PipelineFrame) -> Result<PipelineFrame, PipelineError> {
        if let Some(child) = &mut self.child {
            if let Ok(Some(status)) = child.try_wait() {
                return Err(PipelineError::ProcessingFailed(format!(
                    "Sidecar exited with status {}",
                    status
                )));
            }
        }

        // TODO: Encode frame to sidecar protocol and decode response.
        Ok(frame)
    }

    fn flush(&mut self) -> Result<Vec<PipelineFrame>, PipelineError> { Ok(vec![]) }

    fn reset(&mut self) {
        self.shutdown();
        self.command = None;
        self.args.clear();
        self.env.clear();
    }

    fn name(&self) -> &str { "Sidecar" }
}

fn is_executable_on_path(command: &str) -> bool {
    if command.is_empty() {
        return false;
    }

    let path = Path::new(command);
    if path.components().count() > 1 {
        return path.exists();
    }

    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    let extensions = executable_extensions();
    for dir in std::env::split_paths(&paths) {
        if extensions.is_empty() {
            let candidate = dir.join(command);
            if candidate.exists() {
                return true;
            }
        } else {
            for ext in &extensions {
                let candidate = dir.join(format!("{}{}", command, ext));
                if candidate.exists() {
                    return true;
                }
            }
        }
    }

    false
}

fn executable_extensions() -> Vec<String> {
    if cfg!(windows) {
        if let Some(pathext) = std::env::var_os("PATHEXT") {
            let raw = pathext.to_string_lossy();
            return raw
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(|ext| ext.to_string())
                .collect();
        }
        vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]
    } else {
        Vec::new()
    }
}

// ============================================================================
// Pipeline Manager - Routes frames to active pipeline
// ============================================================================

/// Manages all pipelines and frame routing
pub struct PipelineManager {
    active: PipelineKind,
    sidecar: SidecarPipeline,
    #[cfg(feature = "avisynth")]
    avisynth: AviSynthPipeline,
    #[cfg(feature = "vapoursynth")]
    vapoursynth: VapourSynthPipeline,
    #[cfg(feature = "vulkan-compute")]
    vulkan: VulkanPipeline,
    #[cfg(feature = "cuda")]
    cuda: CudaPipeline,
}

impl PipelineManager {
    pub fn new() -> Self {
        Self {
            active: PipelineKind::Direct,
            sidecar: SidecarPipeline::new(),
            #[cfg(feature = "avisynth")]
            avisynth: AviSynthPipeline::new(),
            #[cfg(feature = "vapoursynth")]
            vapoursynth: VapourSynthPipeline::new(),
            #[cfg(feature = "vulkan-compute")]
            vulkan: VulkanPipeline::new(),
            #[cfg(feature = "cuda")]
            cuda: CudaPipeline::new(),
        }
    }
    
    /// Get available pipelines on this system
    #[allow(unused_mut)]
    pub fn available(&self) -> Vec<PipelineKind> {
        let mut list = vec![PipelineKind::Direct];

        if self.sidecar.is_available() { list.push(PipelineKind::Sidecar); }

        #[cfg(feature = "avisynth")]
        if self.avisynth.is_available() { list.push(PipelineKind::AviSynth); }
        
        #[cfg(feature = "vapoursynth")]
        if self.vapoursynth.is_available() { list.push(PipelineKind::VapourSynth); }
        
        #[cfg(feature = "vulkan-compute")]
        if self.vulkan.is_available() { list.push(PipelineKind::Vulkan); }
        
        #[cfg(feature = "cuda")]
        if self.cuda.is_available() { list.push(PipelineKind::Cuda); }
        
        list
    }
    
    /// Set active pipeline
    pub fn set_active(&mut self, kind: PipelineKind) {
        self.active = kind;
    }
    
    /// Get active pipeline kind
    pub fn active(&self) -> PipelineKind {
        self.active
    }
    
    /// Initialize active pipeline
    pub fn init(&mut self, config: &PipelineConfig) -> Result<(), PipelineError> {
        match self.active {
            PipelineKind::Direct => Ok(()),
            PipelineKind::Sidecar => self.sidecar.init(config),
            #[cfg(feature = "avisynth")]
            PipelineKind::AviSynth => self.avisynth.init(config),
            #[cfg(feature = "vapoursynth")]
            PipelineKind::VapourSynth => self.vapoursynth.init(config),
            #[cfg(feature = "vulkan-compute")]
            PipelineKind::Vulkan => self.vulkan.init(config),
            #[cfg(feature = "cuda")]
            PipelineKind::Cuda => self.cuda.init(config),
            #[allow(unreachable_patterns)]
            _ => Err(PipelineError::NotAvailable(format!("{:?}", self.active))),
        }
    }
    
    /// Process frame through active pipeline
    pub fn process(&mut self, frame: PipelineFrame) -> Result<PipelineFrame, PipelineError> {
        match self.active {
            PipelineKind::Direct => Ok(frame),
            PipelineKind::Sidecar => self.sidecar.process(frame),
            #[cfg(feature = "avisynth")]
            PipelineKind::AviSynth => self.avisynth.process(frame),
            #[cfg(feature = "vapoursynth")]
            PipelineKind::VapourSynth => self.vapoursynth.process(frame),
            #[cfg(feature = "vulkan-compute")]
            PipelineKind::Vulkan => self.vulkan.process(frame),
            #[cfg(feature = "cuda")]
            PipelineKind::Cuda => self.cuda.process(frame),
            #[allow(unreachable_patterns)]
            _ => Ok(frame),
        }
    }
}

impl Default for PipelineManager {
    fn default() -> Self { Self::new() }
}
