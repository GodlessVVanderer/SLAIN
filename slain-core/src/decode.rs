//! # Video Decode Module
//!
//! Unified interface for hardware and software video decoders.
//! 
//! ## Decoder Selection Priority:
//! 1. NVDEC (NVIDIA) - if available and codec supported
//! 2. VCN (AMD) - if available and codec supported  
//! 3. QSV (Intel) - if available and codec supported
//! 4. Software (openh264/dav1d) - fallback

use crate::gpu::{GpuDevice, GpuVendor};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("Unsupported codec: {0}")]
    UnsupportedCodec(String),
    #[error("No decoder available for {codec} (tried: {tried:?})")]
    NoDecoder { codec: String, tried: Vec<String> },
    #[error("Decode failed: {0}")]
    DecodeFailed(String),
    #[error("Invalid NAL unit")]
    InvalidNal,
    #[error("Need more data")]
    NeedMoreData,
}

/// Video codec types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Codec {
    H264,
    H265,
    Vp8,
    Vp9,
    Av1,
}

impl Codec {
    pub fn from_fourcc(fourcc: &[u8; 4]) -> Option<Self> {
        match fourcc {
            b"avc1" | b"h264" | b"H264" => Some(Codec::H264),
            b"hvc1" | b"hev1" | b"h265" | b"H265" => Some(Codec::H265),
            b"vp08" | b"VP8 " => Some(Codec::Vp8),
            b"vp09" | b"VP9 " => Some(Codec::Vp9),
            b"av01" | b"AV1 " => Some(Codec::Av1),
            _ => None,
        }
    }
}

/// Pixel format of decoded frames
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Nv12,       // 4:2:0 semi-planar (Y plane + interleaved UV)
    I420,       // 4:2:0 planar (Y + U + V separate)
    P010,       // 10-bit 4:2:0 (for HDR)
    Rgba8,      // 8-bit RGBA (post-conversion)
}

/// A decoded video frame
#[derive(Debug)]
pub struct DecodedFrame {
    /// Frame data (layout depends on format)
    pub data: Vec<u8>,
    /// Pixel format
    pub format: PixelFormat,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels  
    pub height: u32,
    /// Presentation timestamp in microseconds
    pub pts_us: i64,
    /// Duration in microseconds
    pub duration_us: i64,
    /// Is this a keyframe?
    pub keyframe: bool,
}

/// Decoder trait - implemented by all decoder backends
pub trait Decoder: Send {
    /// Get the codec this decoder handles
    fn codec(&self) -> Codec;
    
    /// Decode a compressed packet into frames
    fn decode(&mut self, data: &[u8], pts_us: i64) -> Result<Vec<DecodedFrame>, DecodeError>;
    
    /// Flush any buffered frames
    fn flush(&mut self) -> Result<Vec<DecodedFrame>, DecodeError>;
    
    /// Reset decoder state
    fn reset(&mut self);
    
    /// Get decoder name for debugging
    fn name(&self) -> &str;
}

/// Decoder backend types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecoderBackend {
    Nvdec,      // NVIDIA hardware
    Vcn,        // AMD hardware
    Qsv,        // Intel hardware
    Vaapi,      // Linux VA-API
    OpenH264,   // Cisco software H.264
    Dav1d,      // VideoLAN AV1
    Software,   // Generic software fallback
}

/// Create the best available decoder for a codec
pub fn create_decoder(
    codec: Codec,
    gpu: Option<&GpuDevice>,
) -> Result<Box<dyn Decoder>, DecodeError> {
    let mut tried = Vec::new();
    
    // Try hardware first
    if let Some(device) = gpu {
        match device.vendor {
            GpuVendor::Nvidia => {
                tried.push("NVDEC".into());
                // TODO: Try NVDEC
            }
            GpuVendor::Amd => {
                tried.push("VCN".into());
                // TODO: Try VCN
            }
            GpuVendor::Intel => {
                tried.push("QSV".into());
                // TODO: Try QSV
            }
            _ => {}
        }
    }
    
    // Try software fallback
    match codec {
        Codec::H264 => {
            tried.push("OpenH264".into());
            #[cfg(feature = "software-decode")]
            {
                return Ok(Box::new(OpenH264Decoder::new()?));
            }
        }
        Codec::Av1 => {
            tried.push("dav1d".into());
            #[cfg(feature = "software-decode")]
            {
                return Ok(Box::new(Dav1dDecoder::new()?));
            }
        }
        _ => {}
    }
    
    Err(DecodeError::NoDecoder {
        codec: format!("{:?}", codec),
        tried,
    })
}

// ============================================================================
// OpenH264 Software Decoder
// ============================================================================

#[cfg(feature = "software-decode")]
pub struct OpenH264Decoder {
    // TODO: openh264 decoder instance
}

#[cfg(feature = "software-decode")]
impl OpenH264Decoder {
    pub fn new() -> Result<Self, DecodeError> {
        // TODO: Initialize openh264
        Ok(Self {})
    }
}

#[cfg(feature = "software-decode")]
impl Decoder for OpenH264Decoder {
    fn codec(&self) -> Codec { Codec::H264 }
    
    fn decode(&mut self, _data: &[u8], _pts_us: i64) -> Result<Vec<DecodedFrame>, DecodeError> {
        // TODO: Real decode
        Ok(vec![])
    }
    
    fn flush(&mut self) -> Result<Vec<DecodedFrame>, DecodeError> {
        Ok(vec![])
    }
    
    fn reset(&mut self) {}
    
    fn name(&self) -> &str { "OpenH264" }
}

// ============================================================================
// dav1d AV1 Decoder
// ============================================================================

#[cfg(feature = "software-decode")]
pub struct Dav1dDecoder {
    // TODO: dav1d decoder instance
}

#[cfg(feature = "software-decode")]
impl Dav1dDecoder {
    pub fn new() -> Result<Self, DecodeError> {
        // TODO: Initialize dav1d
        Ok(Self {})
    }
}

#[cfg(feature = "software-decode")]
impl Decoder for Dav1dDecoder {
    fn codec(&self) -> Codec { Codec::Av1 }
    
    fn decode(&mut self, _data: &[u8], _pts_us: i64) -> Result<Vec<DecodedFrame>, DecodeError> {
        // TODO: Real decode
        Ok(vec![])
    }
    
    fn flush(&mut self) -> Result<Vec<DecodedFrame>, DecodeError> {
        Ok(vec![])
    }
    
    fn reset(&mut self) {}
    
    fn name(&self) -> &str { "dav1d" }
}
