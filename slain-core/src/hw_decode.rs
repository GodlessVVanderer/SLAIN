// HW_DECODE - Unified Hardware Video Decoder Interface
//
// This module provides a unified interface to hardware video decoders:
// • NVIDIA: NVDEC via nvcuvid (see nvdec.rs)
// • AMD: AMF via amfrt64.dll (see amf_decode.rs)
// • Linux: VAAPI via libva (see vaapi_decode.rs)
//
// Each backend is loaded dynamically at runtime - no compile-time dependencies.
// The unified Decoder enum dispatches to the appropriate backend based on
// what's available on the system.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Import real decoder implementations
use crate::nvdec::{self, NvdecDecoder, VideoCodec as NvdecCodec, DecodedFrame as NvdecFrame, FrameFormat};
use crate::amf_decode::{self, AmfDecoder, AmfCodec};
use crate::vaapi_decode::{self, VaapiDecoder, VaapiCodec};

// ============================================================================
// Unified Types
// ============================================================================

/// Video codec types supported by hardware decoders
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HwCodec {
    H264,
    H265,
    VP8,
    VP9,
    AV1,
    MPEG2,
    VC1,
}

impl HwCodec {
    /// Convert to NVDEC codec type
    pub fn to_nvdec(&self) -> Option<NvdecCodec> {
        match self {
            Self::H264 => Some(NvdecCodec::H264),
            Self::H265 => Some(NvdecCodec::H265),
            Self::VP8 => Some(NvdecCodec::VP8),
            Self::VP9 => Some(NvdecCodec::VP9),
            Self::AV1 => Some(NvdecCodec::AV1),
            Self::MPEG2 => Some(NvdecCodec::MPEG2),
            Self::VC1 => None, // NVDEC doesn't support VC1 on modern cards
        }
    }
    
    /// Convert to AMF codec type
    pub fn to_amf(&self) -> Option<AmfCodec> {
        match self {
            Self::H264 => Some(AmfCodec::H264),
            Self::H265 => Some(AmfCodec::H265),
            Self::VP9 => Some(AmfCodec::VP9),
            Self::AV1 => Some(AmfCodec::AV1),
            _ => None, // AMF doesn't support VP8, MPEG2, VC1
        }
    }
    
    /// Convert to VAAPI codec type
    pub fn to_vaapi(&self) -> Option<VaapiCodec> {
        match self {
            Self::H264 => Some(VaapiCodec::H264),
            Self::H265 => Some(VaapiCodec::H265),
            Self::VP8 => Some(VaapiCodec::VP8),
            Self::VP9 => Some(VaapiCodec::VP9),
            Self::AV1 => Some(VaapiCodec::AV1),
            Self::MPEG2 => Some(VaapiCodec::MPEG2),
            Self::VC1 => Some(VaapiCodec::VC1),
        }
    }
    
    /// Parse from string (fourcc or name)
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "h264" | "avc" | "avc1" => Some(Self::H264),
            "h265" | "hevc" | "hvc1" | "hev1" => Some(Self::H265),
            "vp8" => Some(Self::VP8),
            "vp9" => Some(Self::VP9),
            "av1" | "av01" => Some(Self::AV1),
            "mpeg2" | "mpg2" | "mp2v" => Some(Self::MPEG2),
            "vc1" | "wvc1" => Some(Self::VC1),
            _ => None,
        }
    }
}

/// Decoder backend type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HwDecoderType {
    Nvdec,      // NVIDIA GPU (Windows/Linux)
    Amf,        // AMD GPU (Windows only)
    Vaapi,      // VA-API (Linux - Intel/AMD/NVIDIA)
    Software,   // CPU fallback
}

impl HwDecoderType {
    /// Check if this decoder is available on the current system
    pub fn is_available(&self) -> bool {
        match self {
            Self::Nvdec => nvdec::nvdec_check_available(),
            Self::Amf => amf_decode::amf_check_available(),
            Self::Vaapi => vaapi_decode::vaapi_check_available(),
            Self::Software => true,
        }
    }
    
    /// Get priority (lower = better)
    pub fn priority(&self) -> u32 {
        match self {
            Self::Nvdec => 1,   // Fastest, best quality
            Self::Amf => 2,    // AMD GPUs
            Self::Vaapi => 3,  // Linux universal
            Self::Software => 99, // Last resort
        }
    }
    
    /// Get display name
    pub fn name(&self) -> &'static str {
        match self {
            Self::Nvdec => "NVIDIA NVDEC",
            Self::Amf => "AMD AMF",
            Self::Vaapi => "VA-API",
            Self::Software => "Software",
        }
    }
}

/// Unified decoded frame format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecodedFrame {
    pub pts: i64,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
    pub progressive: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    NV12,   // 8-bit YUV 4:2:0 semi-planar
    P010,   // 10-bit YUV 4:2:0 semi-planar
    P016,   // 16-bit YUV 4:2:0 semi-planar
    YUV420, // 8-bit YUV 4:2:0 planar
}

impl From<FrameFormat> for PixelFormat {
    fn from(f: FrameFormat) -> Self {
        match f {
            FrameFormat::NV12 => PixelFormat::NV12,
            FrameFormat::P016 => PixelFormat::P016,
        }
    }
}

impl From<NvdecFrame> for DecodedFrame {
    fn from(f: NvdecFrame) -> Self {
        Self {
            pts: f.pts,
            width: f.width,
            height: f.height,
            pitch: f.pitch,
            format: f.format.into(),
            data: f.data,
            progressive: f.progressive,
        }
    }
}

/// Decoder configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderConfig {
    pub codec: HwCodec,
    pub width: u32,
    pub height: u32,
    pub preferred_backend: Option<HwDecoderType>,
    pub allow_software_fallback: bool,
    pub extra_data: Option<Vec<u8>>, // SPS/PPS for H.264, etc.
}

impl Default for DecoderConfig {
    fn default() -> Self {
        Self {
            codec: HwCodec::H264,
            width: 1920,
            height: 1080,
            preferred_backend: None,
            allow_software_fallback: true,
            extra_data: None,
        }
    }
}

/// Decoder information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecoderInfo {
    pub backend: HwDecoderType,
    pub codec: HwCodec,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub max_surfaces: u32,
}

// ============================================================================
// Unified Decoder
// ============================================================================

/// Unified hardware decoder that wraps platform-specific implementations
pub enum HwDecoder {
    Nvdec(NvdecDecoder),
    Amf(AmfDecoder),
    Vaapi(VaapiDecoder),
    Software(SoftwareDecoder),
}

impl HwDecoder {
    /// Create a new decoder with automatic backend selection
    pub fn new(config: DecoderConfig) -> Result<Self, String> {
        // Try preferred backend first
        if let Some(preferred) = config.preferred_backend {
            if let Ok(decoder) = Self::create_with_backend(&config, preferred) {
                return Ok(decoder);
            }
        }
        
        // Try backends in priority order
        let backends = [
            HwDecoderType::Nvdec,
            HwDecoderType::Amf,
            HwDecoderType::Vaapi,
        ];
        
        for backend in backends {
            if backend.is_available() {
                if let Ok(decoder) = Self::create_with_backend(&config, backend) {
                    return Ok(decoder);
                }
            }
        }
        
        // Fall back to software if allowed
        if config.allow_software_fallback {
            return Ok(Self::Software(SoftwareDecoder::new(config)?));
        }
        
        Err("No suitable hardware decoder available".to_string())
    }
    
    /// Create decoder with specific backend
    fn create_with_backend(config: &DecoderConfig, backend: HwDecoderType) -> Result<Self, String> {
        match backend {
            HwDecoderType::Nvdec => {
                let codec = config.codec.to_nvdec()
                    .ok_or("Codec not supported by NVDEC")?;
                let decoder = NvdecDecoder::new(codec, config.width, config.height)?;
                Ok(Self::Nvdec(decoder))
            }
            HwDecoderType::Amf => {
                let codec = config.codec.to_amf()
                    .ok_or("Codec not supported by AMF")?;
                let decoder = AmfDecoder::new(codec, config.width, config.height)?;
                Ok(Self::Amf(decoder))
            }
            HwDecoderType::Vaapi => {
                let codec = config.codec.to_vaapi()
                    .ok_or("Codec not supported by VAAPI")?;
                let decoder = VaapiDecoder::new(codec, config.width, config.height)?;
                Ok(Self::Vaapi(decoder))
            }
            HwDecoderType::Software => {
                Ok(Self::Software(SoftwareDecoder::new(config.clone())?))
            }
        }
    }
    
    /// Decode a compressed packet
    pub fn decode(&mut self, data: &[u8], pts: i64) -> Result<Option<DecodedFrame>, String> {
        match self {
            Self::Nvdec(d) => {
                d.decode(data, pts).map(|opt| opt.map(|f| f.into()))
            }
            Self::Amf(d) => {
                d.decode(data, pts).map(|opt| opt.map(|f| DecodedFrame {
                    pts: f.pts,
                    width: f.width,
                    height: f.height,
                    pitch: f.pitch,
                    format: match f.format.as_str() {
                        "P010" => PixelFormat::P010,
                        _ => PixelFormat::NV12,
                    },
                    data: f.data,
                    progressive: f.progressive,
                }))
            }
            Self::Vaapi(d) => {
                d.decode(data, pts).map(|opt| opt.map(|f| DecodedFrame {
                    pts: f.pts,
                    width: f.width,
                    height: f.height,
                    pitch: f.pitch,
                    format: match f.format.as_str() {
                        "P010" => PixelFormat::P010,
                        _ => PixelFormat::NV12,
                    },
                    data: f.data,
                    progressive: f.progressive,
                }))
            }
            Self::Software(d) => d.decode(data, pts),
        }
    }
    
    /// Flush decoder and get remaining frames
    pub fn flush(&mut self) -> Vec<DecodedFrame> {
        match self {
            Self::Nvdec(d) => d.flush().into_iter().map(|f| f.into()).collect(),
            Self::Amf(d) => d.flush().into_iter().map(|f| DecodedFrame {
                pts: f.pts,
                width: f.width,
                height: f.height,
                pitch: f.pitch,
                format: match f.format.as_str() {
                    "P010" => PixelFormat::P010,
                    _ => PixelFormat::NV12,
                },
                data: f.data,
                progressive: f.progressive,
            }).collect(),
            Self::Vaapi(d) => d.flush().into_iter().map(|f| DecodedFrame {
                pts: f.pts,
                width: f.width,
                height: f.height,
                pitch: f.pitch,
                format: match f.format.as_str() {
                    "P010" => PixelFormat::P010,
                    _ => PixelFormat::NV12,
                },
                data: f.data,
                progressive: f.progressive,
            }).collect(),
            Self::Software(d) => d.flush(),
        }
    }
    
    /// Get decoder info
    pub fn info(&self) -> DecoderInfo {
        match self {
            Self::Nvdec(d) => {
                let info = d.info();
                DecoderInfo {
                    backend: HwDecoderType::Nvdec,
                    codec: HwCodec::H264, // TODO: get from decoder
                    width: info["width"].as_u64().unwrap_or(1920) as u32,
                    height: info["height"].as_u64().unwrap_or(1080) as u32,
                    pixel_format: PixelFormat::NV12,
                    max_surfaces: 16,
                }
            }
            Self::Amf(d) => {
                let info = d.info();
                DecoderInfo {
                    backend: HwDecoderType::Amf,
                    codec: HwCodec::H264,
                    width: info["width"].as_u64().unwrap_or(1920) as u32,
                    height: info["height"].as_u64().unwrap_or(1080) as u32,
                    pixel_format: PixelFormat::NV12,
                    max_surfaces: 16,
                }
            }
            Self::Vaapi(d) => {
                let info = d.info();
                DecoderInfo {
                    backend: HwDecoderType::Vaapi,
                    codec: HwCodec::H264,
                    width: info["width"].as_u64().unwrap_or(1920) as u32,
                    height: info["height"].as_u64().unwrap_or(1080) as u32,
                    pixel_format: PixelFormat::NV12,
                    max_surfaces: 8,
                }
            }
            Self::Software(d) => d.info().clone(),
        }
    }
    
    /// Get backend type
    pub fn backend(&self) -> HwDecoderType {
        match self {
            Self::Nvdec(_) => HwDecoderType::Nvdec,
            Self::Amf(_) => HwDecoderType::Amf,
            Self::Vaapi(_) => HwDecoderType::Vaapi,
            Self::Software(_) => HwDecoderType::Software,
        }
    }
}

// ============================================================================
// Software Decoder (CPU fallback)
// ============================================================================
#[derive(Clone)]
pub struct SoftwareDecoder {
    config: DecoderConfig,
    frame_counter: u64,
}

impl SoftwareDecoder {
    pub fn new(config: DecoderConfig) -> Result<Self, String> {
        Ok(Self {
            config,
            frame_counter: 0,
        })
    }
    /// Decode compressed data into a raw frame.
    /// NOTE: This is a placeholder software decoder.
    pub fn decode(
        &mut self,
        _data: &[u8],
        pts: i64,
    ) -> Result<Option<DecodedFrame>, String> {
        // TEMPORARY: generate a synthetic frame so the pipeline works
        // This proves decode → render → timing is functional
        self.frame_counter += 1;
        let width = self.config.width.max(1);
        let height = self.config.height.max(1);
        let frame_size = (width * height * 3) as usize; // RGB24
        let mut buffer = vec![0u8; frame_size];
        // Simple moving color pattern so you SEE motion
        let color = (self.frame_counter % 255) as u8;
        for pixel in buffer.chunks_exact_mut(3) {
            pixel[0] = color;         // R
            pixel[1] = 255 - color;   // G
            pixel[2] = color / 2;     // B
        }
        Ok(Some(DecodedFrame {
            pts,
            width,
            height,
            pitch: width * 3,
            format: PixelFormat::YUV420, // placeholder
            data: buffer,
            progressive: true,
        }))
    }
    pub fn flush(&mut self) -> Vec<DecodedFrame> {
        Vec::new()
    }
    pub fn info(&self) -> DecoderInfo {
        DecoderInfo {
            backend: HwDecoderType::Software,
            codec: self.config.codec,
            width: self.config.width,
            height: self.config.height,
            pixel_format: PixelFormat::YUV420,
            max_surfaces: 1,
        }
    }
}


// ============================================================================
// Helper Functions
// ============================================================================

/// Find best available decoder for a codec
pub fn find_best_decoder(codec: HwCodec) -> Option<HwDecoderType> {
    let backends = [
        HwDecoderType::Nvdec,
        HwDecoderType::Amf,
        HwDecoderType::Vaapi,
    ];
    
    backends.into_iter()
        .filter(|b| b.is_available())
        .filter(|b| match b {
            HwDecoderType::Nvdec => codec.to_nvdec().is_some(),
            HwDecoderType::Amf => codec.to_amf().is_some(),
            HwDecoderType::Vaapi => codec.to_vaapi().is_some(),
            HwDecoderType::Software => true,
        })
        .min_by_key(|b| b.priority())
}

/// Get all available decoders
pub fn available_decoders() -> Vec<HwDecoderType> {
    [
        HwDecoderType::Nvdec,
        HwDecoderType::Amf,
        HwDecoderType::Vaapi,
        HwDecoderType::Software,
    ].into_iter()
        .filter(|d| d.is_available())
        .collect()
}

/// Get decoder capabilities summary
pub fn decoder_capabilities() -> HashMap<String, serde_json::Value> {
    let mut caps = HashMap::new();
    
    // NVDEC
    if nvdec::nvdec_check_available() {
        caps.insert("nvdec".to_string(), nvdec::nvdec_get_capabilities());
    }
    
    // AMF
    if amf_decode::amf_check_available() {
        caps.insert("amf".to_string(), amf_decode::amf_get_capabilities());
    }
    
    // VAAPI
    if vaapi_decode::vaapi_check_available() {
        caps.insert("vaapi".to_string(), vaapi_decode::vaapi_get_capabilities());
    }
    
    caps
}

// ============================================================================
// Public API
// ============================================================================




pub fn hwdec_available_decoders() -> Vec<String> {
    available_decoders()
        .into_iter()
        .map(|d| d.name().to_string())
        .collect()
}


pub fn hwdec_best_for_codec(codec: String) -> Option<String> {
    let hc = HwCodec::from_str(&codec)?;
    find_best_decoder(hc).map(|d| d.name().to_string())
}


pub fn hwdec_capabilities() -> serde_json::Value {
    serde_json::json!(decoder_capabilities())
}


pub fn hwdec_check_nvidia() -> bool {
    nvdec::nvdec_check_available()
}


pub fn hwdec_check_amd() -> bool {
    amf_decode::amf_check_available()
}


pub fn hwdec_check_vaapi() -> bool {
    vaapi_decode::vaapi_check_available()
}


pub fn hwdec_check_intel() -> bool {
    // Intel uses VAAPI on Linux, or QuickSync on Windows (not implemented)
    #[cfg(target_os = "linux")]
    {
        vaapi_decode::vaapi_check_available()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false // Intel QuickSync not implemented for Windows yet
    }
}


pub fn hwdec_system_info() -> serde_json::Value {
    serde_json::json!({
        "available_backends": available_decoders().iter().map(|d| d.name()).collect::<Vec<_>>(),
        "nvidia": {
            "available": nvdec::nvdec_check_available(),
            "description": nvdec::nvdec_description(),
        },
        "amd": {
            "available": amf_decode::amf_check_available(),
            "description": amf_decode::amf_description(),
        },
        "vaapi": {
            "available": vaapi_decode::vaapi_check_available(),
            "description": vaapi_decode::vaapi_description(),
        },
        "supported_codecs": ["H264", "H265", "VP8", "VP9", "AV1", "MPEG2"],
    })
}


pub fn hwdec_description() -> String {
    r#"
HW_DECODE - Unified Hardware Video Decoder Interface

This module provides a unified interface to hardware video decoders with
automatic backend selection based on available hardware.

SUPPORTED BACKENDS:
• NVIDIA NVDEC: GeForce GTX 600+ / Quadro K-series+
  - Codecs: H.264, H.265, VP8, VP9, AV1 (30-series+), MPEG2
  - Dynamic loading via nvcuda.dll / libnvcuvid.so

• AMD AMF: Radeon RX 400+ / Pro WX-series+
  - Codecs: H.264, H.265, VP9 (Vega+), AV1 (RX 6000+)
  - Dynamic loading via amfrt64.dll (Windows only)

• VA-API: Intel/AMD/NVIDIA on Linux
  - Codecs: All common formats
  - Dynamic loading via libva.so.2

• Software: CPU fallback (requires additional libraries)

USAGE:
1. Create decoder with codec and resolution
2. Feed compressed NAL units via decode()
3. Receive NV12/P010 frames
4. Convert to RGB for display

AUTOMATIC SELECTION:
The decoder automatically picks the best available backend.
Priority: NVDEC > AMF > VAAPI > Software
"#.to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_codec_parsing() {
        assert_eq!(HwCodec::from_str("h264"), Some(HwCodec::H264));
        assert_eq!(HwCodec::from_str("HEVC"), Some(HwCodec::H265));
        assert_eq!(HwCodec::from_str("av01"), Some(HwCodec::AV1));
        assert_eq!(HwCodec::from_str("unknown"), None);
    }
    
    #[test]
    fn test_backend_priority() {
        assert!(HwDecoderType::Nvdec.priority() < HwDecoderType::Software.priority());
        assert!(HwDecoderType::Amf.priority() < HwDecoderType::Vaapi.priority());
    }
}
