//! DirectShow COM interfaces
//!
//! Minimal interface definitions needed for DirectShow filter graph control.

use windows::core::GUID;
use windows::Win32::Foundation::BOOL;

// Note: Most DirectShow interfaces come from windows::Win32::Media::DirectShow

// ============================================================================
// GUIDs for DirectShow interfaces
// ============================================================================

/// IID_IGraphBuilder
pub const IID_IGRAPHBUILDER: GUID = GUID::from_u128(0x56a868a9_0ad4_11ce_b03a_0020af0ba770);

/// IID_IMediaControl
pub const IID_IMEDIACONTROL: GUID = GUID::from_u128(0x56a868b1_0ad4_11ce_b03a_0020af0ba770);

/// IID_IMediaSeeking
pub const IID_IMEDIASEEKING: GUID = GUID::from_u128(0x36b73880_c2c8_11cf_8b46_00805f6cef60);

/// IID_IMediaEvent
pub const IID_IMEDIAEVENT: GUID = GUID::from_u128(0x56a868b6_0ad4_11ce_b03a_0020af0ba770);

/// IID_IVideoWindow
pub const IID_IVIDEOWINDOW: GUID = GUID::from_u128(0x56a868b4_0ad4_11ce_b03a_0020af0ba770);

/// IID_IBasicVideo
pub const IID_IBASICVIDEO: GUID = GUID::from_u128(0x56a868b5_0ad4_11ce_b03a_0020af0ba770);

/// IID_IBasicAudio
pub const IID_IBASICAUDIO: GUID = GUID::from_u128(0x56a868b3_0ad4_11ce_b03a_0020af0ba770);

/// IID_IBaseFilter
pub const IID_IBASEFILTER: GUID = GUID::from_u128(0x56a86895_0ad4_11ce_b03a_0020af0ba770);

/// IID_IPin
pub const IID_IPIN: GUID = GUID::from_u128(0x56a86891_0ad4_11ce_b03a_0020af0ba770);

/// IID_IEnumPins
pub const IID_IENUMPINS: GUID = GUID::from_u128(0x56a86892_0ad4_11ce_b03a_0020af0ba770);

/// IID_IEnumFilters
pub const IID_IENUMFILTERS: GUID = GUID::from_u128(0x56a86893_0ad4_11ce_b03a_0020af0ba770);

/// IID_IFilterGraph
pub const IID_IFILTERGRAPH: GUID = GUID::from_u128(0x56a8689f_0ad4_11ce_b03a_0020af0ba770);

/// IID_ISampleGrabber
pub const IID_ISAMPLEGRABBER: GUID = GUID::from_u128(0x6b652fff_11fe_4fce_92ad_0266b5d7c78f);

/// IID_ISampleGrabberCB  
pub const IID_ISAMPLEGRABBERCB: GUID = GUID::from_u128(0x0579154a_2b53_4994_b0d0_e773148eff85);

/// IID_IFileSourceFilter
pub const IID_IFILESOURCEFILTER: GUID = GUID::from_u128(0x56a868a6_0ad4_11ce_b03a_0020af0ba770);

/// IID_IFileSinkFilter
pub const IID_IFILESINKFILTER: GUID = GUID::from_u128(0xa2104830_7c70_11cf_8bce_00aa00a3f1a6);

/// IID_IMediaFilter
pub const IID_IMEDIAFILTER: GUID = GUID::from_u128(0x56a86899_0ad4_11ce_b03a_0020af0ba770);

/// IID_IReferenceClock
pub const IID_IREFERENCECLOCK: GUID = GUID::from_u128(0x56a86897_0ad4_11ce_b03a_0020af0ba770);

// ============================================================================
// CLSIDs for DirectShow components
// ============================================================================

/// CLSID_FilterGraph
pub const CLSID_FILTERGRAPH: GUID = GUID::from_u128(0xe436ebb3_524f_11ce_9f53_0020af0ba770);

/// CLSID_FilterGraphNoThread
pub const CLSID_FILTERGRAPH_NOTHREAD: GUID =
    GUID::from_u128(0xe436ebb8_524f_11ce_9f53_0020af0ba770);

/// CLSID_SampleGrabber (from qedit.dll)
pub const CLSID_SAMPLEGRABBER: GUID = GUID::from_u128(0xc1f400a0_3f08_11d3_9f0b_006008039e37);

/// CLSID_NullRenderer
pub const CLSID_NULLRENDERER: GUID = GUID::from_u128(0xc1f400a4_3f08_11d3_9f0b_006008039e37);

// ============================================================================
// Media Types
// ============================================================================

/// MEDIATYPE_Video
pub const MEDIATYPE_VIDEO: GUID = GUID::from_u128(0x73646976_0000_0010_8000_00aa00389b71);

/// MEDIATYPE_Audio  
pub const MEDIATYPE_AUDIO: GUID = GUID::from_u128(0x73647561_0000_0010_8000_00aa00389b71);

/// MEDIASUBTYPE_RGB24
pub const MEDIASUBTYPE_RGB24: GUID = GUID::from_u128(0xe436eb7d_524f_11ce_9f53_0020af0ba770);

/// MEDIASUBTYPE_RGB32
pub const MEDIASUBTYPE_RGB32: GUID = GUID::from_u128(0xe436eb7e_524f_11ce_9f53_0020af0ba770);

/// MEDIASUBTYPE_ARGB32
pub const MEDIASUBTYPE_ARGB32: GUID = GUID::from_u128(0x773c9ac0_3274_11d0_b724_00aa006c1a01);

/// MEDIASUBTYPE_NV12
pub const MEDIASUBTYPE_NV12: GUID = GUID::from_u128(0x3231564e_0000_0010_8000_00aa00389b71);

/// MEDIASUBTYPE_YUY2
pub const MEDIASUBTYPE_YUY2: GUID = GUID::from_u128(0x32595559_0000_0010_8000_00aa00389b71);

/// MEDIASUBTYPE_I420
pub const MEDIASUBTYPE_I420: GUID = GUID::from_u128(0x30323449_0000_0010_8000_00aa00389b71);

/// MEDIASUBTYPE_IYUV
pub const MEDIASUBTYPE_IYUV: GUID = GUID::from_u128(0x56555949_0000_0010_8000_00aa00389b71);

/// FORMAT_VideoInfo
pub const FORMAT_VIDEOINFO: GUID = GUID::from_u128(0x05589f80_c356_11ce_bf01_00aa0055595a);

/// FORMAT_VideoInfo2
pub const FORMAT_VIDEOINFO2: GUID = GUID::from_u128(0xf72a76a0_eb0a_11d0_ace4_0000c0cc16ba);

/// FORMAT_WaveFormatEx
pub const FORMAT_WAVEFORMATEX: GUID = GUID::from_u128(0x05589f81_c356_11ce_bf01_00aa0055595a);

// ============================================================================
// Pin Direction
// ============================================================================

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinDirection {
    Input = 0,
    Output = 1,
}

// ============================================================================
// Filter State
// ============================================================================

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterState {
    Stopped = 0,
    Paused = 1,
    Running = 2,
}

// ============================================================================
// AM_MEDIA_TYPE structure
// ============================================================================

#[repr(C)]
#[derive(Clone)]
pub struct AmMediaType {
    pub major_type: GUID,
    pub sub_type: GUID,
    pub fixed_size_samples: BOOL,
    pub temporal_compression: BOOL,
    pub sample_size: u32,
    pub format_type: GUID,
    pub punk: *mut std::ffi::c_void, // IUnknown pointer
    pub cb_format: u32,
    pub pb_format: *mut u8,
}

impl Default for AmMediaType {
    fn default() -> Self {
        Self {
            major_type: GUID::zeroed(),
            sub_type: GUID::zeroed(),
            fixed_size_samples: BOOL(0),
            temporal_compression: BOOL(0),
            sample_size: 0,
            format_type: GUID::zeroed(),
            punk: std::ptr::null_mut(),
            cb_format: 0,
            pb_format: std::ptr::null_mut(),
        }
    }
}

// ============================================================================
// VIDEOINFOHEADER structure
// ============================================================================

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct BitmapInfoHeader {
    pub size: u32,
    pub width: i32,
    pub height: i32,
    pub planes: u16,
    pub bit_count: u16,
    pub compression: u32,
    pub size_image: u32,
    pub x_pels_per_meter: i32,
    pub y_pels_per_meter: i32,
    pub clr_used: u32,
    pub clr_important: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VideoInfoHeader {
    pub source: Rect,
    pub target: Rect,
    pub bit_rate: u32,
    pub bit_error_rate: u32,
    pub avg_time_per_frame: i64,
    pub bmi_header: BitmapInfoHeader,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct VideoInfoHeader2 {
    pub source: Rect,
    pub target: Rect,
    pub bit_rate: u32,
    pub bit_error_rate: u32,
    pub avg_time_per_frame: i64,
    pub interlace_flags: u32,
    pub copy_protect_flags: u32,
    pub pict_aspect_ratio_x: u32,
    pub pict_aspect_ratio_y: u32,
    pub control_flags: u32,
    pub reserved2: u32,
    pub bmi_header: BitmapInfoHeader,
}

// ============================================================================
// Reference time (100ns units)
// ============================================================================

pub type ReferenceTime = i64;

/// Convert milliseconds to reference time (100ns units)
pub fn ms_to_reference_time(ms: i64) -> ReferenceTime {
    ms * 10_000
}

/// Convert reference time to milliseconds
pub fn reference_time_to_ms(rt: ReferenceTime) -> i64 {
    rt / 10_000
}

// ============================================================================
// Seeking capabilities
// ============================================================================

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SeekingCapabilities: u32 {
        const CAN_SEEK_ABSOLUTE = 0x001;
        const CAN_SEEK_FORWARDS = 0x002;
        const CAN_SEEK_BACKWARDS = 0x004;
        const CAN_GET_CURRENT_POS = 0x008;
        const CAN_GET_STOP_POS = 0x010;
        const CAN_GET_DURATION = 0x020;
        const CAN_PLAY_BACKWARDS = 0x040;
        const CAN_DO_SEGMENTS = 0x080;
        const SOURCE = 0x100;
    }
}

// ============================================================================
// Event codes
// ============================================================================

#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCode {
    Complete = 0x01,
    UserAbort = 0x02,
    ErrorAbort = 0x03,
    Time = 0x04,
    Repaint = 0x05,
    StErrStopped = 0x06,
    StErrStPlaying = 0x07,
    Paused = 0x0E,
    End = 0x19,
    Unknown = 0xFF,
}

impl From<i32> for EventCode {
    fn from(value: i32) -> Self {
        match value {
            0x01 => Self::Complete,
            0x02 => Self::UserAbort,
            0x03 => Self::ErrorAbort,
            0x04 => Self::Time,
            0x05 => Self::Repaint,
            0x06 => Self::StErrStopped,
            0x07 => Self::StErrStPlaying,
            0x0E => Self::Paused,
            0x19 => Self::End,
            _ => Self::Unknown,
        }
    }
}
