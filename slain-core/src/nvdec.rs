// NVDEC - NVIDIA Hardware Video Decoder
//
// Full implementation using NVIDIA's Video Codec SDK via dynamic library loading.
// No compile-time CUDA dependency - loads nvcuda.dll/nvcuvid.dll at runtime.
//
// Pipeline:
// 1. Load CUDA and CUVID libraries dynamically
// 2. Create CUDA context on GPU 0
// 3. Create video parser (feeds NAL units)
// 4. Parser callbacks create decoder and handle frames
// 5. Map decoded frames from GPU memory to host
// 6. Return NV12/P016 frame data

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::ffi::c_void;
use std::ptr;
use std::sync::{Arc, Mutex, OnceLock};

// ============================================================================
// CUDA Types (from cuda.h)
// ============================================================================

type CUresult = i32;
type CUdevice = i32;
type CUcontext = *mut c_void;
type CUstream = *mut c_void;
type CUdeviceptr = u64;

const CUDA_SUCCESS: CUresult = 0;

// ============================================================================
// CUVID Types (from cuviddec.h / nvcuvid.h)
// ============================================================================

type CUvideoparser = *mut c_void;
type CUvideodecoder = *mut c_void;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CudaVideoCodec {
    MPEG1 = 0,
    MPEG2 = 1,
    MPEG4 = 2,
    VC1 = 3,
    H264 = 4,
    JPEG = 5,
    H264SVC = 6,
    H264MVC = 7,
    HEVC = 8,
    VP8 = 9,
    VP9 = 10,
    AV1 = 11,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum CudaVideoChromaFormat {
    Monochrome = 0,
    YUV420 = 1,
    YUV422 = 2,
    YUV444 = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum CudaVideoSurfaceFormat {
    NV12 = 0,
    P016 = 1,
    YUV444 = 2,
    YUV444P16 = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum CudaVideoDeinterlaceMode {
    Weave = 0,
    Bob = 1,
    Adaptive = 2,
}

// CUVIDEOFORMAT - Video format info from parser
#[repr(C)]
#[derive(Debug, Clone)]
pub struct CuvidVideoFormat {
    pub codec: CudaVideoCodec,
    pub frame_rate: CuvidFraction,
    pub progressive_sequence: u8,
    pub bit_depth_luma_minus8: u8,
    pub bit_depth_chroma_minus8: u8,
    pub min_num_decode_surfaces: u8,
    pub coded_width: u32,
    pub coded_height: u32,
    pub display_area: CuvidRect,
    pub chroma_format: CudaVideoChromaFormat,
    pub bitrate: u32,
    pub display_aspect_ratio: CuvidFraction,
    pub video_signal_description: CuvidVideoSignalDescription,
    pub seqhdr_data_length: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CuvidFraction {
    pub numerator: u32,
    pub denominator: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CuvidRect {
    pub left: i16,
    pub top: i16,
    pub right: i16,
    pub bottom: i16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct CuvidVideoSignalDescription {
    pub video_format: u8,
    pub video_full_range_flag: u8,
    pub reserved_zero_bits: u8,
    pub color_primaries: u8,
    pub transfer_characteristics: u8,
    pub matrix_coefficients: u8,
}

// CUVIDPICPARAMS - Picture parameters for decoding
#[repr(C)]
#[allow(non_snake_case)]
pub struct CuvidPicParams {
    pub pic_width_in_mbs: i32,
    pub pic_height_in_mbs: i32,
    pub curr_pic_idx: i32,
    pub field_pic_flag: i32,
    pub bottom_field_flag: i32,
    pub second_field: i32,
    pub nbitstreamdatabytes: u32,
    pub pbitstreamdata: *const u8,
    pub nNumSlices: u32,
    pub pSliceDataOffsets: *const u32,
    pub ref_pic_flag: i32,
    pub intra_pic_flag: i32,
    pub reserved: [u32; 30],
    pub codec_specific: [u8; 1024],
}

// CUVIDPARSERDISPINFO - Display info from parser
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CuvidDispInfo {
    pub picture_index: i32,
    pub progressive_frame: i32,
    pub top_field_first: i32,
    pub repeat_first_field: i32,
    pub timestamp: i64,
}

// CUVIDDECODECREATEINFO - Decoder creation parameters
#[repr(C)]
#[allow(non_snake_case)]
pub struct CuvidDecodeCreateInfo {
    pub ulWidth: u32,
    pub ulHeight: u32,
    pub ulNumDecodeSurfaces: u32,
    pub CodecType: CudaVideoCodec,
    pub ChromaFormat: CudaVideoChromaFormat,
    pub ulCreationFlags: u32,
    pub bitDepthMinus8: u32,
    pub ulIntraDecodeOnly: u32,
    pub ulMaxWidth: u32,
    pub ulMaxHeight: u32,
    pub Reserved1: u32,
    pub display_area: CuvidRect,
    pub OutputFormat: CudaVideoSurfaceFormat,
    pub DeinterlaceMode: CudaVideoDeinterlaceMode,
    pub ulTargetWidth: u32,
    pub ulTargetHeight: u32,
    pub ulNumOutputSurfaces: u32,
    pub vidLock: *mut c_void,
    pub target_rect: CuvidRect,
    pub Reserved2: [u32; 5],
}

// CUVIDPARSERPARAMS - Parser creation parameters
#[repr(C)]
#[allow(non_snake_case)]
pub struct CuvidParserParams {
    pub CodecType: CudaVideoCodec,
    pub ulMaxNumDecodeSurfaces: u32,
    pub ulClockRate: u32,
    pub ulErrorThreshold: u32,
    pub ulMaxDisplayDelay: u32,
    pub uReserved1: [u32; 5],
    pub pUserData: *mut c_void,
    pub pfnSequenceCallback: Option<extern "C" fn(*mut c_void, *mut CuvidVideoFormat) -> i32>,
    pub pfnDecodePicture: Option<extern "C" fn(*mut c_void, *mut CuvidPicParams) -> i32>,
    pub pfnDisplayPicture: Option<extern "C" fn(*mut c_void, *mut CuvidDispInfo) -> i32>,
    pub pExtVideoInfo: *mut c_void,
}

// CUVIDSOURCEDATAPACKET - Packet for parsing
#[repr(C)]
pub struct CuvidSourceDataPacket {
    pub flags: u32,
    pub payload_size: u32,
    pub payload: *const u8,
    pub timestamp: i64,
}

const CUVID_PKT_ENDOFSTREAM: u32 = 0x01;
const CUVID_PKT_TIMESTAMP: u32 = 0x02;

// CUVIDPROCPARAMS - Frame mapping parameters
#[repr(C)]
#[allow(non_snake_case)]
pub struct CuvidProcParams {
    pub progressive_frame: i32,
    pub second_field: i32,
    pub top_field_first: i32,
    pub unpaired_field: i32,
    pub reserved_flags: u32,
    pub reserved_zero: u32,
    pub raw_input_dptr: u64,
    pub raw_input_pitch: u32,
    pub raw_input_format: u32,
    pub raw_output_dptr: u64,
    pub raw_output_pitch: u32,
    pub Reserved1: u32,
    pub output_stream: CUstream,
    pub Reserved: [u32; 46],
}

// ============================================================================
// Library Path Detection
// ============================================================================

#[cfg(target_os = "windows")]
fn get_cuda_lib_path() -> String {
    "nvcuda.dll".to_string()
}

#[cfg(target_os = "windows")]
fn get_cuvid_lib_path() -> String {
    "nvcuvid.dll".to_string()
}

#[cfg(target_os = "linux")]
fn get_cuda_lib_path() -> String {
    for path in &[
        "/usr/lib/x86_64-linux-gnu/libcuda.so.1",
        "/usr/lib/libcuda.so.1",
        "/usr/local/cuda/lib64/libcuda.so.1",
        "libcuda.so.1",
    ] {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    "libcuda.so.1".to_string()
}

#[cfg(target_os = "linux")]
fn get_cuvid_lib_path() -> String {
    for path in &[
        "/usr/lib/x86_64-linux-gnu/libnvcuvid.so.1",
        "/usr/lib/libnvcuvid.so.1",
        "/usr/local/cuda/lib64/libnvcuvid.so.1",
        "libnvcuvid.so.1",
    ] {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    "libnvcuvid.so.1".to_string()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn get_cuda_lib_path() -> String {
    String::new()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn get_cuvid_lib_path() -> String {
    String::new()
}

// ============================================================================
// Function Types
// ============================================================================

type CuInitFn = unsafe extern "C" fn(u32) -> CUresult;
type CuDeviceGetFn = unsafe extern "C" fn(*mut CUdevice, i32) -> CUresult;
type CuDeviceGetCountFn = unsafe extern "C" fn(*mut i32) -> CUresult;
type CuDeviceGetNameFn = unsafe extern "C" fn(*mut u8, i32, CUdevice) -> CUresult;
type CuDeviceGetAttributeFn = unsafe extern "C" fn(*mut i32, i32, CUdevice) -> CUresult;
type CuCtxCreateFn = unsafe extern "C" fn(*mut CUcontext, u32, CUdevice) -> CUresult;
type CuCtxDestroyFn = unsafe extern "C" fn(CUcontext) -> CUresult;
type CuCtxPushCurrentFn = unsafe extern "C" fn(CUcontext) -> CUresult;
type CuCtxPopCurrentFn = unsafe extern "C" fn(*mut CUcontext) -> CUresult;
type CuMemcpyDtoHFn = unsafe extern "C" fn(*mut c_void, CUdeviceptr, usize) -> CUresult;

type CuvidCreateVideoParserFn =
    unsafe extern "C" fn(*mut CUvideoparser, *mut CuvidParserParams) -> CUresult;
type CuvidDestroyVideoParserFn = unsafe extern "C" fn(CUvideoparser) -> CUresult;
type CuvidParseVideoDataFn =
    unsafe extern "C" fn(CUvideoparser, *mut CuvidSourceDataPacket) -> CUresult;
type CuvidCreateDecoderFn =
    unsafe extern "C" fn(*mut CUvideodecoder, *mut CuvidDecodeCreateInfo) -> CUresult;
type CuvidDestroyDecoderFn = unsafe extern "C" fn(CUvideodecoder) -> CUresult;
type CuvidDecodePictureFn = unsafe extern "C" fn(CUvideodecoder, *mut CuvidPicParams) -> CUresult;
type CuvidMapVideoFrameFn = unsafe extern "C" fn(
    CUvideodecoder,
    i32,
    *mut CUdeviceptr,
    *mut u32,
    *mut CuvidProcParams,
) -> CUresult;
type CuvidUnmapVideoFrameFn = unsafe extern "C" fn(CUvideodecoder, CUdeviceptr) -> CUresult;

// ============================================================================
// Loaded Functions Container
// ============================================================================

struct NvdecLibraries {
    _cuda_lib: libloading::Library,
    _cuvid_lib: libloading::Library,

    cu_init: CuInitFn,
    cu_device_get: CuDeviceGetFn,
    cu_device_get_count: CuDeviceGetCountFn,
    cu_device_get_name: CuDeviceGetNameFn,
    cu_device_get_attribute: CuDeviceGetAttributeFn,
    cu_ctx_create: CuCtxCreateFn,
    cu_ctx_destroy: CuCtxDestroyFn,
    cu_ctx_push_current: CuCtxPushCurrentFn,
    cu_ctx_pop_current: CuCtxPopCurrentFn,
    cu_memcpy_dtoh: CuMemcpyDtoHFn,

    cuvid_create_video_parser: CuvidCreateVideoParserFn,
    cuvid_destroy_video_parser: CuvidDestroyVideoParserFn,
    cuvid_parse_video_data: CuvidParseVideoDataFn,
    cuvid_create_decoder: CuvidCreateDecoderFn,
    cuvid_destroy_decoder: CuvidDestroyDecoderFn,
    cuvid_decode_picture: CuvidDecodePictureFn,
    cuvid_map_video_frame: CuvidMapVideoFrameFn,
    cuvid_unmap_video_frame: CuvidUnmapVideoFrameFn,
}

unsafe impl Send for NvdecLibraries {}
unsafe impl Sync for NvdecLibraries {}

static NVDEC_LIBS: OnceLock<Option<NvdecLibraries>> = OnceLock::new();

fn load_nvdec_libraries() -> Option<&'static NvdecLibraries> {
    NVDEC_LIBS
        .get_or_init(|| {
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                unsafe {
                    let cuda_path = get_cuda_lib_path();
                    let cuvid_path = get_cuvid_lib_path();

                    let cuda_lib = match libloading::Library::new(&cuda_path) {
                        Ok(lib) => lib,
                        Err(e) => {
                            tracing::warn!("Failed to load CUDA library {}: {}", cuda_path, e);
                            return None;
                        }
                    };

                    let cuvid_lib = match libloading::Library::new(&cuvid_path) {
                        Ok(lib) => lib,
                        Err(e) => {
                            tracing::warn!("Failed to load CUVID library {}: {}", cuvid_path, e);
                            return None;
                        }
                    };

                    // Load CUDA functions
                    let cu_init: CuInitFn = *cuda_lib.get(b"cuInit\0").ok()?;
                    let cu_device_get: CuDeviceGetFn = *cuda_lib.get(b"cuDeviceGet\0").ok()?;
                    let cu_device_get_count: CuDeviceGetCountFn =
                        *cuda_lib.get(b"cuDeviceGetCount\0").ok()?;
                    let cu_device_get_name: CuDeviceGetNameFn =
                        *cuda_lib.get(b"cuDeviceGetName\0").ok()?;
                    let cu_device_get_attribute: CuDeviceGetAttributeFn =
                        *cuda_lib.get(b"cuDeviceGetAttribute\0").ok()?;
                    let cu_ctx_create: CuCtxCreateFn = *cuda_lib.get(b"cuCtxCreate_v2\0").ok()?;
                    let cu_ctx_destroy: CuCtxDestroyFn =
                        *cuda_lib.get(b"cuCtxDestroy_v2\0").ok()?;
                    let cu_ctx_push_current: CuCtxPushCurrentFn =
                        *cuda_lib.get(b"cuCtxPushCurrent_v2\0").ok()?;
                    let cu_ctx_pop_current: CuCtxPopCurrentFn =
                        *cuda_lib.get(b"cuCtxPopCurrent_v2\0").ok()?;
                    let cu_memcpy_dtoh: CuMemcpyDtoHFn =
                        *cuda_lib.get(b"cuMemcpyDtoH_v2\0").ok()?;

                    // Load CUVID functions
                    let cuvid_create_video_parser: CuvidCreateVideoParserFn =
                        *cuvid_lib.get(b"cuvidCreateVideoParser\0").ok()?;
                    let cuvid_destroy_video_parser: CuvidDestroyVideoParserFn =
                        *cuvid_lib.get(b"cuvidDestroyVideoParser\0").ok()?;
                    let cuvid_parse_video_data: CuvidParseVideoDataFn =
                        *cuvid_lib.get(b"cuvidParseVideoData\0").ok()?;
                    let cuvid_create_decoder: CuvidCreateDecoderFn =
                        *cuvid_lib.get(b"cuvidCreateDecoder\0").ok()?;
                    let cuvid_destroy_decoder: CuvidDestroyDecoderFn =
                        *cuvid_lib.get(b"cuvidDestroyDecoder\0").ok()?;
                    let cuvid_decode_picture: CuvidDecodePictureFn =
                        *cuvid_lib.get(b"cuvidDecodePicture\0").ok()?;
                    let cuvid_map_video_frame: CuvidMapVideoFrameFn = *cuvid_lib
                        .get(b"cuvidMapVideoFrame64\0")
                        .or_else(|_| cuvid_lib.get(b"cuvidMapVideoFrame\0"))
                        .ok()?;
                    let cuvid_unmap_video_frame: CuvidUnmapVideoFrameFn = *cuvid_lib
                        .get(b"cuvidUnmapVideoFrame64\0")
                        .or_else(|_| cuvid_lib.get(b"cuvidUnmapVideoFrame\0"))
                        .ok()?;

                    // Initialize CUDA
                    let result = cu_init(0);
                    if result != CUDA_SUCCESS {
                        tracing::warn!("cuInit failed with error {}", result);
                        return None;
                    }

                    tracing::info!("NVDEC libraries loaded successfully");

                    Some(NvdecLibraries {
                        _cuda_lib: cuda_lib,
                        _cuvid_lib: cuvid_lib,
                        cu_init,
                        cu_device_get,
                        cu_device_get_count,
                        cu_device_get_name,
                        cu_device_get_attribute,
                        cu_ctx_create,
                        cu_ctx_destroy,
                        cu_ctx_push_current,
                        cu_ctx_pop_current,
                        cu_memcpy_dtoh,
                        cuvid_create_video_parser,
                        cuvid_destroy_video_parser,
                        cuvid_parse_video_data,
                        cuvid_create_decoder,
                        cuvid_destroy_decoder,
                        cuvid_decode_picture,
                        cuvid_map_video_frame,
                        cuvid_unmap_video_frame,
                    })
                }
            }

            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            {
                None
            }
        })
        .as_ref()
}

// ============================================================================
// Public Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NvdecCapabilities {
    pub available: bool,
    pub device_name: String,
    pub compute_capability: (i32, i32),
    pub supported_codecs: Vec<String>,
    pub max_width: u32,
    pub max_height: u32,
    pub cuda_version: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    H265,
    VP8,
    VP9,
    AV1,
    MPEG2,
}

impl VideoCodec {
    pub fn to_cuvid(&self) -> CudaVideoCodec {
        match self {
            Self::H264 => CudaVideoCodec::H264,
            Self::H265 => CudaVideoCodec::HEVC,
            Self::VP8 => CudaVideoCodec::VP8,
            Self::VP9 => CudaVideoCodec::VP9,
            Self::AV1 => CudaVideoCodec::AV1,
            Self::MPEG2 => CudaVideoCodec::MPEG2,
        }
    }

    pub fn from_fourcc(fourcc: &[u8; 4]) -> Option<Self> {
        match fourcc {
            b"avc1" | b"h264" | b"H264" => Some(Self::H264),
            b"hvc1" | b"hev1" | b"hevc" => Some(Self::H265),
            b"vp08" | b"VP8 " => Some(Self::VP8),
            b"vp09" | b"VP9 " => Some(Self::VP9),
            b"av01" | b"AV1 " => Some(Self::AV1),
            b"mpg2" | b"mp2v" => Some(Self::MPEG2),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodedFrame {
    pub pts: i64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: FrameFormat,
    pub data: Vec<u8>,
    pub progressive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameFormat {
    NV12,
    P016,
}

// ============================================================================
// Decoder State (for callbacks)
// ============================================================================

struct DecoderState {
    libs: &'static NvdecLibraries,
    ctx: CUcontext,
    decoder: CUvideodecoder,
    width: u32,
    height: u32,
    bit_depth: u8,
    output_format: CudaVideoSurfaceFormat,
    pending_frames: VecDeque<PendingFrame>,
}

struct PendingFrame {
    picture_index: i32,
    timestamp: i64,
    progressive: bool,
}

// ============================================================================
// NVDEC Decoder
// ============================================================================

pub struct NvdecDecoder {
    libs: &'static NvdecLibraries,
    // Raw pointer to heap-allocated state - stable for C callbacks
    state_ptr: *mut DecoderState,
    parser: CUvideoparser,
    codec: VideoCodec,
}

// Parser callback: Sequence change (create/recreate decoder)
extern "C" fn sequence_callback(user_data: *mut c_void, format: *mut CuvidVideoFormat) -> i32 {
    if user_data.is_null() || format.is_null() {
        tracing::error!("NVDEC sequence_callback: null pointer");
        return 0;
    }

    unsafe {
        let state = &mut *(user_data as *mut DecoderState);
        let fmt = &*format;

        tracing::info!(
            "NVDEC sequence_callback: {}x{}, codec {:?}, surfaces {}",
            fmt.coded_width,
            fmt.coded_height,
            fmt.codec,
            fmt.min_num_decode_surfaces
        );

        // Push CUDA context for this thread
        (state.libs.cu_ctx_push_current)(state.ctx);

        // Destroy existing decoder
        if !state.decoder.is_null() {
            (state.libs.cuvid_destroy_decoder)(state.decoder);
            state.decoder = ptr::null_mut();
        }

        state.width = fmt.coded_width;
        state.height = fmt.coded_height;
        state.bit_depth = fmt.bit_depth_luma_minus8 + 8;
        state.output_format = if state.bit_depth > 8 {
            CudaVideoSurfaceFormat::P016
        } else {
            CudaVideoSurfaceFormat::NV12
        };

        // Create decoder
        let mut create_info = CuvidDecodeCreateInfo {
            ulWidth: fmt.coded_width,
            ulHeight: fmt.coded_height,
            ulNumDecodeSurfaces: std::cmp::max(fmt.min_num_decode_surfaces as u32, 20),
            CodecType: fmt.codec,
            ChromaFormat: fmt.chroma_format,
            ulCreationFlags: 0,
            bitDepthMinus8: fmt.bit_depth_luma_minus8 as u32,
            ulIntraDecodeOnly: 0,
            ulMaxWidth: fmt.coded_width,
            ulMaxHeight: fmt.coded_height,
            Reserved1: 0,
            display_area: CuvidRect {
                left: fmt.display_area.left,
                top: fmt.display_area.top,
                right: if fmt.display_area.right > 0 {
                    fmt.display_area.right
                } else {
                    fmt.coded_width as i16
                },
                bottom: if fmt.display_area.bottom > 0 {
                    fmt.display_area.bottom
                } else {
                    fmt.coded_height as i16
                },
            },
            OutputFormat: state.output_format,
            DeinterlaceMode: CudaVideoDeinterlaceMode::Adaptive,
            ulTargetWidth: fmt.coded_width,
            ulTargetHeight: fmt.coded_height,
            ulNumOutputSurfaces: 4,
            vidLock: ptr::null_mut(),
            target_rect: CuvidRect::default(),
            Reserved2: [0; 5],
        };

        let result = (state.libs.cuvid_create_decoder)(&mut state.decoder, &mut create_info);

        // Pop context
        let mut old_ctx: CUcontext = ptr::null_mut();
        (state.libs.cu_ctx_pop_current)(&mut old_ctx);

        if result != CUDA_SUCCESS {
            tracing::error!("cuvidCreateDecoder failed: {}", result);
            return 0;
        }

        tracing::info!("NVDEC decoder created successfully");
        fmt.min_num_decode_surfaces as i32
    }
}

// Parser callback: Decode picture
extern "C" fn decode_callback(user_data: *mut c_void, pic_params: *mut CuvidPicParams) -> i32 {
    if user_data.is_null() || pic_params.is_null() {
        tracing::error!("NVDEC decode_callback: null pointer");
        return 0;
    }

    unsafe {
        let state = &mut *(user_data as *mut DecoderState);

        if state.decoder.is_null() {
            tracing::error!("NVDEC decode_callback: decoder is null");
            return 0;
        }

        // Push context
        (state.libs.cu_ctx_push_current)(state.ctx);

        let result = (state.libs.cuvid_decode_picture)(state.decoder, pic_params);

        // Pop context
        let mut old_ctx: CUcontext = ptr::null_mut();
        (state.libs.cu_ctx_pop_current)(&mut old_ctx);

        if result != CUDA_SUCCESS {
            tracing::error!("cuvidDecodePicture failed: {}", result);
            return 0;
        }

        1
    }
}

// Parser callback: Display picture (queue for mapping)
extern "C" fn display_callback(user_data: *mut c_void, disp_info: *mut CuvidDispInfo) -> i32 {
    if user_data.is_null() {
        tracing::error!("NVDEC display_callback: null user_data");
        return 0;
    }

    // disp_info can be null to signal end of stream
    if disp_info.is_null() {
        return 1;
    }

    unsafe {
        let state = &mut *(user_data as *mut DecoderState);
        let info = &*disp_info;

        if state.decoder.is_null() {
            tracing::error!("NVDEC display_callback: decoder is null");
            return 0;
        }

        state.pending_frames.push_back(PendingFrame {
            picture_index: info.picture_index,
            timestamp: info.timestamp,
            progressive: info.progressive_frame != 0,
        });

        1
    }
}

/// Check if NVDEC is available
pub fn nvdec_available() -> bool {
    load_nvdec_libraries().is_some()
}

/// Get NVDEC capabilities
pub fn nvdec_capabilities() -> NvdecCapabilities {
    let libs = match load_nvdec_libraries() {
        Some(l) => l,
        None => {
            return NvdecCapabilities {
                available: false,
                device_name: String::new(),
                compute_capability: (0, 0),
                supported_codecs: Vec::new(),
                max_width: 0,
                max_height: 0,
                cuda_version: 0,
            }
        }
    };

    unsafe {
        let mut device_count = 0;
        (libs.cu_device_get_count)(&mut device_count);

        if device_count == 0 {
            return NvdecCapabilities {
                available: false,
                device_name: "No NVIDIA GPU".to_string(),
                compute_capability: (0, 0),
                supported_codecs: Vec::new(),
                max_width: 0,
                max_height: 0,
                cuda_version: 0,
            };
        }

        let mut device: CUdevice = 0;
        (libs.cu_device_get)(&mut device, 0);

        let mut name_buf = [0u8; 256];
        (libs.cu_device_get_name)(name_buf.as_mut_ptr(), 256, device);
        let device_name = String::from_utf8_lossy(
            &name_buf[..name_buf.iter().position(|&x| x == 0).unwrap_or(256)],
        )
        .to_string();

        let mut major = 0;
        let mut minor = 0;
        (libs.cu_device_get_attribute)(&mut major, 75, device);
        (libs.cu_device_get_attribute)(&mut minor, 76, device);

        // Codecs based on compute capability
        let mut codecs = vec!["H.264".to_string(), "MPEG-2".to_string()];
        if major >= 5 {
            codecs.push("H.265".to_string());
        }
        if major >= 6 {
            codecs.push("VP9".to_string());
        }
        if major >= 8 {
            codecs.push("AV1".to_string());
        }

        NvdecCapabilities {
            available: true,
            device_name,
            compute_capability: (major, minor),
            supported_codecs: codecs,
            max_width: 8192,
            max_height: 8192,
            cuda_version: major * 10 + minor,
        }
    }
}

impl NvdecDecoder {
    /// Create new NVDEC decoder
    pub fn new(codec: VideoCodec, width: u32, height: u32) -> Result<Self, String> {
        let libs = load_nvdec_libraries().ok_or_else(|| "NVDEC not available".to_string())?;

        unsafe {
            let mut device: CUdevice = 0;
            let result = (libs.cu_device_get)(&mut device, 0);
            if result != CUDA_SUCCESS {
                return Err(format!("cuDeviceGet failed: {}", result));
            }

            let mut ctx: CUcontext = ptr::null_mut();
            let result = (libs.cu_ctx_create)(&mut ctx, 0, device);
            if result != CUDA_SUCCESS {
                return Err(format!("cuCtxCreate failed: {}", result));
            }

            // Allocate state on heap with stable address for C callbacks
            let state = Box::new(DecoderState {
                libs,
                ctx,
                decoder: ptr::null_mut(),
                width,
                height,
                bit_depth: 8,
                output_format: CudaVideoSurfaceFormat::NV12,
                pending_frames: VecDeque::new(),
            });

            // Convert to raw pointer - this is stable and won't move
            let state_ptr = Box::into_raw(state);

            let mut parser_params = CuvidParserParams {
                CodecType: codec.to_cuvid(),
                ulMaxNumDecodeSurfaces: 20,
                ulClockRate: 0,
                ulErrorThreshold: 100, // Allow some errors
                ulMaxDisplayDelay: 4,
                uReserved1: [0; 5],
                pUserData: state_ptr as *mut c_void,
                pfnSequenceCallback: Some(sequence_callback),
                pfnDecodePicture: Some(decode_callback),
                pfnDisplayPicture: Some(display_callback),
                pExtVideoInfo: ptr::null_mut(),
            };

            let mut parser: CUvideoparser = ptr::null_mut();
            let result = (libs.cuvid_create_video_parser)(&mut parser, &mut parser_params);
            if result != CUDA_SUCCESS {
                // Clean up on failure
                let _ = Box::from_raw(state_ptr);
                (libs.cu_ctx_destroy)(ctx);
                return Err(format!("cuvidCreateVideoParser failed: {}", result));
            }

            tracing::info!("NVDEC decoder created for {:?} {}x{}", codec, width, height);

            Ok(Self {
                libs,
                state_ptr,
                parser,
                codec,
            })
        }
    }

    /// Decode a packet
    pub fn decode(&mut self, data: &[u8], pts: i64) -> Result<Option<DecodedFrame>, String> {
        if self.parser.is_null() || self.state_ptr.is_null() {
            return Err("Parser not initialized".to_string());
        }

        unsafe {
            let state = &*self.state_ptr;
            (self.libs.cu_ctx_push_current)(state.ctx);

            let mut packet = CuvidSourceDataPacket {
                flags: CUVID_PKT_TIMESTAMP,
                payload_size: data.len() as u32,
                payload: data.as_ptr(),
                timestamp: pts,
            };

            let result = (self.libs.cuvid_parse_video_data)(self.parser, &mut packet);

            let mut old_ctx: CUcontext = ptr::null_mut();
            (self.libs.cu_ctx_pop_current)(&mut old_ctx);

            if result != CUDA_SUCCESS {
                return Err(format!("cuvidParseVideoData failed: {}", result));
            }

            self.map_pending_frame()
        }
    }

    fn map_pending_frame(&self) -> Result<Option<DecodedFrame>, String> {
        if self.state_ptr.is_null() {
            return Ok(None);
        }

        unsafe {
            let state = &mut *self.state_ptr;

            if state.pending_frames.is_empty() || state.decoder.is_null() {
                return Ok(None);
            }

            let pending = state.pending_frames.pop_front().unwrap();

            (self.libs.cu_ctx_push_current)(state.ctx);

            let mut device_ptr: CUdeviceptr = 0;
            let mut pitch: u32 = 0;

            let mut proc_params = CuvidProcParams {
                progressive_frame: if pending.progressive { 1 } else { 0 },
                second_field: 0,
                top_field_first: 1,
                unpaired_field: 0,
                reserved_flags: 0,
                reserved_zero: 0,
                raw_input_dptr: 0,
                raw_input_pitch: 0,
                raw_input_format: 0,
                raw_output_dptr: 0,
                raw_output_pitch: 0,
                Reserved1: 0,
                output_stream: ptr::null_mut(),
                Reserved: [0; 46],
            };

            let result = (self.libs.cuvid_map_video_frame)(
                state.decoder,
                pending.picture_index,
                &mut device_ptr,
                &mut pitch,
                &mut proc_params,
            );

            if result != CUDA_SUCCESS {
                (self.libs.cu_ctx_pop_current)(&mut ptr::null_mut());
                return Err(format!("cuvidMapVideoFrame failed: {}", result));
            }

            // NV12: Y plane + UV plane
            let y_size = pitch as usize * state.height as usize;
            let uv_size = pitch as usize * (state.height as usize / 2);
            let total_size = y_size + uv_size;

            let mut frame_data = vec![0u8; total_size];
            let result = (self.libs.cu_memcpy_dtoh)(
                frame_data.as_mut_ptr() as *mut c_void,
                device_ptr,
                total_size,
            );

            (self.libs.cuvid_unmap_video_frame)(state.decoder, device_ptr);
            (self.libs.cu_ctx_pop_current)(&mut ptr::null_mut());

            if result != CUDA_SUCCESS {
                return Err(format!("cuMemcpyDtoH failed: {}", result));
            }

            let format = if state.bit_depth > 8 {
                FrameFormat::P016
            } else {
                FrameFormat::NV12
            };

            Ok(Some(DecodedFrame {
                pts: pending.timestamp,
                width: state.width,
                height: state.height,
                pitch,
                format,
                data: frame_data,
                progressive: pending.progressive,
            }))
        }
    }

    /// Flush decoder
    pub fn flush(&mut self) -> Vec<DecodedFrame> {
        if self.parser.is_null() || self.state_ptr.is_null() {
            return Vec::new();
        }

        unsafe {
            let state = &*self.state_ptr;
            (self.libs.cu_ctx_push_current)(state.ctx);

            let mut packet = CuvidSourceDataPacket {
                flags: CUVID_PKT_ENDOFSTREAM,
                payload_size: 0,
                payload: ptr::null(),
                timestamp: 0,
            };

            let _ = (self.libs.cuvid_parse_video_data)(self.parser, &mut packet);
            (self.libs.cu_ctx_pop_current)(&mut ptr::null_mut());
        }

        let mut frames = Vec::new();
        while let Ok(Some(frame)) = self.map_pending_frame() {
            frames.push(frame);
        }
        frames
    }

    /// Get decoder info
    pub fn info(&self) -> serde_json::Value {
        if self.state_ptr.is_null() {
            return serde_json::json!({ "backend": "nvdec", "error": "no state" });
        }
        unsafe {
            let state = &*self.state_ptr;
            serde_json::json!({
                "backend": "nvdec",
                "codec": format!("{:?}", self.codec),
                "width": state.width,
                "height": state.height,
                "bit_depth": state.bit_depth,
                "output_format": if state.bit_depth > 8 { "P016" } else { "NV12" },
            })
        }
    }
}

impl Drop for NvdecDecoder {
    fn drop(&mut self) {
        unsafe {
            if !self.parser.is_null() {
                (self.libs.cuvid_destroy_video_parser)(self.parser);
                self.parser = ptr::null_mut();
            }

            if !self.state_ptr.is_null() {
                let state = &mut *self.state_ptr;

                if !state.decoder.is_null() {
                    (self.libs.cuvid_destroy_decoder)(state.decoder);
                }

                if !state.ctx.is_null() {
                    (self.libs.cu_ctx_destroy)(state.ctx);
                }

                // Reclaim the Box to free memory
                let _ = Box::from_raw(self.state_ptr);
                self.state_ptr = ptr::null_mut();
            }
        }
    }
}

unsafe impl Send for NvdecDecoder {}
unsafe impl Sync for NvdecDecoder {}

// ============================================================================
// Public Rust API
// ============================================================================

pub fn nvdec_check_available() -> bool {
    nvdec_available()
}

pub fn nvdec_get_capabilities() -> serde_json::Value {
    serde_json::to_value(nvdec_capabilities()).unwrap_or_default()
}

pub fn nvdec_description() -> String {
    r#"
NVDEC - NVIDIA Hardware Video Decoder (IMPLEMENTED)

Loads nvcuda.dll + nvcuvid.dll at runtime via libloading.
No compile-time CUDA SDK dependency required.

SUPPORTED CODECS:
• H.264/AVC - All NVIDIA GPUs (2012+)
• H.265/HEVC - Maxwell 2+ (GTX 950+)
• VP9 - Pascal+ (GTX 1000+)
• AV1 - Ampere+ (RTX 3000+)

PIPELINE:
1. Load CUDA/CUVID libraries dynamically
2. Create CUDA context on GPU
3. Create video parser for codec
4. Parser callbacks handle decode
5. Map frames from GPU to host RAM
6. Return NV12/P016 data
"#
    .to_string()
}
