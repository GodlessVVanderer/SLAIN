// AMF DECODE - AMD Advanced Media Framework Video Decoder
//
// Full implementation using AMD's AMF SDK via dynamic library loading.
// Loads amfrt64.dll at runtime - no compile-time AMD SDK dependency.
//
// Pipeline:
// 1. Load AMF runtime library
// 2. Query version and get factory
// 3. Create AMF context with D3D11
// 4. Create decoder component for codec
// 5. Feed compressed packets
// 6. Retrieve decoded surfaces
// 7. Copy to host memory

use std::ffi::c_void;
use std::ptr;
use std::sync::OnceLock;
use std::collections::VecDeque;
use serde::{Deserialize, Serialize};

// ============================================================================
// AMF Types (from AMF SDK headers)
// ============================================================================

type AmfResult = i32;
type AMFContext = *mut c_void;
type AMFComponent = *mut c_void;
type AMFSurface = *mut c_void;
type AMFBuffer = *mut c_void;
type AMFData = *mut c_void;
type AMFFactory = *mut c_void;
type AMFTrace = *mut c_void;
type AMFPlane = *mut c_void;

// AMF Result codes
const AMF_OK: AmfResult = 0;
const AMF_FAIL: AmfResult = 1;
const AMF_EOF: AmfResult = 11;
const AMF_REPEAT: AmfResult = 12;
const AMF_INPUT_FULL: AmfResult = 13;
const AMF_NEED_MORE_INPUT: AmfResult = 24;

// Memory types
const AMF_MEMORY_UNKNOWN: i32 = 0;
const AMF_MEMORY_HOST: i32 = 1;
const AMF_MEMORY_DX9: i32 = 2;
const AMF_MEMORY_DX11: i32 = 3;
const AMF_MEMORY_OPENCL: i32 = 4;
const AMF_MEMORY_OPENGL: i32 = 5;
const AMF_MEMORY_XV: i32 = 6;
const AMF_MEMORY_GRALLOC: i32 = 7;
const AMF_MEMORY_COMPUTE_FOR_DX9: i32 = 8;
const AMF_MEMORY_COMPUTE_FOR_DX11: i32 = 9;
const AMF_MEMORY_VULKAN: i32 = 10;
const AMF_MEMORY_DX12: i32 = 11;

// Surface formats
const AMF_SURFACE_UNKNOWN: i32 = 0;
const AMF_SURFACE_NV12: i32 = 1;
const AMF_SURFACE_YV12: i32 = 2;
const AMF_SURFACE_BGRA: i32 = 3;
const AMF_SURFACE_ARGB: i32 = 4;
const AMF_SURFACE_RGBA: i32 = 5;
const AMF_SURFACE_GRAY8: i32 = 6;
const AMF_SURFACE_YUV420P: i32 = 7;
const AMF_SURFACE_U8V8: i32 = 8;
const AMF_SURFACE_YUY2: i32 = 9;
const AMF_SURFACE_P010: i32 = 10;
const AMF_SURFACE_RGBA_F16: i32 = 11;
const AMF_SURFACE_UYVY: i32 = 12;
const AMF_SURFACE_Y210: i32 = 13;
const AMF_SURFACE_Y410: i32 = 14;
const AMF_SURFACE_Y416: i32 = 15;
const AMF_SURFACE_P012: i32 = 16;
const AMF_SURFACE_P016: i32 = 17;

// Decoder codec IDs
const AMF_VIDEO_DECODER_H264_AVC: &str = "AMFVideoDecoderUVD_H264_AVC";
const AMF_VIDEO_DECODER_H265_HEVC: &str = "AMFVideoDecoderHW_H265_HEVC";
const AMF_VIDEO_DECODER_H265_MAIN10: &str = "AMFVideoDecoderHW_H265_MAIN10";
const AMF_VIDEO_DECODER_VP9: &str = "AMFVideoDecoderHW_VP9";
const AMF_VIDEO_DECODER_VP9_10BIT: &str = "AMFVideoDecoderHW_VP9_10BIT";
const AMF_VIDEO_DECODER_AV1: &str = "AMFVideoDecoderHW_AV1";

// ============================================================================
// AMF Interface VTables
// ============================================================================

// AMFFactory interface
#[repr(C)]
struct AMFFactoryVtbl {
    acquire: unsafe extern "C" fn(*mut c_void) -> i64,
    release: unsafe extern "C" fn(*mut c_void) -> i64,
    get_trace: unsafe extern "C" fn(*mut c_void, *mut AMFTrace) -> AmfResult,
    get_debug: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_runtime: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    create_context: unsafe extern "C" fn(*mut c_void, *mut AMFContext) -> AmfResult,
    create_component: unsafe extern "C" fn(*mut c_void, AMFContext, *const u16, *mut AMFComponent) -> AmfResult,
    set_cache_folder: unsafe extern "C" fn(*mut c_void, *const u16) -> AmfResult,
    get_cache_folder: unsafe extern "C" fn(*mut c_void, *mut *const u16) -> AmfResult,
    get_debug_object: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    load_external_component: unsafe extern "C" fn(*mut c_void, AMFContext, *const u16, *const u16, *mut AMFComponent) -> AmfResult,
    unload_external_component: unsafe extern "C" fn(*mut c_void, *const u16) -> AmfResult,
}

#[repr(C)]
struct AMFFactoryObj {
    vtbl: *const AMFFactoryVtbl,
}

// AMFContext interface
#[repr(C)]
struct AMFContextVtbl {
    acquire: unsafe extern "C" fn(*mut c_void) -> i64,
    release: unsafe extern "C" fn(*mut c_void) -> i64,
    get_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut c_void) -> AmfResult,
    set_property: unsafe extern "C" fn(*mut c_void, *const u16, *const c_void) -> AmfResult,
    has_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "C" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "C" fn(*mut c_void, usize, *mut u16, usize, *mut c_void) -> AmfResult,
    clear: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    copy_to: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) -> AmfResult,
    add_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    terminate: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    init_dx9: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_dx9_device: unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
    lock_dx9: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    unlock_dx9: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    init_dx11: unsafe extern "C" fn(*mut c_void, *mut c_void, i32) -> AmfResult,
    get_dx11_device: unsafe extern "C" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
    lock_dx11: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    unlock_dx11: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    init_opengl: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> AmfResult,
    get_opengl_context: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_opengl_draw_context: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    lock_opengl: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    unlock_opengl: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    init_opencl: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_opencl_context: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_opencl_command_queue: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_opencl_device: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    lock_opencl: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    unlock_opencl: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    init_xv: unsafe extern "C" fn(*mut c_void, *mut c_void, *mut c_void) -> AmfResult,
    get_xv_device: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    lock_xv: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    unlock_xv: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    init_gralloc: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    lock_gralloc: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    unlock_gralloc: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    alloc_surface: unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32, *mut AMFSurface) -> AmfResult,
    create_surface_from_host_native: unsafe extern "C" fn(*mut c_void, *mut c_void, i32, i32, i32, i32, i32, *mut AMFSurface) -> AmfResult,
    alloc_buffer: unsafe extern "C" fn(*mut c_void, i32, usize, *mut AMFBuffer) -> AmfResult,
    create_buffer_from_host_native: unsafe extern "C" fn(*mut c_void, *mut c_void, usize, *mut AMFBuffer, *mut c_void) -> AmfResult,
    init_vulkan: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_vulkan_device: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    lock_vulkan: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    unlock_vulkan: unsafe extern "C" fn(*mut c_void) -> AmfResult,
}

#[repr(C)]
struct AMFContextObj {
    vtbl: *const AMFContextVtbl,
}

// AMFComponent interface  
#[repr(C)]
struct AMFComponentVtbl {
    acquire: unsafe extern "C" fn(*mut c_void) -> i64,
    release: unsafe extern "C" fn(*mut c_void) -> i64,
    get_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut c_void) -> AmfResult,
    set_property: unsafe extern "C" fn(*mut c_void, *const u16, *const c_void) -> AmfResult,
    has_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "C" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "C" fn(*mut c_void, usize, *mut u16, usize, *mut c_void) -> AmfResult,
    clear: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    copy_to: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) -> AmfResult,
    add_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    init: unsafe extern "C" fn(*mut c_void, i32, i32, i32) -> AmfResult,
    reinit: unsafe extern "C" fn(*mut c_void, i32, i32, i32) -> AmfResult,
    terminate: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    drain: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    flush: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    submit_input: unsafe extern "C" fn(*mut c_void, AMFData) -> AmfResult,
    query_output: unsafe extern "C" fn(*mut c_void, *mut AMFData) -> AmfResult,
    get_context: unsafe extern "C" fn(*mut c_void, *mut AMFContext) -> AmfResult,
    set_output_data_allocator: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_output_data_allocator: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_caps: unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    optimize: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
}

#[repr(C)]
struct AMFComponentObj {
    vtbl: *const AMFComponentVtbl,
}

// AMFData interface
#[repr(C)]
struct AMFDataVtbl {
    acquire: unsafe extern "C" fn(*mut c_void) -> i64,
    release: unsafe extern "C" fn(*mut c_void) -> i64,
    get_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut c_void) -> AmfResult,
    set_property: unsafe extern "C" fn(*mut c_void, *const u16, *const c_void) -> AmfResult,
    has_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "C" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "C" fn(*mut c_void, usize, *mut u16, usize, *mut c_void) -> AmfResult,
    clear: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    copy_to: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) -> AmfResult,
    add_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_memory_type: unsafe extern "C" fn(*mut c_void) -> i32,
    duplicate: unsafe extern "C" fn(*mut c_void, i32, *mut AMFData) -> AmfResult,
    convert: unsafe extern "C" fn(*mut c_void, i32) -> AmfResult,
    interop: unsafe extern "C" fn(*mut c_void, i32) -> AmfResult,
    get_data_type: unsafe extern "C" fn(*mut c_void) -> i32,
    is_reusable: unsafe extern "C" fn(*mut c_void) -> bool,
    set_pts: unsafe extern "C" fn(*mut c_void, i64),
    get_pts: unsafe extern "C" fn(*mut c_void) -> i64,
    set_duration: unsafe extern "C" fn(*mut c_void, i64),
    get_duration: unsafe extern "C" fn(*mut c_void) -> i64,
}

#[repr(C)]
struct AMFDataObj {
    vtbl: *const AMFDataVtbl,
}

// AMFBuffer interface (extends AMFData)
#[repr(C)]
struct AMFBufferVtbl {
    // Inherits from AMFData
    acquire: unsafe extern "C" fn(*mut c_void) -> i64,
    release: unsafe extern "C" fn(*mut c_void) -> i64,
    get_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut c_void) -> AmfResult,
    set_property: unsafe extern "C" fn(*mut c_void, *const u16, *const c_void) -> AmfResult,
    has_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "C" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "C" fn(*mut c_void, usize, *mut u16, usize, *mut c_void) -> AmfResult,
    clear: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    copy_to: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) -> AmfResult,
    add_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_memory_type: unsafe extern "C" fn(*mut c_void) -> i32,
    duplicate: unsafe extern "C" fn(*mut c_void, i32, *mut AMFData) -> AmfResult,
    convert: unsafe extern "C" fn(*mut c_void, i32) -> AmfResult,
    interop: unsafe extern "C" fn(*mut c_void, i32) -> AmfResult,
    get_data_type: unsafe extern "C" fn(*mut c_void) -> i32,
    is_reusable: unsafe extern "C" fn(*mut c_void) -> bool,
    set_pts: unsafe extern "C" fn(*mut c_void, i64),
    get_pts: unsafe extern "C" fn(*mut c_void) -> i64,
    set_duration: unsafe extern "C" fn(*mut c_void, i64),
    get_duration: unsafe extern "C" fn(*mut c_void) -> i64,
    // Buffer-specific
    set_size: unsafe extern "C" fn(*mut c_void, usize) -> AmfResult,
    get_size: unsafe extern "C" fn(*mut c_void) -> usize,
    get_native: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
}

#[repr(C)]
struct AMFBufferObj {
    vtbl: *const AMFBufferVtbl,
}

// AMFSurface interface
#[repr(C)]
struct AMFSurfaceVtbl {
    // Inherits from AMFData
    acquire: unsafe extern "C" fn(*mut c_void) -> i64,
    release: unsafe extern "C" fn(*mut c_void) -> i64,
    get_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut c_void) -> AmfResult,
    set_property: unsafe extern "C" fn(*mut c_void, *const u16, *const c_void) -> AmfResult,
    has_property: unsafe extern "C" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "C" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "C" fn(*mut c_void, usize, *mut u16, usize, *mut c_void) -> AmfResult,
    clear: unsafe extern "C" fn(*mut c_void) -> AmfResult,
    copy_to: unsafe extern "C" fn(*mut c_void, *mut c_void, bool) -> AmfResult,
    add_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "C" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_memory_type: unsafe extern "C" fn(*mut c_void) -> i32,
    duplicate: unsafe extern "C" fn(*mut c_void, i32, *mut AMFData) -> AmfResult,
    convert: unsafe extern "C" fn(*mut c_void, i32) -> AmfResult,
    interop: unsafe extern "C" fn(*mut c_void, i32) -> AmfResult,
    get_data_type: unsafe extern "C" fn(*mut c_void) -> i32,
    is_reusable: unsafe extern "C" fn(*mut c_void) -> bool,
    set_pts: unsafe extern "C" fn(*mut c_void, i64),
    get_pts: unsafe extern "C" fn(*mut c_void) -> i64,
    set_duration: unsafe extern "C" fn(*mut c_void, i64),
    get_duration: unsafe extern "C" fn(*mut c_void) -> i64,
    // Surface-specific
    get_format: unsafe extern "C" fn(*mut c_void) -> i32,
    get_planes_count: unsafe extern "C" fn(*mut c_void) -> usize,
    get_plane_at: unsafe extern "C" fn(*mut c_void, usize) -> AMFPlane,
    get_plane: unsafe extern "C" fn(*mut c_void, i32) -> AMFPlane,
    set_crop: unsafe extern "C" fn(*mut c_void, i32, i32, i32, i32) -> AmfResult,
}

#[repr(C)]
struct AMFSurfaceObj {
    vtbl: *const AMFSurfaceVtbl,
}

// AMFPlane interface
#[repr(C)]
struct AMFPlaneVtbl {
    get_type: unsafe extern "C" fn(*mut c_void) -> i32,
    get_native: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    get_pixel_size_in_bytes: unsafe extern "C" fn(*mut c_void) -> i32,
    get_offset_x: unsafe extern "C" fn(*mut c_void) -> i32,
    get_offset_y: unsafe extern "C" fn(*mut c_void) -> i32,
    get_width: unsafe extern "C" fn(*mut c_void) -> i32,
    get_height: unsafe extern "C" fn(*mut c_void) -> i32,
    get_hpitch: unsafe extern "C" fn(*mut c_void) -> i32,
    get_vpitch: unsafe extern "C" fn(*mut c_void) -> i32,
    is_tiled: unsafe extern "C" fn(*mut c_void) -> bool,
}

#[repr(C)]
struct AMFPlaneObj {
    vtbl: *const AMFPlaneVtbl,
}

// ============================================================================
// Dynamic Library Loading
// ============================================================================

#[cfg(target_os = "windows")]
const AMF_DLL: &str = "amfrt64.dll";

type AMFQueryVersionFn = unsafe extern "C" fn(*mut u64) -> AmfResult;
type AMFInitFn = unsafe extern "C" fn(u64, *mut *mut c_void) -> AmfResult;

struct AmfLibrary {
    _lib: libloading::Library,
    factory: *mut c_void,
    version: u64,
}

unsafe impl Send for AmfLibrary {}
unsafe impl Sync for AmfLibrary {}

static AMF_LIB: OnceLock<Option<AmfLibrary>> = OnceLock::new();

fn load_amf_library() -> Option<&'static AmfLibrary> {
    AMF_LIB.get_or_init(|| {
        #[cfg(target_os = "windows")]
        {
            unsafe {
                let lib = match libloading::Library::new(AMF_DLL) {
                    Ok(l) => l,
                    Err(e) => {
                        tracing::warn!("Failed to load AMF library: {}", e);
                        return None;
                    }
                };
                
                let query_version: AMFQueryVersionFn = *lib.get(b"AMFQueryVersion\0").ok()?;
                let init: AMFInitFn = *lib.get(b"AMFInit\0").ok()?;
                
                // Query version
                let mut version: u64 = 0;
                let result = query_version(&mut version);
                if result != AMF_OK {
                    tracing::warn!("AMFQueryVersion failed: {}", result);
                    return None;
                }
                
                // Initialize and get factory
                let mut factory: *mut c_void = ptr::null_mut();
                let result = init(version, &mut factory);
                if result != AMF_OK || factory.is_null() {
                    tracing::warn!("AMFInit failed: {}", result);
                    return None;
                }
                
                tracing::info!("AMF library loaded, version {}.{}.{}", 
                    (version >> 48) & 0xFFFF,
                    (version >> 32) & 0xFFFF,
                    version & 0xFFFFFFFF);
                
                Some(AmfLibrary {
                    _lib: lib,
                    factory,
                    version,
                })
            }
        }
        
        #[cfg(not(target_os = "windows"))]
        {
            None
        }
    }).as_ref()
}

// ============================================================================
// Public Types
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmfCodec {
    H264,
    H265,
    H265_10bit,
    VP9,
    VP9_10bit,
    AV1,
}

impl AmfCodec {
    fn to_amf_id(&self) -> &'static str {
        match self {
            Self::H264 => AMF_VIDEO_DECODER_H264_AVC,
            Self::H265 => AMF_VIDEO_DECODER_H265_HEVC,
            Self::H265_10bit => AMF_VIDEO_DECODER_H265_MAIN10,
            Self::VP9 => AMF_VIDEO_DECODER_VP9,
            Self::VP9_10bit => AMF_VIDEO_DECODER_VP9_10BIT,
            Self::AV1 => AMF_VIDEO_DECODER_AV1,
        }
    }
    
    fn to_wstring(&self) -> Vec<u16> {
        let s = self.to_amf_id();
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GpuGeneration {
    GCN1,
    GCN2,
    GCN3,
    GCN4,
    Polaris,
    Vega,
    Navi,
    Navi2,
    Navi3,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmfCapabilities {
    pub available: bool,
    pub version: String,
    pub gpu_name: String,
    pub gpu_generation: GpuGeneration,
    pub supported_codecs: Vec<String>,
    pub max_width: u32,
    pub max_height: u32,
    pub supports_10bit: bool,
}

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub pts: i64,
    pub width: u32,
    pub height: u32,
    pub format: SurfaceFormat,
    pub data: Vec<u8>,
    pub pitch: u32,
    pub progressive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SurfaceFormat {
    NV12,
    P010,
}

impl SurfaceFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            SurfaceFormat::NV12 => "NV12",
            SurfaceFormat::P010 => "P010",
        }
    }
}

// ============================================================================
// AMF Decoder
// ============================================================================

pub struct AmfDecoder {
    lib: &'static AmfLibrary,
    context: AMFContext,
    decoder: AMFComponent,
    codec: AmfCodec,
    width: u32,
    height: u32,
    bit_depth: u8,
}

/// Check if AMF is available
pub fn amf_available() -> bool {
    load_amf_library().is_some()
}

/// Get AMF capabilities
pub fn amf_capabilities() -> AmfCapabilities {
    let lib = match load_amf_library() {
        Some(l) => l,
        None => return AmfCapabilities {
            available: false,
            version: String::new(),
            gpu_name: String::new(),
            gpu_generation: GpuGeneration::Unknown,
            supported_codecs: Vec::new(),
            max_width: 0,
            max_height: 0,
            supports_10bit: false,
        },
    };
    
    let version = format!("{}.{}.{}", 
        (lib.version >> 48) & 0xFFFF,
        (lib.version >> 32) & 0xFFFF,
        lib.version & 0xFFFFFFFF);
    
    // Query GPU info would require initializing context
    // Return general capabilities
    AmfCapabilities {
        available: true,
        version,
        gpu_name: "AMD GPU".to_string(),
        gpu_generation: GpuGeneration::Unknown,
        supported_codecs: vec![
            "H.264".to_string(),
            "H.265".to_string(),
            "VP9".to_string(),
            "AV1".to_string(),
        ],
        max_width: 8192,
        max_height: 8192,
        supports_10bit: true,
    }
}

impl AmfDecoder {
    /// Create new AMF decoder
    pub fn new(codec: AmfCodec, width: u32, height: u32) -> Result<Self, String> {
        let lib = load_amf_library()
            .ok_or_else(|| "AMF not available".to_string())?;
        
        unsafe {
            let factory = lib.factory as *mut AMFFactoryObj;
            if factory.is_null() {
                return Err("Factory is null".to_string());
            }
            
            // Create context
            let mut context: AMFContext = ptr::null_mut();
            let result = ((*(*factory).vtbl).create_context)(factory as *mut c_void, &mut context);
            if result != AMF_OK {
                return Err(format!("CreateContext failed: {}", result));
            }
            
            let ctx = context as *mut AMFContextObj;
            
            // Initialize with D3D11
            let result = ((*(*ctx).vtbl).init_dx11)(ctx as *mut c_void, ptr::null_mut(), 0);
            if result != AMF_OK {
                ((*(*ctx).vtbl).release)(ctx as *mut c_void);
                return Err(format!("InitDX11 failed: {}", result));
            }
            
            // Create decoder component
            let codec_id = codec.to_wstring();
            let mut decoder: AMFComponent = ptr::null_mut();
            let result = ((*(*factory).vtbl).create_component)(
                factory as *mut c_void, 
                context, 
                codec_id.as_ptr(),
                &mut decoder
            );
            if result != AMF_OK {
                ((*(*ctx).vtbl).terminate)(ctx as *mut c_void);
                ((*(*ctx).vtbl).release)(ctx as *mut c_void);
                return Err(format!("CreateComponent failed: {}", result));
            }
            
            let dec = decoder as *mut AMFComponentObj;
            
            // Initialize decoder
            let surface_format = match codec {
                AmfCodec::H265_10bit | AmfCodec::VP9_10bit => AMF_SURFACE_P010,
                _ => AMF_SURFACE_NV12,
            };
            
            let result = ((*(*dec).vtbl).init)(dec as *mut c_void, surface_format, width as i32, height as i32);
            if result != AMF_OK {
                ((*(*dec).vtbl).release)(dec as *mut c_void);
                ((*(*ctx).vtbl).terminate)(ctx as *mut c_void);
                ((*(*ctx).vtbl).release)(ctx as *mut c_void);
                return Err(format!("Decoder Init failed: {}", result));
            }
            
            let bit_depth = match codec {
                AmfCodec::H265_10bit | AmfCodec::VP9_10bit => 10,
                _ => 8,
            };
            
            tracing::info!("AMF decoder created for {:?} {}x{}", codec, width, height);
            
            Ok(Self {
                lib,
                context,
                decoder,
                codec,
                width,
                height,
                bit_depth,
            })
        }
    }
    
    /// Submit compressed data for decoding
    pub fn decode(&mut self, data: &[u8], pts: i64) -> Result<Option<DecodedFrame>, String> {
        unsafe {
            let ctx = self.context as *mut AMFContextObj;
            let dec = self.decoder as *mut AMFComponentObj;
            
            // Allocate buffer
            let mut buffer: AMFBuffer = ptr::null_mut();
            let result = ((*(*ctx).vtbl).alloc_buffer)(ctx as *mut c_void, AMF_MEMORY_HOST, data.len(), &mut buffer);
            if result != AMF_OK {
                return Err(format!("AllocBuffer failed: {}", result));
            }
            
            let buf = buffer as *mut AMFBufferObj;
            
            // Copy data to buffer
            let native = ((*(*buf).vtbl).get_native)(buf as *mut c_void);
            if !native.is_null() {
                ptr::copy_nonoverlapping(data.as_ptr(), native as *mut u8, data.len());
            }
            
            // Set PTS
            ((*(*buf).vtbl).set_pts)(buf as *mut c_void, pts);
            
            // Submit input
            let result = ((*(*dec).vtbl).submit_input)(dec as *mut c_void, buffer);
            ((*(*buf).vtbl).release)(buf as *mut c_void);
            
            if result != AMF_OK && result != AMF_INPUT_FULL && result != AMF_NEED_MORE_INPUT {
                return Err(format!("SubmitInput failed: {}", result));
            }
            
            // Try to get output
            self.query_output()
        }
    }
    
    fn query_output(&mut self) -> Result<Option<DecodedFrame>, String> {
        unsafe {
            let dec = self.decoder as *mut AMFComponentObj;
            
            let mut output: AMFData = ptr::null_mut();
            let result = ((*(*dec).vtbl).query_output)(dec as *mut c_void, &mut output);
            
            if result == AMF_REPEAT || result == AMF_EOF || output.is_null() {
                return Ok(None);
            }
            
            if result != AMF_OK {
                return Err(format!("QueryOutput failed: {}", result));
            }
            
            // Cast to surface
            let surface = output as *mut AMFSurfaceObj;
            let data_obj = output as *mut AMFDataObj;
            
            // Get PTS
            let pts = ((*(*data_obj).vtbl).get_pts)(data_obj as *mut c_void);
            
            // Convert to host memory
            let result = ((*(*data_obj).vtbl).convert)(data_obj as *mut c_void, AMF_MEMORY_HOST);
            if result != AMF_OK {
                ((*(*data_obj).vtbl).release)(data_obj as *mut c_void);
                return Err(format!("Convert to host failed: {}", result));
            }
            
            // Get plane data
            let plane_count = ((*(*surface).vtbl).get_planes_count)(surface as *mut c_void);
            if plane_count == 0 {
                ((*(*data_obj).vtbl).release)(data_obj as *mut c_void);
                return Ok(None);
            }
            
            // Get Y plane
            let y_plane = ((*(*surface).vtbl).get_plane_at)(surface as *mut c_void, 0);
            if y_plane.is_null() {
                ((*(*data_obj).vtbl).release)(data_obj as *mut c_void);
                return Err("Y plane is null".to_string());
            }
            
            let y_plane_obj = y_plane as *mut AMFPlaneObj;
            let y_width = ((*(*y_plane_obj).vtbl).get_width)(y_plane as *mut c_void) as u32;
            let y_height = ((*(*y_plane_obj).vtbl).get_height)(y_plane as *mut c_void) as u32;
            let y_pitch = ((*(*y_plane_obj).vtbl).get_hpitch)(y_plane as *mut c_void) as u32;
            let y_native = ((*(*y_plane_obj).vtbl).get_native)(y_plane as *mut c_void);
            
            // Calculate sizes
            let y_size = (y_pitch * y_height) as usize;
            let uv_size = (y_pitch * y_height / 2) as usize;
            let total_size = y_size + uv_size;
            
            // Copy data
            let mut frame_data = vec![0u8; total_size];
            if !y_native.is_null() {
                ptr::copy_nonoverlapping(y_native as *const u8, frame_data.as_mut_ptr(), total_size);
            }
            
            let format = if self.bit_depth > 8 {
                SurfaceFormat::P010
            } else {
                SurfaceFormat::NV12
            };
            
            ((*(*data_obj).vtbl).release)(data_obj as *mut c_void);
            
            Ok(Some(DecodedFrame {
                pts,
                width: y_width,
                height: y_height,
                format,
                data: frame_data,
                pitch: y_pitch,
                progressive: true, // AMF decodes to progressive
            }))
        }
    }
    
    /// Flush decoder
    pub fn flush(&mut self) -> Vec<DecodedFrame> {
        unsafe {
            let dec = self.decoder as *mut AMFComponentObj;
            let _ = ((*(*dec).vtbl).drain)(dec as *mut c_void);
        }
        
        let mut frames = Vec::new();
        while let Ok(Some(frame)) = self.query_output() {
            frames.push(frame);
        }
        frames
    }
    
    /// Get decoder info
    pub fn info(&self) -> serde_json::Value {
        serde_json::json!({
            "backend": "amf",
            "codec": format!("{:?}", self.codec),
            "width": self.width,
            "height": self.height,
            "bit_depth": self.bit_depth,
            "output_format": if self.bit_depth > 8 { "P010" } else { "NV12" },
        })
    }
}

impl Drop for AmfDecoder {
    fn drop(&mut self) {
        unsafe {
            let dec = self.decoder as *mut AMFComponentObj;
            let ctx = self.context as *mut AMFContextObj;
            
            if !dec.is_null() {
                ((*(*dec).vtbl).terminate)(dec as *mut c_void);
                ((*(*dec).vtbl).release)(dec as *mut c_void);
            }
            
            if !ctx.is_null() {
                ((*(*ctx).vtbl).terminate)(ctx as *mut c_void);
                ((*(*ctx).vtbl).release)(ctx as *mut c_void);
            }
        }
    }
}

// ============================================================================
// Public Rust API
// ============================================================================




pub fn amf_check_available() -> bool {
    amf_available()
}


pub fn amf_get_capabilities() -> serde_json::Value {
    serde_json::to_value(amf_capabilities()).unwrap_or_default()
}


pub fn amf_supported_codecs(gpu_gen: String) -> Vec<String> {
    let gen = match gpu_gen.to_lowercase().as_str() {
        "navi3" | "rdna3" => GpuGeneration::Navi3,
        "navi2" | "rdna2" => GpuGeneration::Navi2,
        "navi" | "rdna" => GpuGeneration::Navi,
        "vega" => GpuGeneration::Vega,
        "polaris" => GpuGeneration::Polaris,
        _ => GpuGeneration::Unknown,
    };
    
    let mut codecs = vec!["H.264".to_string()];
    
    match gen {
        GpuGeneration::Polaris | GpuGeneration::Vega | 
        GpuGeneration::Navi | GpuGeneration::Navi2 | GpuGeneration::Navi3 => {
            codecs.push("H.265".to_string());
        }
        _ => {}
    }
    
    match gen {
        GpuGeneration::Vega | GpuGeneration::Navi | 
        GpuGeneration::Navi2 | GpuGeneration::Navi3 => {
            codecs.push("VP9".to_string());
        }
        _ => {}
    }
    
    match gen {
        GpuGeneration::Navi2 | GpuGeneration::Navi3 => {
            codecs.push("AV1".to_string());
        }
        _ => {}
    }
    
    codecs
}


pub fn amf_description() -> String {
    r#"
AMF DECODE - AMD Advanced Media Framework (IMPLEMENTED)

Loads amfrt64.dll at runtime via libloading.
No compile-time AMD SDK dependency required.

SUPPORTED CODECS:
• H.264/AVC - All AMD GPUs with UVD
• H.265/HEVC - Polaris+ (RX 400+)
• VP9 - Vega+ (RX Vega, RX 5000+)
• AV1 - RDNA2+ (RX 6000+)

GPU GENERATIONS:
• Polaris (RX 400/500): H.264, H.265
• Vega: H.264, H.265, VP9
• RDNA (RX 5000): H.264, H.265, VP9
• RDNA2 (RX 6000): H.264, H.265, VP9, AV1
• RDNA3 (RX 7000): H.264, H.265, VP9, AV1

PIPELINE:
1. Load amfrt64.dll dynamically
2. Initialize AMF factory
3. Create D3D11 context
4. Create decoder component
5. Submit compressed packets
6. Query decoded surfaces
7. Convert to host memory
"#.to_string()
}
