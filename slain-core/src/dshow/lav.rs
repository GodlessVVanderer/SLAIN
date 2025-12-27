//! LAV Filters COM integration
//!
//! CLSIDs and interfaces for LAV Splitter and LAV Video Decoder.

use std::path::Path;
use windows::core::GUID;

// ============================================================================
// LAV Filters CLSIDs
// ============================================================================

/// CLSID for LAV Splitter Source (file reader + demuxer)
pub const CLSID_LAV_SPLITTER_SOURCE: GUID = GUID::from_u128(0xb98d13e7_55db_4385_a33d_09fd1ba26338);

/// CLSID for LAV Splitter (demuxer only, needs source)
pub const CLSID_LAV_SPLITTER: GUID = GUID::from_u128(0x171252a0_8820_4afe_9df8_5c92b2d66b04);

/// CLSID for LAV Video Decoder
pub const CLSID_LAV_VIDEO: GUID = GUID::from_u128(0xee30215d_164f_4a92_a4eb_9d4c13390f9f);

/// CLSID for LAV Audio Decoder
pub const CLSID_LAV_AUDIO: GUID = GUID::from_u128(0xe8e73b6b_4cb3_44a4_be99_4f7bcb96e491);

// ============================================================================
// LAV Video Settings Interface
// ============================================================================

/// IID for ILAVVideoSettings
pub const IID_ILAV_VIDEO_SETTINGS: GUID = GUID::from_u128(0xfa40d6e9_4d38_4761_add2_71a9ec5fd32f);

/// LAV Video Hardware Decoder modes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavHwAccel {
    None = 0,
    Cuda = 1,
    QuickSync = 2,
    Dxva2 = 3,
    Dxva2CopyBack = 4,
    Dxva2Native = 5,
    D3D11 = 6,
}

/// LAV Deinterlacing modes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavDeintMode {
    Auto = 0,
    Aggressive = 1,
    Force = 2,
    Disabled = 3,
}

/// LAV Deinterlacing field order
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavDeintFieldOrder {
    Auto = 0,
    TopFieldFirst = 1,
    BottomFieldFirst = 2,
}

/// LAV Software Deinterlacing modes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavSwDeintMode {
    None = 0,
    Yadif = 1,
    W3fdifSimple = 2,
    W3fdifComplex = 3,
}

/// LAV Video output pixel formats
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavOutPixFmt {
    None = 0,
    Yv12 = 1,
    Nv12 = 2,
    Yuy2 = 3,
    Uyvy = 4,
    Ayuv = 5,
    P010 = 6,
    P210 = 7,
    Y410 = 8,
    P016 = 9,
    P216 = 10,
    Y416 = 11,
    Rgb32 = 12,
    Rgb24 = 13,
    V210 = 14,
    V410 = 15,
    Yv16 = 16,
    Yv24 = 17,
    Rgb48 = 18,
}

/// LAV Video dithering modes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavDither {
    Ordered = 0,
    Random = 1,
    ErrorDiffusion = 2,
}

// ============================================================================
// LAV Video Settings
// ============================================================================

/// Configuration for LAV Video Decoder
#[derive(Debug, Clone)]
pub struct LavVideoSettings {
    /// Hardware acceleration mode
    pub hw_accel: LavHwAccel,
    /// Deinterlacing mode
    pub deint_mode: LavDeintMode,
    /// Deinterlacing field order
    pub deint_field_order: LavDeintFieldOrder,
    /// Output at double rate (50/60fps for interlaced)
    pub deint_output_double: bool,
    /// Software deinterlacing mode (when HW deint unavailable)
    pub sw_deint_mode: LavSwDeintMode,
    /// Enable HW deinterlacing
    pub hw_deint: bool,
    /// HW deinterlacing output mode (0=best quality, 1=bob, 2=adaptive)
    pub hw_deint_mode: u32,
    /// High quality processing
    pub high_quality: bool,
    /// Dithering mode
    pub dither_mode: LavDither,
    /// Preferred output pixel format
    pub output_format: LavOutPixFmt,
    /// Enable DXVA output (requires D3D11 or DXVA2)
    pub dxva_output: bool,
    /// Number of decode threads (0=auto)
    pub num_threads: u32,
    /// Enable stream AR correction
    pub stream_ar: bool,
    /// Enable film mode detection
    pub film_mode: bool,
}

impl Default for LavVideoSettings {
    fn default() -> Self {
        Self {
            hw_accel: LavHwAccel::Cuda, // CUVID
            deint_mode: LavDeintMode::Force,
            deint_field_order: LavDeintFieldOrder::Auto,
            deint_output_double: true, // 50/60fps output
            sw_deint_mode: LavSwDeintMode::Yadif,
            hw_deint: true,
            hw_deint_mode: 2, // Adaptive
            high_quality: true,
            dither_mode: LavDither::Random,
            output_format: LavOutPixFmt::Nv12,
            dxva_output: false,
            num_threads: 0,
            stream_ar: true,
            film_mode: true,
        }
    }
}

impl LavVideoSettings {
    /// Create settings matching Josh's preferred configuration:
    /// - CUVID hardware acceleration
    /// - Adaptive HW deinterlacing
    /// - Forced deinterlacing mode
    /// - 50/60fps output (double rate)
    pub fn cuvid_adaptive_forced() -> Self {
        Self {
            hw_accel: LavHwAccel::Cuda,
            deint_mode: LavDeintMode::Force,
            deint_field_order: LavDeintFieldOrder::Auto,
            deint_output_double: true,
            sw_deint_mode: LavSwDeintMode::Yadif,
            hw_deint: true,
            hw_deint_mode: 2, // Adaptive
            high_quality: true,
            dither_mode: LavDither::Random,
            output_format: LavOutPixFmt::Nv12,
            dxva_output: false,
            num_threads: 0,
            stream_ar: true,
            film_mode: true,
        }
    }
}

// ============================================================================
// LAV Audio Settings
// ============================================================================

/// IID for ILAVAudioSettings
pub const IID_ILAV_AUDIO_SETTINGS: GUID = GUID::from_u128(0x4158a22b_6553_45d4_8f8b_f3f90b5b5c9b);

/// LAV Audio mixing modes
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LavAudioMixing {
    None = 0,
    Dolby = 1,
    DplII = 2,
    Untouched = 3,
}

/// Configuration for LAV Audio Decoder
#[derive(Debug, Clone)]
pub struct LavAudioSettings {
    /// Enable DRC (Dynamic Range Compression)
    pub drc: bool,
    /// DRC level (0-100)
    pub drc_level: u32,
    /// Audio delay in milliseconds
    pub audio_delay: i32,
    /// Enable bit-streaming
    pub bitstream: bool,
    /// Bitstream formats (DTS, AC3, TrueHD, etc.)
    pub bitstream_formats: u32,
    /// Mixing mode
    pub mixing: LavAudioMixing,
    /// Output sample format
    pub sample_format: u32,
    /// Output sample rate (0=auto)
    pub sample_rate: u32,
    /// Expand mono to stereo
    pub expand_mono: bool,
    /// Expand 6.1 to 7.1
    pub expand_61: bool,
}

impl Default for LavAudioSettings {
    fn default() -> Self {
        Self {
            drc: true,
            drc_level: 100,
            audio_delay: 0,
            bitstream: false,
            bitstream_formats: 0,
            mixing: LavAudioMixing::None,
            sample_format: 0, // Auto
            sample_rate: 0,   // Auto
            expand_mono: true,
            expand_61: true,
        }
    }
}

// ============================================================================
// LAV Splitter Settings
// ============================================================================

/// IID for ILAVSplitterSettings
pub const IID_ILAV_SPLITTER_SETTINGS: GUID =
    GUID::from_u128(0x774a919d_ea95_4a87_8a1e_f48abf3b2678);

/// Configuration for LAV Splitter
#[derive(Debug, Clone)]
pub struct LavSplitterSettings {
    /// Preferred audio language (e.g., "eng", "jpn")
    pub audio_language: String,
    /// Preferred subtitle language
    pub subtitle_language: String,
    /// Enable subtitles by default
    pub subtitles_enabled: bool,
    /// Prefer forced subtitles
    pub forced_subtitles: bool,
    /// Queue size in MB
    pub queue_size: u32,
    /// Network stream timeout in ms
    pub network_timeout: u32,
    /// Enable VC-1 correction
    pub vc1_correction: bool,
    /// Enable matroska external segments
    pub mkv_external_segments: bool,
}

impl Default for LavSplitterSettings {
    fn default() -> Self {
        Self {
            audio_language: "eng".to_string(),
            subtitle_language: "eng".to_string(),
            subtitles_enabled: false,
            forced_subtitles: true,
            queue_size: 256,
            network_timeout: 10000,
            vc1_correction: true,
            mkv_external_segments: true,
        }
    }
}

// ============================================================================
// Installation check
// ============================================================================

/// Check if LAV Filters are installed by looking for the DLLs and registry entries
pub fn check_lav_installed() -> bool {
    // Check common installation paths
    let paths = [
        r"C:\Program Files\LAV Filters\LAVVideo.ax",
        r"C:\Program Files (x86)\LAV Filters\LAVVideo.ax",
        r"C:\Windows\System32\LAVVideo.ax",
        r"C:\Windows\SysWOW64\LAVVideo.ax",
    ];

    for path in &paths {
        if Path::new(path).exists() {
            return true;
        }
    }

    // Also check registry for CLSID
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::System::Registry::{
            RegCloseKey, RegOpenKeyExW, HKEY_CLASSES_ROOT, KEY_READ,
        };

        let key_path = format!(
            "CLSID\\{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
            CLSID_LAV_VIDEO.data1,
            CLSID_LAV_VIDEO.data2,
            CLSID_LAV_VIDEO.data3,
            CLSID_LAV_VIDEO.data4[0],
            CLSID_LAV_VIDEO.data4[1],
            CLSID_LAV_VIDEO.data4[2],
            CLSID_LAV_VIDEO.data4[3],
            CLSID_LAV_VIDEO.data4[4],
            CLSID_LAV_VIDEO.data4[5],
            CLSID_LAV_VIDEO.data4[6],
            CLSID_LAV_VIDEO.data4[7],
        );

        let wide: Vec<u16> = key_path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut hkey = windows::Win32::System::Registry::HKEY::default();

        unsafe {
            if RegOpenKeyExW(
                HKEY_CLASSES_ROOT,
                PCWSTR::from_raw(wide.as_ptr()),
                0,
                KEY_READ,
                &mut hkey,
            )
            .is_ok()
            {
                let _ = RegCloseKey(hkey);
                return true;
            }
        }
    }

    false
}

/// Get LAV Filters version if installed
pub fn get_lav_version() -> Option<String> {
    // Try to read version from the DLL
    let paths = [
        r"C:\Program Files\LAV Filters\LAVVideo.ax",
        r"C:\Program Files (x86)\LAV Filters\LAVVideo.ax",
    ];

    for path in &paths {
        if Path::new(path).exists() {
            // Would need to read version resource from DLL
            // For now, just return that it's installed
            return Some("installed".to_string());
        }
    }

    None
}
