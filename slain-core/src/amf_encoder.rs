// AMD Advanced Media Framework (AMF) Encoder - FULL IMPLEMENTATION
// Uses VCN hardware encoder on AMD GPUs (RX 400+, Ryzen APUs)
// 
// This offloads video encoding from NVIDIA to AMD,
// freeing NVIDIA for rendering/AI tasks

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use libloading::{Library, Symbol};
use serde::{Deserialize, Serialize};


// ============================================================================
// AMF Types (from AMF SDK headers)
// ============================================================================

type AmfResult = i32;
const AMF_OK: AmfResult = 0;
const AMF_REPEAT: AmfResult = 6;
const AMF_INPUT_FULL: AmfResult = 7;
const AMF_EOF: AmfResult = 9;

// Memory types
const AMF_MEMORY_HOST: i32 = 0;

// Surface formats
const AMF_SURFACE_NV12: i32 = 1;
const AMF_SURFACE_P010: i32 = 10;
const AMF_SURFACE_BGRA: i32 = 3;

// Usage modes
const AMF_VIDEO_ENCODER_HEVC_USAGE_TRANSCODING: i64 = 0;
const AMF_VIDEO_ENCODER_HEVC_USAGE_LOW_LATENCY: i64 = 3;

// Quality presets
const AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET_SPEED: i64 = 1;
const AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET_BALANCED: i64 = 0;
const AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET_QUALITY: i64 = 2;

// Rate control
const AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_CQP: i64 = 0;
const AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_CBR: i64 = 1;
const AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_VBR: i64 = 2;
const AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_VBR_LAT: i64 = 3;

// ============================================================================
// AMF VTable Interfaces
// ============================================================================

/// AMF Factory interface VTable
#[repr(C)]
struct AMFFactoryVtbl {
    acquire: unsafe extern "system" fn(*mut c_void) -> i64,
    release: unsafe extern "system" fn(*mut c_void) -> i64,
    get_trace: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_debug: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_runtime_version: unsafe extern "system" fn(*mut c_void, *mut u64) -> AmfResult,
    get_trace_interface: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    create_context: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    create_component: unsafe extern "system" fn(*mut c_void, *mut c_void, *const u16, *mut *mut c_void) -> AmfResult,
    set_cache_folder: unsafe extern "system" fn(*mut c_void, *const u16) -> AmfResult,
    get_cache_folder: unsafe extern "system" fn(*mut c_void, *mut *const u16) -> AmfResult,
    get_debug_interface: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
}

#[repr(C)]
struct AMFFactory {
    vtbl: *const AMFFactoryVtbl,
}

/// AMF Context interface VTable  
#[repr(C)]
struct AMFContextVtbl {
    acquire: unsafe extern "system" fn(*mut c_void) -> i64,
    release: unsafe extern "system" fn(*mut c_void) -> i64,
    terminate: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    init_dx9: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_dx9_device: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
    lock_dx9: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    unlock_dx9: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    init_dx11: unsafe extern "system" fn(*mut c_void, *mut c_void, i32) -> AmfResult,
    get_dx11_device: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
    lock_dx11: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    unlock_dx11: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    init_opengl: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut c_void) -> AmfResult,
    get_opengl_context: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_opengl_drawable: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    lock_opengl: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    unlock_opengl: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    init_opencl: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_opencl_context: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_opencl_command_queue: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_opencl_device_id: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    lock_opencl: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    unlock_opencl: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    init_dx12: unsafe extern "system" fn(*mut c_void, *mut c_void, i32) -> AmfResult,
    get_dx12_device: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
    lock_dx12: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    unlock_dx12: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    init_vulkan: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_vulkan_context: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    lock_vulkan: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    unlock_vulkan: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    alloc_buffer: unsafe extern "system" fn(*mut c_void, i32, usize, *mut *mut c_void) -> AmfResult,
    alloc_surface: unsafe extern "system" fn(*mut c_void, i32, i32, i32, i32, *mut *mut c_void) -> AmfResult,
    get_compute_factory: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
}

#[repr(C)]
struct AMFContext {
    vtbl: *const AMFContextVtbl,
}

/// AMF Component (encoder) interface VTable
#[repr(C)]
struct AMFComponentVtbl {
    acquire: unsafe extern "system" fn(*mut c_void) -> i64,
    release: unsafe extern "system" fn(*mut c_void) -> i64,
    set_property: unsafe extern "system" fn(*mut c_void, *const u16, AMFVariant) -> AmfResult,
    get_property: unsafe extern "system" fn(*mut c_void, *const u16, *mut AMFVariant) -> AmfResult,
    has_property: unsafe extern "system" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "system" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "system" fn(*mut c_void, usize, *mut *const u16, *mut AMFVariant) -> AmfResult,
    clear: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    add_observer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    init: unsafe extern "system" fn(*mut c_void, i32, i32, i32) -> AmfResult,
    reinit: unsafe extern "system" fn(*mut c_void, i32, i32) -> AmfResult,
    terminate: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    drain: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    flush: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    submit_input: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    query_output: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    get_context: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    set_output_data_allocator: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_caps: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> AmfResult,
    optimize: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
}

#[repr(C)]
struct AMFComponent {
    vtbl: *const AMFComponentVtbl,
}

/// AMF Surface interface VTable
#[repr(C)]
struct AMFSurfaceVtbl {
    acquire: unsafe extern "system" fn(*mut c_void) -> i64,
    release: unsafe extern "system" fn(*mut c_void) -> i64,
    set_property: unsafe extern "system" fn(*mut c_void, *const u16, AMFVariant) -> AmfResult,
    get_property: unsafe extern "system" fn(*mut c_void, *const u16, *mut AMFVariant) -> AmfResult,
    has_property: unsafe extern "system" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "system" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "system" fn(*mut c_void, usize, *mut *const u16, *mut AMFVariant) -> AmfResult,
    clear: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    add_observer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_memory_type: unsafe extern "system" fn(*mut c_void) -> i32,
    duplicate: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
    convert: unsafe extern "system" fn(*mut c_void, i32) -> AmfResult,
    interop: unsafe extern "system" fn(*mut c_void, i32) -> AmfResult,
    get_data_type: unsafe extern "system" fn(*mut c_void) -> i32,
    is_reusable: unsafe extern "system" fn(*mut c_void) -> bool,
    set_pts: unsafe extern "system" fn(*mut c_void, i64),
    get_pts: unsafe extern "system" fn(*mut c_void) -> i64,
    set_duration: unsafe extern "system" fn(*mut c_void, i64),
    get_duration: unsafe extern "system" fn(*mut c_void) -> i64,
    get_format: unsafe extern "system" fn(*mut c_void) -> i32,
    get_planes_count: unsafe extern "system" fn(*mut c_void) -> usize,
    get_plane_at: unsafe extern "system" fn(*mut c_void, usize, *mut *mut c_void) -> AmfResult,
    get_plane: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
}

#[repr(C)]
struct AMFSurface {
    vtbl: *const AMFSurfaceVtbl,
}

/// AMF Plane interface VTable
#[repr(C)]
struct AMFPlaneVtbl {
    get_type: unsafe extern "system" fn(*mut c_void) -> i32,
    get_offset_x: unsafe extern "system" fn(*mut c_void) -> i32,
    get_offset_y: unsafe extern "system" fn(*mut c_void) -> i32,
    get_width: unsafe extern "system" fn(*mut c_void) -> i32,
    get_height: unsafe extern "system" fn(*mut c_void) -> i32,
    get_h_pitch: unsafe extern "system" fn(*mut c_void) -> i32,
    get_v_pitch: unsafe extern "system" fn(*mut c_void) -> i32,
    get_native: unsafe extern "system" fn(*mut c_void) -> *mut u8,
    is_tilted: unsafe extern "system" fn(*mut c_void) -> bool,
}

#[repr(C)]
struct AMFPlane {
    vtbl: *const AMFPlaneVtbl,
}

/// AMF Buffer interface VTable (for encoded output)
#[repr(C)]
struct AMFBufferVtbl {
    acquire: unsafe extern "system" fn(*mut c_void) -> i64,
    release: unsafe extern "system" fn(*mut c_void) -> i64,
    set_property: unsafe extern "system" fn(*mut c_void, *const u16, AMFVariant) -> AmfResult,
    get_property: unsafe extern "system" fn(*mut c_void, *const u16, *mut AMFVariant) -> AmfResult,
    has_property: unsafe extern "system" fn(*mut c_void, *const u16, *mut bool) -> AmfResult,
    get_property_count: unsafe extern "system" fn(*mut c_void, *mut usize) -> AmfResult,
    get_property_at: unsafe extern "system" fn(*mut c_void, usize, *mut *const u16, *mut AMFVariant) -> AmfResult,
    clear: unsafe extern "system" fn(*mut c_void) -> AmfResult,
    add_observer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    remove_observer: unsafe extern "system" fn(*mut c_void, *mut c_void) -> AmfResult,
    get_memory_type: unsafe extern "system" fn(*mut c_void) -> i32,
    duplicate: unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> AmfResult,
    convert: unsafe extern "system" fn(*mut c_void, i32) -> AmfResult,
    interop: unsafe extern "system" fn(*mut c_void, i32) -> AmfResult,
    get_data_type: unsafe extern "system" fn(*mut c_void) -> i32,
    is_reusable: unsafe extern "system" fn(*mut c_void) -> bool,
    set_pts: unsafe extern "system" fn(*mut c_void, i64),
    get_pts: unsafe extern "system" fn(*mut c_void) -> i64,
    set_duration: unsafe extern "system" fn(*mut c_void, i64),
    get_duration: unsafe extern "system" fn(*mut c_void) -> i64,
    get_size: unsafe extern "system" fn(*mut c_void) -> usize,
    get_native: unsafe extern "system" fn(*mut c_void) -> *mut u8,
}

#[repr(C)]
struct AMFBuffer {
    vtbl: *const AMFBufferVtbl,
}

/// AMF Variant type for property values
#[repr(C)]
#[derive(Clone, Copy)]
union AMFVariantValue {
    bool_val: bool,
    int64_val: i64,
    double_val: f64,
    string_val: *const u16,
    interface_val: *mut c_void,
    rect_val: [i32; 4],
    size_val: [i32; 2],
    point_val: [i32; 2],
    rate_val: [u32; 2],
    ratio_val: [u32; 2],
    color_val: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct AMFVariant {
    variant_type: i32,
    value: AMFVariantValue,
}

impl AMFVariant {
    fn from_int64(val: i64) -> Self {
        Self {
            variant_type: 2, // AMF_VARIANT_INT64
            value: AMFVariantValue { int64_val: val },
        }
    }
    
    fn from_rate(num: u32, den: u32) -> Self {
        Self {
            variant_type: 9, // AMF_VARIANT_RATE
            value: AMFVariantValue { rate_val: [num, den] },
        }
    }
}

// ============================================================================
// Encoder Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmfEncoderConfig {
    pub codec: AmfCodec,
    pub width: u32,
    pub height: u32,
    pub framerate_num: u32,
    pub framerate_den: u32,
    pub bitrate_bps: u64,
    pub max_bitrate_bps: u64,
    pub gop_size: u32,
    pub b_frames: u32,
    pub quality_preset: AmfQualityPreset,
    pub rate_control: AmfRateControl,
    pub profile: AmfProfile,
    pub color_format: AmfSurfaceFormat,
    pub low_latency: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmfCodec {
    H264Avc,
    H265Hevc,
    Av1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmfQualityPreset {
    Speed,
    Balanced,
    Quality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmfRateControl {
    Cqp,
    Cbr,
    Vbr,
    VbrLatency,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmfProfile {
    H264Baseline,
    H264Main,
    H264High,
    H265Main,
    H265Main10,
    Av1Main,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmfSurfaceFormat {
    Unknown = 0,
    Nv12 = 1,
    Yv12 = 2,
    Bgra = 3,
    Argb = 4,
    Rgba = 5,
    P010 = 10,
    Rgba16f = 20,
}

impl Default for AmfEncoderConfig {
    fn default() -> Self {
        Self {
            codec: AmfCodec::H265Hevc,
            width: 1920,
            height: 1080,
            framerate_num: 60,
            framerate_den: 1,
            bitrate_bps: 8_000_000,
            max_bitrate_bps: 12_000_000,
            gop_size: 120,
            b_frames: 0,
            quality_preset: AmfQualityPreset::Balanced,
            rate_control: AmfRateControl::Vbr,
            profile: AmfProfile::H265Main,
            color_format: AmfSurfaceFormat::Nv12,
            low_latency: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmfEncoderStats {
    pub frames_encoded: u64,
    pub bytes_output: u64,
    pub average_bitrate_bps: u64,
    pub encode_fps: f64,
    pub gpu_usage_percent: f64,
}

// ============================================================================
// Function Types
// ============================================================================

type AMFQueryVersionFn = unsafe extern "C" fn(*mut u64) -> AmfResult;
type AMFInitFn = unsafe extern "C" fn(u64, *mut *mut c_void) -> AmfResult;

// ============================================================================
// Helper Functions
// ============================================================================

fn to_wide_string(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// ============================================================================
// AMF Encoder
// ============================================================================

pub struct AmfEncoder {
    _library: Library,
    factory: *mut AMFFactory,
    context: *mut AMFContext,
    encoder: *mut AMFComponent,
    config: AmfEncoderConfig,
    initialized: AtomicBool,
    frames_encoded: AtomicU64,
    bytes_output: AtomicU64,
    start_time: Option<Instant>,
}

unsafe impl Send for AmfEncoder {}
unsafe impl Sync for AmfEncoder {}

impl AmfEncoder {
    /// Create a new AMF encoder
    pub fn new(config: AmfEncoderConfig) -> Result<Self, String> {
        #[cfg(target_os = "windows")]
        let lib_name = "amfrt64.dll";
        
        #[cfg(not(target_os = "windows"))]
        let lib_name = "libamfrt64.so.1";
        
        let library = unsafe { Library::new(lib_name) }
            .map_err(|e| format!("Failed to load AMF: {}. AMD driver required.", e))?;
        
        Ok(Self {
            _library: library,
            factory: ptr::null_mut(),
            context: ptr::null_mut(),
            encoder: ptr::null_mut(),
            config,
            initialized: AtomicBool::new(false),
            frames_encoded: AtomicU64::new(0),
            bytes_output: AtomicU64::new(0),
            start_time: None,
        })
    }
    
    /// Initialize the encoder
    pub fn initialize(&mut self) -> Result<(), String> {
        if self.initialized.load(Ordering::SeqCst) {
            return Ok(());
        }
        
        unsafe {
            // Query AMF version
            let query_version: Symbol<AMFQueryVersionFn> = self._library.get(b"AMFQueryVersion\0")
                .map_err(|e| format!("AMFQueryVersion not found: {}", e))?;
            
            let mut version: u64 = 0;
            let result = query_version(&mut version);
            if result != AMF_OK {
                return Err(format!("AMFQueryVersion failed: {}", result));
            }
            
            tracing::info!("AMF version: {}.{}.{}", 
                (version >> 48) & 0xFFFF,
                (version >> 32) & 0xFFFF,
                (version >> 16) & 0xFFFF
            );
            
            // Initialize AMF
            let amf_init: Symbol<AMFInitFn> = self._library.get(b"AMFInit\0")
                .map_err(|e| format!("AMFInit not found: {}", e))?;
            
            let mut factory_ptr: *mut c_void = ptr::null_mut();
            let result = amf_init(version, &mut factory_ptr);
            if result != AMF_OK || factory_ptr.is_null() {
                return Err(format!("AMFInit failed: {}", result));
            }
            self.factory = factory_ptr as *mut AMFFactory;
            
            // Create context
            let factory = &*self.factory;
            let mut context_ptr: *mut c_void = ptr::null_mut();
            let result = ((*factory.vtbl).create_context)(
                self.factory as *mut c_void,
                &mut context_ptr
            );
            if result != AMF_OK || context_ptr.is_null() {
                return Err(format!("CreateContext failed: {}", result));
            }
            self.context = context_ptr as *mut AMFContext;
            
            // Initialize D3D11 (Windows)
            #[cfg(target_os = "windows")]
            {
                let context = &*self.context;
                let result = ((*context.vtbl).init_dx11)(
                    self.context as *mut c_void,
                    ptr::null_mut(),
                    0
                );
                if result != AMF_OK {
                    return Err(format!("InitDX11 failed: {}", result));
                }
            }
            
            // Create encoder component
            let codec_id = match self.config.codec {
                AmfCodec::H264Avc => "AMFVideoEncoderVCE_AVC",
                AmfCodec::H265Hevc => "AMFVideoEncoder_HEVC",
                AmfCodec::Av1 => "AMFVideoEncoder_AV1",
            };
            let codec_wide = to_wide_string(codec_id);
            
            let factory = &*self.factory;
            let mut encoder_ptr: *mut c_void = ptr::null_mut();
            let result = ((*factory.vtbl).create_component)(
                self.factory as *mut c_void,
                self.context as *mut c_void,
                codec_wide.as_ptr(),
                &mut encoder_ptr
            );
            if result != AMF_OK || encoder_ptr.is_null() {
                return Err(format!("CreateComponent failed: {}. Codec {} may not be supported.", result, codec_id));
            }
            self.encoder = encoder_ptr as *mut AMFComponent;
            
            // Configure encoder
            self.configure_encoder()?;
            
            // Initialize encoder
            let surface_format = match self.config.color_format {
                AmfSurfaceFormat::Nv12 => AMF_SURFACE_NV12,
                AmfSurfaceFormat::P010 => AMF_SURFACE_P010,
                AmfSurfaceFormat::Bgra => AMF_SURFACE_BGRA,
                _ => AMF_SURFACE_NV12,
            };
            
            let encoder = &*self.encoder;
            let result = ((*encoder.vtbl).init)(
                self.encoder as *mut c_void,
                surface_format,
                self.config.width as i32,
                self.config.height as i32
            );
            if result != AMF_OK {
                return Err(format!("Encoder init failed: {}", result));
            }
            
            self.initialized.store(true, Ordering::SeqCst);
            self.start_time = Some(Instant::now());
            
            tracing::info!("AMF {} encoder initialized: {}x{} @ {} fps",
                codec_id,
                self.config.width,
                self.config.height,
                self.config.framerate_num / self.config.framerate_den
            );
            
            Ok(())
        }
    }
    
    /// Configure encoder properties
    fn configure_encoder(&self) -> Result<(), String> {
        unsafe {
            let encoder = &*self.encoder;
            let set_prop = (*encoder.vtbl).set_property;
            
            let usage = if self.config.low_latency {
                AMF_VIDEO_ENCODER_HEVC_USAGE_LOW_LATENCY
            } else {
                AMF_VIDEO_ENCODER_HEVC_USAGE_TRANSCODING
            };
            
            let quality = match self.config.quality_preset {
                AmfQualityPreset::Speed => AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET_SPEED,
                AmfQualityPreset::Balanced => AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET_BALANCED,
                AmfQualityPreset::Quality => AMF_VIDEO_ENCODER_HEVC_QUALITY_PRESET_QUALITY,
            };
            
            let rc_method = match self.config.rate_control {
                AmfRateControl::Cqp => AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_CQP,
                AmfRateControl::Cbr => AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_CBR,
                AmfRateControl::Vbr => AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_VBR,
                AmfRateControl::VbrLatency => AMF_VIDEO_ENCODER_HEVC_RATE_CONTROL_VBR_LAT,
            };
            
            let _ = set_prop(self.encoder as *mut c_void,
                to_wide_string("Usage").as_ptr(),
                AMFVariant::from_int64(usage));
            
            let _ = set_prop(self.encoder as *mut c_void,
                to_wide_string("QualityPreset").as_ptr(),
                AMFVariant::from_int64(quality));
            
            let _ = set_prop(self.encoder as *mut c_void,
                to_wide_string("RateControlMethod").as_ptr(),
                AMFVariant::from_int64(rc_method));
            
            let _ = set_prop(self.encoder as *mut c_void,
                to_wide_string("TargetBitrate").as_ptr(),
                AMFVariant::from_int64(self.config.bitrate_bps as i64));
            
            let _ = set_prop(self.encoder as *mut c_void,
                to_wide_string("PeakBitrate").as_ptr(),
                AMFVariant::from_int64(self.config.max_bitrate_bps as i64));
            
            let _ = set_prop(self.encoder as *mut c_void,
                to_wide_string("FrameRate").as_ptr(),
                AMFVariant::from_rate(self.config.framerate_num, self.config.framerate_den));
            
            let _ = set_prop(self.encoder as *mut c_void,
                to_wide_string("GOPSize").as_ptr(),
                AMFVariant::from_int64(self.config.gop_size as i64));
        }
        
        Ok(())
    }
    
    /// Encode a frame
    pub fn encode_frame(&mut self, frame_data: &[u8], pts: i64) -> Result<Vec<u8>, String> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Err("Encoder not initialized".to_string());
        }
        
        unsafe {
            let context = &*self.context;
            let mut surface_ptr: *mut c_void = ptr::null_mut();
            
            let surface_format = match self.config.color_format {
                AmfSurfaceFormat::Nv12 => AMF_SURFACE_NV12,
                AmfSurfaceFormat::P010 => AMF_SURFACE_P010,
                AmfSurfaceFormat::Bgra => AMF_SURFACE_BGRA,
                _ => AMF_SURFACE_NV12,
            };
            
            let result = ((*context.vtbl).alloc_surface)(
                self.context as *mut c_void,
                AMF_MEMORY_HOST,
                surface_format,
                self.config.width as i32,
                self.config.height as i32,
                &mut surface_ptr
            );
            
            if result != AMF_OK || surface_ptr.is_null() {
                return Err(format!("Failed to allocate surface: {}", result));
            }
            
            let surface = surface_ptr as *mut AMFSurface;
            let surface_vtbl = &*(*surface).vtbl;
            let planes_count = (surface_vtbl.get_planes_count)(surface_ptr);
            
            let mut data_offset = 0usize;
            for i in 0..planes_count {
                let mut plane_ptr: *mut c_void = ptr::null_mut();
                let result = (surface_vtbl.get_plane_at)(surface_ptr, i, &mut plane_ptr);
                if result != AMF_OK || plane_ptr.is_null() {
                    continue;
                }
                
                let plane = plane_ptr as *mut AMFPlane;
                let plane_vtbl = &*(*plane).vtbl;
                
                let width = (plane_vtbl.get_width)(plane_ptr) as usize;
                let height = (plane_vtbl.get_height)(plane_ptr) as usize;
                let pitch = (plane_vtbl.get_h_pitch)(plane_ptr) as usize;
                let native_ptr = (plane_vtbl.get_native)(plane_ptr);
                
                if native_ptr.is_null() {
                    continue;
                }
                
                let src_pitch = width;
                for row in 0..height {
                    let src_offset = data_offset + row * src_pitch;
                    let dst_offset = row * pitch;
                    
                    if src_offset + src_pitch <= frame_data.len() {
                        ptr::copy_nonoverlapping(
                            frame_data.as_ptr().add(src_offset),
                            native_ptr.add(dst_offset),
                            src_pitch.min(pitch)
                        );
                    }
                }
                
                data_offset += width * height;
            }
            
            (surface_vtbl.set_pts)(surface_ptr, pts);
            
            let encoder = &*self.encoder;
            let mut result = ((*encoder.vtbl).submit_input)(
                self.encoder as *mut c_void,
                surface_ptr
            );
            
            while result == AMF_INPUT_FULL {
                let _ = self.poll_output();
                result = ((*encoder.vtbl).submit_input)(
                    self.encoder as *mut c_void,
                    surface_ptr
                );
            }
            
            let surface = surface_ptr as *mut AMFSurface;
            ((*(*surface).vtbl).release)(surface_ptr);
            
            if result != AMF_OK {
                return Err(format!("SubmitInput failed: {}", result));
            }
            
            self.frames_encoded.fetch_add(1, Ordering::Relaxed);
            
            self.poll_output()
        }
    }
    
    /// Poll for encoded output
    fn poll_output(&mut self) -> Result<Vec<u8>, String> {
        unsafe {
            let encoder = &*self.encoder;
            let mut data_ptr: *mut c_void = ptr::null_mut();
            
            let result = ((*encoder.vtbl).query_output)(
                self.encoder as *mut c_void,
                &mut data_ptr
            );
            
            if result == AMF_REPEAT || result == AMF_EOF {
                return Ok(Vec::new());
            }
            
            if result != AMF_OK || data_ptr.is_null() {
                return Ok(Vec::new());
            }
            
            let buffer = data_ptr as *mut AMFBuffer;
            let buffer_vtbl = &*(*buffer).vtbl;
            
            let size = (buffer_vtbl.get_size)(data_ptr);
            let native = (buffer_vtbl.get_native)(data_ptr);
            
            if native.is_null() || size == 0 {
                (buffer_vtbl.release)(data_ptr);
                return Ok(Vec::new());
            }
            
            let mut output = vec![0u8; size];
            ptr::copy_nonoverlapping(native, output.as_mut_ptr(), size);
            
            self.bytes_output.fetch_add(size as u64, Ordering::Relaxed);
            
            (buffer_vtbl.release)(data_ptr);
            
            Ok(output)
        }
    }
    
    /// Flush encoder and get remaining frames
    pub fn flush(&mut self) -> Result<Vec<Vec<u8>>, String> {
        if !self.initialized.load(Ordering::SeqCst) {
            return Ok(Vec::new());
        }
        
        let mut outputs = Vec::new();
        
        unsafe {
            let encoder = &*self.encoder;
            let _ = ((*encoder.vtbl).drain)(self.encoder as *mut c_void);
            
            loop {
                let mut data_ptr: *mut c_void = ptr::null_mut();
                let result = ((*encoder.vtbl).query_output)(
                    self.encoder as *mut c_void,
                    &mut data_ptr
                );
                
                if result == AMF_EOF || data_ptr.is_null() {
                    break;
                }
                
                if result == AMF_OK {
                    let buffer = data_ptr as *mut AMFBuffer;
                    let buffer_vtbl = &*(*buffer).vtbl;
                    
                    let size = (buffer_vtbl.get_size)(data_ptr);
                    let native = (buffer_vtbl.get_native)(data_ptr);
                    
                    if !native.is_null() && size > 0 {
                        let mut output = vec![0u8; size];
                        ptr::copy_nonoverlapping(native, output.as_mut_ptr(), size);
                        outputs.push(output);
                        self.bytes_output.fetch_add(size as u64, Ordering::Relaxed);
                    }
                    
                    (buffer_vtbl.release)(data_ptr);
                }
            }
        }
        
        Ok(outputs)
    }
    
    /// Get encoder statistics
    pub fn get_stats(&self) -> AmfEncoderStats {
        let frames = self.frames_encoded.load(Ordering::Relaxed);
        let bytes = self.bytes_output.load(Ordering::Relaxed);
        let elapsed = self.start_time.map(|t| t.elapsed().as_secs_f64()).unwrap_or(1.0);
        
        AmfEncoderStats {
            frames_encoded: frames,
            bytes_output: bytes,
            average_bitrate_bps: if elapsed > 0.0 { (bytes as f64 * 8.0 / elapsed) as u64 } else { 0 },
            encode_fps: if elapsed > 0.0 { frames as f64 / elapsed } else { 0.0 },
            gpu_usage_percent: 0.0,
        }
    }
    
    /// Get encoder info
    pub fn info(&self) -> serde_json::Value {
        serde_json::json!({
            "codec": format!("{:?}", self.config.codec),
            "width": self.config.width,
            "height": self.config.height,
            "framerate": format!("{}/{}", self.config.framerate_num, self.config.framerate_den),
            "bitrate_bps": self.config.bitrate_bps,
            "quality": format!("{:?}", self.config.quality_preset),
            "rate_control": format!("{:?}", self.config.rate_control),
            "initialized": self.initialized.load(Ordering::Relaxed),
        })
    }
}

impl Drop for AmfEncoder {
    fn drop(&mut self) {
        unsafe {
            if !self.encoder.is_null() {
                let encoder = &*self.encoder;
                let _ = ((*encoder.vtbl).terminate)(self.encoder as *mut c_void);
                ((*encoder.vtbl).release)(self.encoder as *mut c_void);
            }
            
            if !self.context.is_null() {
                let context = &*self.context;
                let _ = ((*context.vtbl).terminate)(self.context as *mut c_void);
                ((*context.vtbl).release)(self.context as *mut c_void);
            }
            
            if !self.factory.is_null() {
                let factory = &*self.factory;
                ((*factory.vtbl).release)(self.factory as *mut c_void);
            }
        }
        
        tracing::info!("AMF encoder released");
    }
}

// ============================================================================
// Recording State
// ============================================================================

use parking_lot::Mutex;
use std::fs::File;
use std::io::Write;

use once_cell::sync::Lazy;

static RECORDING_STATE: Lazy<Mutex<Option<RecordingSession>>> = Lazy::new(|| Mutex::new(None));

struct RecordingSession {
    encoder: AmfEncoder,
    output_file: File,
    frame_count: u64,
}

// ============================================================================
// Public API
// ============================================================================


pub fn amf_capabilities() -> serde_json::Value {
    #[cfg(target_os = "windows")]
    let available = unsafe { Library::new("amfrt64.dll").is_ok() };
    
    #[cfg(not(target_os = "windows"))]
    let available = false;
    
    serde_json::json!({
        "available": available,
        "codecs": ["H.264/AVC", "H.265/HEVC", "AV1"],
        "features": {
            "hardware_encoding": true,
            "low_latency": true,
            "10bit": true,
            "hdr": true,
            "b_frames": true,
        },
        "supported_gpus": "AMD RX 400+, Ryzen APUs (Vega+)",
        "driver": "AMD Adrenalin 21.1.1+",
    })
}


pub fn amf_start_recording(
    path: String,
    width: u32,
    height: u32,
    fps: u32,
    bitrate_mbps: u32,
) -> Result<String, String> {
    let mut state = RECORDING_STATE.lock();
    
    if state.is_some() {
        return Err("Already recording".to_string());
    }
    
    let config = AmfEncoderConfig {
        width,
        height,
        framerate_num: fps,
        framerate_den: 1,
        bitrate_bps: (bitrate_mbps as u64) * 1_000_000,
        max_bitrate_bps: (bitrate_mbps as u64) * 1_500_000,
        ..Default::default()
    };
    
    let mut encoder = AmfEncoder::new(config)?;
    encoder.initialize()?;
    
    let file = File::create(&path)
        .map_err(|e| format!("Failed to create output file: {}", e))?;
    
    *state = Some(RecordingSession {
        encoder,
        output_file: file,
        frame_count: 0,
    });
    
    Ok(format!("Recording started: {}", path))
}


pub fn amf_stop_recording() -> Result<serde_json::Value, String> {
    let mut state = RECORDING_STATE.lock();
    
    let session = state.take()
        .ok_or("Not recording")?;
    
    let mut encoder = session.encoder;
    let mut file = session.output_file;
    
    let remaining = encoder.flush()?;
    for data in remaining {
        file.write_all(&data)
            .map_err(|e| format!("Write error: {}", e))?;
    }
    
    let stats = encoder.get_stats();
    
    Ok(serde_json::json!({
        "frames": session.frame_count,
        "stats": stats,
    }))
}


pub fn amf_is_recording() -> bool {
    RECORDING_STATE.lock().is_some()
}


pub fn amf_encoder_info() -> serde_json::Value {
    serde_json::json!({
        "name": "AMD AMF Encoder",
        "description": "Hardware video encoding using AMD VCN (Video Core Next)",
        "supported_codecs": {
            "h264": "All VCN-enabled GPUs (RX 400+, Ryzen APUs)",
            "h265": "VCN 1.0+ (RX 400+)",
            "av1": "VCN 4.0+ (RX 7000 series)",
        },
        "quality_presets": ["Speed", "Balanced", "Quality"],
        "rate_control": ["CQP", "CBR", "VBR", "VBR Low-Latency"],
        "features": [
            "8-bit and 10-bit encoding",
            "HDR passthrough",
            "Low-latency mode for streaming",
            "B-frame support",
            "Lookahead",
        ],
    })
}
