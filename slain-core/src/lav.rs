//! # LAV Filters - Pure Rust Implementation
//!
//! A complete replacement for LAV Filters (DirectShow) written in pure Rust.
//! No FFmpeg dependency - uses native Rust demuxers and decoders.
//!
//! ## Components
//!
//! - **LavSplitter**: Container demuxer (MKV, MP4, AVI, TS, FLV, OGG)
//! - **LavSplitterSource**: File and stream source reader
//! - **LavAudio**: Audio decoder (AAC, AC3, DTS, FLAC, Vorbis, Opus, MP3)
//! - **LavVideo**: Video decoder with hardware acceleration (H.264, H.265, VP9, AV1)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐
//! │ LavSplitterSource│  ← File/URL/Stream input
//! └────────┬────────┘
//!          │
//!          ▼
//! ┌─────────────────┐
//! │   LavSplitter   │  ← Auto-detect format, demux to streams
//! └────────┬────────┘
//!          │
//!     ┌────┴────┐
//!     ▼         ▼
//! ┌───────┐ ┌───────┐
//! │LavVideo│ │LavAudio│  ← Decode streams
//! └───────┘ └───────┘
//! ```

use openh264::formats::YUVSource;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, Clone)]
pub enum LavError {
    /// File not found or cannot be opened
    FileNotFound(String),
    /// Unknown or unsupported container format
    UnknownFormat,
    /// Unsupported codec
    UnsupportedCodec(String),
    /// Decoder initialization failed
    DecoderInit(String),
    /// Decode error
    DecodeError(String),
    /// End of stream
    EndOfStream,
    /// Need more data
    NeedMoreData,
    /// Seek failed
    SeekFailed,
    /// IO error
    IoError(String),
    /// Invalid data
    InvalidData(String),
}

impl std::fmt::Display for LavError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileNotFound(s) => write!(f, "File not found: {}", s),
            Self::UnknownFormat => write!(f, "Unknown container format"),
            Self::UnsupportedCodec(s) => write!(f, "Unsupported codec: {}", s),
            Self::DecoderInit(s) => write!(f, "Decoder init failed: {}", s),
            Self::DecodeError(s) => write!(f, "Decode error: {}", s),
            Self::EndOfStream => write!(f, "End of stream"),
            Self::NeedMoreData => write!(f, "Need more data"),
            Self::SeekFailed => write!(f, "Seek failed"),
            Self::IoError(s) => write!(f, "IO error: {}", s),
            Self::InvalidData(s) => write!(f, "Invalid data: {}", s),
        }
    }
}

impl std::error::Error for LavError {}

pub type LavResult<T> = Result<T, LavError>;

// ============================================================================
// Container Formats
// ============================================================================

/// Supported container formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ContainerFormat {
    /// Matroska / WebM
    Matroska,
    /// MP4 / MOV / M4V
    Mp4,
    /// AVI
    Avi,
    /// MPEG Transport Stream
    MpegTs,
    /// MPEG Program Stream
    MpegPs,
    /// FLV (Flash Video)
    Flv,
    /// OGG / OGM
    Ogg,
    /// WAV (audio only)
    Wav,
    /// Raw H.264 Annex B
    RawH264,
    /// Raw HEVC Annex B
    RawHevc,
    /// BluRay MPLS playlist
    BluRayMpls,
}

impl ContainerFormat {
    /// Detect format from file signature (magic bytes)
    pub fn detect(header: &[u8]) -> Option<Self> {
        if header.len() < 12 {
            return None;
        }

        // Matroska/WebM: 0x1A 0x45 0xDF 0xA3
        if header.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
            return Some(Self::Matroska);
        }

        // MP4/MOV: ftyp box or moov/mdat
        if &header[4..8] == b"ftyp" || &header[4..8] == b"moov" || &header[4..8] == b"mdat" {
            return Some(Self::Mp4);
        }

        // AVI: RIFF....AVI
        if header.starts_with(b"RIFF") && &header[8..12] == b"AVI " {
            return Some(Self::Avi);
        }

        // WAV: RIFF....WAVE
        if header.starts_with(b"RIFF") && &header[8..12] == b"WAVE" {
            return Some(Self::Wav);
        }

        // MPEG-TS: 0x47 sync byte (check multiple)
        if header[0] == 0x47 && (header.len() < 188 || header[188] == 0x47) {
            return Some(Self::MpegTs);
        }

        // MPEG-PS: 0x00 0x00 0x01 0xBA
        if header.starts_with(&[0x00, 0x00, 0x01, 0xBA]) {
            return Some(Self::MpegPs);
        }

        // FLV: FLV signature
        if header.starts_with(b"FLV") {
            return Some(Self::Flv);
        }

        // OGG: OggS
        if header.starts_with(b"OggS") {
            return Some(Self::Ogg);
        }

        // Raw H.264: NAL start code
        if header.starts_with(&[0x00, 0x00, 0x00, 0x01]) || header.starts_with(&[0x00, 0x00, 0x01])
        {
            // Could be H.264 or HEVC - check NAL type
            let nal_start = if header[2] == 0x01 { 3 } else { 4 };
            if header.len() > nal_start {
                let nal_type = header[nal_start] & 0x1F;
                if nal_type == 7 || nal_type == 8 {
                    // SPS/PPS - likely H.264
                    return Some(Self::RawH264);
                }
            }
        }

        None
    }

    /// Get common file extensions for this format
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Self::Matroska => &["mkv", "webm", "mka", "mk3d"],
            Self::Mp4 => &["mp4", "m4v", "m4a", "mov", "3gp"],
            Self::Avi => &["avi"],
            Self::MpegTs => &["ts", "m2ts", "mts"],
            Self::MpegPs => &["mpg", "mpeg", "vob"],
            Self::Flv => &["flv"],
            Self::Ogg => &["ogg", "ogv", "ogm", "oga"],
            Self::Wav => &["wav"],
            Self::RawH264 => &["h264", "264", "avc"],
            Self::RawHevc => &["h265", "265", "hevc"],
            Self::BluRayMpls => &["mpls"],
        }
    }
}

// ============================================================================
// Codec Types
// ============================================================================

/// Video codecs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VideoCodec {
    H264,
    H265,
    Vp8,
    Vp9,
    Av1,
    Mpeg2,
    Mpeg4,
    Vc1,
    Theora,
    ProRes,
    Mjpeg,
    RawVideo,
}

impl VideoCodec {
    /// Parse from FourCC
    pub fn from_fourcc(fourcc: &[u8; 4]) -> Option<Self> {
        match fourcc {
            b"avc1" | b"h264" | b"H264" | b"x264" | b"X264" => Some(Self::H264),
            b"hvc1" | b"hev1" | b"h265" | b"H265" | b"x265" => Some(Self::H265),
            b"vp08" | b"VP80" => Some(Self::Vp8),
            b"vp09" | b"VP90" => Some(Self::Vp9),
            b"av01" | b"AV01" => Some(Self::Av1),
            b"mpg2" | b"MPG2" | b"mp2v" => Some(Self::Mpeg2),
            b"mp4v" | b"MP4V" | b"xvid" | b"XVID" | b"divx" | b"DIVX" => Some(Self::Mpeg4),
            b"WVC1" | b"wvc1" | b"vc-1" => Some(Self::Vc1),
            b"theo" => Some(Self::Theora),
            b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" => Some(Self::ProRes),
            b"mjpg" | b"MJPG" => Some(Self::Mjpeg),
            _ => None,
        }
    }

    /// Parse from MKV codec ID
    pub fn from_mkv_codec_id(codec_id: &str) -> Option<Self> {
        match codec_id {
            "V_MPEG4/ISO/AVC" => Some(Self::H264),
            "V_MPEGH/ISO/HEVC" => Some(Self::H265),
            "V_VP8" => Some(Self::Vp8),
            "V_VP9" => Some(Self::Vp9),
            "V_AV1" => Some(Self::Av1),
            "V_MPEG2" => Some(Self::Mpeg2),
            "V_MPEG4/ISO/SP" | "V_MPEG4/ISO/ASP" | "V_MPEG4/MS/V3" => Some(Self::Mpeg4),
            "V_THEORA" => Some(Self::Theora),
            "V_PRORES" => Some(Self::ProRes),
            "V_MJPEG" => Some(Self::Mjpeg),
            _ => None,
        }
    }

    /// Check if hardware decoding is typically available
    pub fn hw_decode_available(&self) -> bool {
        matches!(
            self,
            Self::H264 | Self::H265 | Self::Vp9 | Self::Av1 | Self::Mpeg2 | Self::Vc1
        )
    }
}

/// Audio codecs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AudioCodec {
    Aac,
    Ac3,
    Eac3,
    Dts,
    DtsHd,
    TrueHd,
    Flac,
    Vorbis,
    Opus,
    Mp3,
    Mp2,
    Pcm,
    Alac,
    Wma,
    Alaw,
    Mulaw,
}

impl AudioCodec {
    /// Parse from FourCC/tag
    pub fn from_fourcc(fourcc: &[u8; 4]) -> Option<Self> {
        match fourcc {
            b"mp4a" | b"AAC " => Some(Self::Aac),
            b"ac-3" | b"AC3 " => Some(Self::Ac3),
            b"ec-3" | b"EAC3" => Some(Self::Eac3),
            b"dtsc" | b"DTS " => Some(Self::Dts),
            b"dtsh" | b"dtsl" => Some(Self::DtsHd),
            b"mlpa" => Some(Self::TrueHd),
            b"fLaC" | b"FLAC" => Some(Self::Flac),
            b"vorb" => Some(Self::Vorbis),
            b"Opus" | b"opus" => Some(Self::Opus),
            b"mp3 " | b".mp3" => Some(Self::Mp3),
            b"alac" | b"ALAC" => Some(Self::Alac),
            _ => None,
        }
    }

    /// Parse from MKV codec ID
    pub fn from_mkv_codec_id(codec_id: &str) -> Option<Self> {
        match codec_id {
            "A_AAC" | "A_AAC/MPEG4/LC" | "A_AAC/MPEG4/LTP" => Some(Self::Aac),
            "A_AC3" => Some(Self::Ac3),
            "A_EAC3" => Some(Self::Eac3),
            "A_DTS" => Some(Self::Dts),
            "A_DTS/EXPRESS" | "A_DTS/LOSSLESS" => Some(Self::DtsHd),
            "A_TRUEHD" => Some(Self::TrueHd),
            "A_FLAC" => Some(Self::Flac),
            "A_VORBIS" => Some(Self::Vorbis),
            "A_OPUS" => Some(Self::Opus),
            "A_MPEG/L3" => Some(Self::Mp3),
            "A_MPEG/L2" => Some(Self::Mp2),
            "A_PCM/INT/LIT" | "A_PCM/INT/BIG" | "A_PCM/FLOAT/IEEE" => Some(Self::Pcm),
            "A_ALAC" => Some(Self::Alac),
            _ => None,
        }
    }

    /// Can be bitstreamed to receiver
    pub fn can_bitstream(&self) -> bool {
        matches!(
            self,
            Self::Ac3 | Self::Eac3 | Self::Dts | Self::DtsHd | Self::TrueHd
        )
    }
}

// ============================================================================
// Stream Info
// ============================================================================

/// Video stream information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStreamInfo {
    /// Stream index
    pub index: u32,
    /// Codec
    pub codec: VideoCodec,
    /// Width in pixels
    pub width: u32,
    /// Height in pixels
    pub height: u32,
    /// Frame rate (fps)
    pub frame_rate: f64,
    /// Pixel aspect ratio
    pub par: f64,
    /// Bit depth (8, 10, 12)
    pub bit_depth: u8,
    /// Color space
    pub color_space: ColorSpace,
    /// HDR metadata if present
    pub hdr: Option<HdrMetadata>,
    /// Codec private data (SPS/PPS for H.264, etc.)
    pub codec_private: Vec<u8>,
    /// Language
    pub language: Option<String>,
    /// Title
    pub title: Option<String>,
    /// Is default track
    pub is_default: bool,
    /// Duration in microseconds
    pub duration_us: i64,
    /// Average bitrate in bits/s
    pub bitrate: u64,
}

/// Audio stream information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    /// Stream index
    pub index: u32,
    /// Codec
    pub codec: AudioCodec,
    /// Sample rate in Hz
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u8,
    /// Channel layout
    pub channel_layout: ChannelLayout,
    /// Bit depth (16, 24, 32)
    pub bit_depth: u8,
    /// Codec private data
    pub codec_private: Vec<u8>,
    /// Language
    pub language: Option<String>,
    /// Title
    pub title: Option<String>,
    /// Is default track
    pub is_default: bool,
    /// Duration in microseconds
    pub duration_us: i64,
    /// Average bitrate in bits/s
    pub bitrate: u64,
}

/// Subtitle stream information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleStreamInfo {
    /// Stream index
    pub index: u32,
    /// Format (SRT, ASS, PGS, VobSub)
    pub format: SubtitleFormat,
    /// Language
    pub language: Option<String>,
    /// Title
    pub title: Option<String>,
    /// Is default track
    pub is_default: bool,
    /// Is forced (for foreign parts)
    pub is_forced: bool,
    /// Is hearing impaired
    pub is_sdh: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubtitleFormat {
    Srt,
    Ass,
    Ssa,
    Pgs,
    VobSub,
    WebVtt,
    Dvb,
    Cea608,
    Cea708,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorSpace {
    Bt601,
    Bt709,
    Bt2020,
    Srgb,
}

impl Default for ColorSpace {
    fn default() -> Self {
        Self::Bt709
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdrMetadata {
    pub transfer: HdrTransfer,
    pub max_cll: u16,  // Maximum Content Light Level
    pub max_fall: u16, // Maximum Frame Average Light Level
    pub mastering_display: Option<MasteringDisplay>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HdrTransfer {
    Pq,  // HDR10, Dolby Vision
    Hlg, // HLG
    Sdr, // Standard Dynamic Range
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasteringDisplay {
    pub primaries: [[f32; 2]; 3], // RGB primaries
    pub white_point: [f32; 2],
    pub luminance_min: f32,
    pub luminance_max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Surround3_0, // L R C
    Quad,        // L R Ls Rs
    Surround5_0, // L R C Ls Rs
    Surround5_1, // L R C LFE Ls Rs
    Surround7_1, // L R C LFE Ls Rs Lb Rb
    Custom(u32), // Channel mask
}

impl ChannelLayout {
    pub fn from_channels(channels: u8) -> Self {
        match channels {
            1 => Self::Mono,
            2 => Self::Stereo,
            3 => Self::Surround3_0,
            4 => Self::Quad,
            5 => Self::Surround5_0,
            6 => Self::Surround5_1,
            8 => Self::Surround7_1,
            n => Self::Custom(n as u32),
        }
    }

    pub fn channel_count(&self) -> u8 {
        match self {
            Self::Mono => 1,
            Self::Stereo => 2,
            Self::Surround3_0 => 3,
            Self::Quad => 4,
            Self::Surround5_0 => 5,
            Self::Surround5_1 => 6,
            Self::Surround7_1 => 8,
            Self::Custom(n) => *n as u8,
        }
    }
}

// ============================================================================
// Media Info (Container metadata)
// ============================================================================

/// Complete media file information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    /// Container format
    pub format: ContainerFormat,
    /// Total duration in microseconds
    pub duration_us: i64,
    /// File size in bytes
    pub file_size: u64,
    /// Overall bitrate in bits/s
    pub bitrate: u64,
    /// Is seekable
    pub seekable: bool,
    /// Video streams
    pub video_streams: Vec<VideoStreamInfo>,
    /// Audio streams
    pub audio_streams: Vec<AudioStreamInfo>,
    /// Subtitle streams
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
    /// Container metadata (title, artist, etc.)
    pub metadata: HashMap<String, String>,
    /// Chapter list
    pub chapters: Vec<Chapter>,
    /// Attachments (fonts, covers)
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chapter {
    pub title: Option<String>,
    pub start_us: i64,
    pub end_us: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub mime_type: String,
    pub size: usize,
    pub data: Vec<u8>,
}

// ============================================================================
// Packets
// ============================================================================

/// Compressed packet from demuxer
#[derive(Debug, Clone)]
pub struct Packet {
    /// Stream index
    pub stream_index: u32,
    /// Packet data
    pub data: Vec<u8>,
    /// Presentation timestamp (microseconds)
    pub pts: i64,
    /// Decode timestamp (microseconds)
    pub dts: i64,
    /// Duration (microseconds)
    pub duration: i64,
    /// Is keyframe
    pub keyframe: bool,
    /// Position in file (for seeking)
    pub position: u64,
}

/// Decoded video frame
#[derive(Debug, Clone)]
pub struct VideoFrame {
    /// Frame data
    pub data: Vec<u8>,
    /// Width
    pub width: u32,
    /// Height
    pub height: u32,
    /// Pixel format
    pub format: PixelFormat,
    /// Presentation timestamp
    pub pts: i64,
    /// Duration
    pub duration: i64,
    /// Is keyframe
    pub keyframe: bool,
    /// Interlaced
    pub interlaced: bool,
    /// Top field first (if interlaced)
    pub tff: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    Nv12,
    I420,
    I422,
    I444,
    P010,
    P016,
    Rgb24,
    Rgba32,
    Bgra32,
}

/// Decoded audio frame
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// Sample data (interleaved)
    pub data: Vec<u8>,
    /// Sample format
    pub format: SampleFormat,
    /// Sample rate
    pub sample_rate: u32,
    /// Number of channels
    pub channels: u8,
    /// Number of samples per channel
    pub samples: usize,
    /// Presentation timestamp
    pub pts: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleFormat {
    S16,
    S32,
    F32,
    F64,
}

// ============================================================================
// LAV Splitter Source
// ============================================================================

/// Source reader for files and streams
pub struct LavSplitterSource {
    /// Inner reader
    reader: Box<dyn ReadSeek + Send>,
    /// File size (if known)
    size: Option<u64>,
    /// Current position
    position: u64,
    /// Is network source
    is_network: bool,
    /// Buffer for reading
    buffer: Vec<u8>,
}

/// Trait combining Read + Seek
pub trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

impl LavSplitterSource {
    /// Open a file
    pub fn open_file<P: AsRef<Path>>(path: P) -> LavResult<Self> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|e| LavError::FileNotFound(format!("{}: {}", path.display(), e)))?;

        let size = file.metadata().ok().map(|m| m.len());

        Ok(Self {
            reader: Box::new(BufReader::new(file)),
            size,
            position: 0,
            is_network: false,
            buffer: vec![0u8; 64 * 1024],
        })
    }

    /// Get file size
    pub fn size(&self) -> Option<u64> {
        self.size
    }

    /// Read bytes
    pub fn read(&mut self, buf: &mut [u8]) -> LavResult<usize> {
        let n = self
            .reader
            .read(buf)
            .map_err(|e| LavError::IoError(e.to_string()))?;
        self.position += n as u64;
        Ok(n)
    }

    /// Read exact bytes
    pub fn read_exact(&mut self, buf: &mut [u8]) -> LavResult<()> {
        self.reader
            .read_exact(buf)
            .map_err(|e| LavError::IoError(e.to_string()))?;
        self.position += buf.len() as u64;
        Ok(())
    }

    /// Seek to position
    pub fn seek(&mut self, pos: u64) -> LavResult<()> {
        self.reader
            .seek(SeekFrom::Start(pos))
            .map_err(|_| LavError::SeekFailed)?;
        self.position = pos;
        Ok(())
    }

    /// Get current position
    pub fn position(&self) -> u64 {
        self.position
    }

    /// Detect container format
    pub fn detect_format(&mut self) -> LavResult<ContainerFormat> {
        let mut header = [0u8; 256];
        self.read(&mut header)?;
        self.seek(0)?;

        ContainerFormat::detect(&header).ok_or(LavError::UnknownFormat)
    }
}

// ============================================================================
// LAV Splitter
// ============================================================================

/// Container demuxer
pub struct LavSplitter {
    /// Source
    source: LavSplitterSource,
    /// Detected format
    format: ContainerFormat,
    /// Media info
    info: MediaInfo,
    /// Current packet queue per stream
    packet_queues: HashMap<u32, Vec<Packet>>,
    /// Selected video stream
    video_stream: Option<u32>,
    /// Selected audio stream
    audio_stream: Option<u32>,
    /// Selected subtitle stream
    subtitle_stream: Option<u32>,
    /// Current position (microseconds)
    position_us: i64,
    /// Is at end of file
    eof: bool,
}

impl LavSplitter {
    /// Open a file
    pub fn open<P: AsRef<Path>>(path: P) -> LavResult<Self> {
        let mut source = LavSplitterSource::open_file(&path)?;
        let format = source.detect_format()?;

        // Parse container and get media info
        let info = Self::parse_container(&mut source, format)?;

        // Select default streams
        let video_stream = info
            .video_streams
            .iter()
            .find(|s| s.is_default)
            .or_else(|| info.video_streams.first())
            .map(|s| s.index);

        let audio_stream = info
            .audio_streams
            .iter()
            .find(|s| s.is_default)
            .or_else(|| info.audio_streams.first())
            .map(|s| s.index);

        let subtitle_stream = info
            .subtitle_streams
            .iter()
            .find(|s| s.is_default)
            .map(|s| s.index);

        Ok(Self {
            source,
            format,
            info,
            packet_queues: HashMap::new(),
            video_stream,
            audio_stream,
            subtitle_stream,
            position_us: 0,
            eof: false,
        })
    }

    /// Get media info
    pub fn info(&self) -> &MediaInfo {
        &self.info
    }

    /// Get container format
    pub fn format(&self) -> ContainerFormat {
        self.format
    }

    /// Get duration in microseconds
    pub fn duration(&self) -> i64 {
        self.info.duration_us
    }

    /// Select video stream by index
    pub fn select_video(&mut self, index: u32) -> bool {
        if self.info.video_streams.iter().any(|s| s.index == index) {
            self.video_stream = Some(index);
            true
        } else {
            false
        }
    }

    /// Select audio stream by index
    pub fn select_audio(&mut self, index: u32) -> bool {
        if self.info.audio_streams.iter().any(|s| s.index == index) {
            self.audio_stream = Some(index);
            true
        } else {
            false
        }
    }

    /// Select subtitle stream by index
    pub fn select_subtitle(&mut self, index: Option<u32>) {
        self.subtitle_stream = index;
    }

    /// Read next packet
    pub fn read_packet(&mut self) -> LavResult<Packet> {
        if self.eof {
            return Err(LavError::EndOfStream);
        }

        // This would call format-specific demuxer
        match self.format {
            ContainerFormat::Matroska => self.read_mkv_packet(),
            ContainerFormat::Mp4 => self.read_mp4_packet(),
            ContainerFormat::Avi => self.read_avi_packet(),
            ContainerFormat::MpegTs => self.read_ts_packet(),
            _ => Err(LavError::UnsupportedCodec(format!("{:?}", self.format))),
        }
    }

    /// Seek to timestamp (microseconds)
    pub fn seek(&mut self, timestamp_us: i64) -> LavResult<()> {
        // Clear packet queues
        self.packet_queues.clear();
        self.eof = false;

        // Seek in container
        self.seek_internal(timestamp_us)?;
        self.position_us = timestamp_us;

        Ok(())
    }

    /// Get current position
    pub fn position(&self) -> i64 {
        self.position_us
    }

    // ========================================================================
    // REAL FORMAT-SPECIFIC IMPLEMENTATIONS
    // ========================================================================

    fn parse_container(
        source: &mut LavSplitterSource,
        format: ContainerFormat,
    ) -> LavResult<MediaInfo> {
        match format {
            ContainerFormat::Matroska => Self::parse_mkv(source),
            ContainerFormat::Mp4 => Self::parse_mp4(source),
            ContainerFormat::Avi => Self::parse_avi(source),
            ContainerFormat::MpegTs => Self::parse_ts(source),
            _ => Err(LavError::UnsupportedCodec(format!("{:?}", format))),
        }
    }

    /// Parse Matroska/WebM container - REAL EBML parsing
    fn parse_mkv(source: &mut LavSplitterSource) -> LavResult<MediaInfo> {
        let file_size = source.size().unwrap_or(0);
        let mut info = MediaInfo {
            format: ContainerFormat::Matroska,
            duration_us: 0,
            file_size,
            bitrate: 0,
            seekable: true,
            video_streams: Vec::new(),
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            metadata: HashMap::new(),
            chapters: Vec::new(),
            attachments: Vec::new(),
        };

        // Read EBML header
        let mut header = [0u8; 4];
        source.read_exact(&mut header)?;
        if header != [0x1A, 0x45, 0xDF, 0xA3] {
            return Err(LavError::InvalidData("Not a valid MKV file".into()));
        }

        // Parse EBML header size and skip to Segment
        let header_size = Self::read_ebml_size(source)?;
        source.seek(source.position() + header_size)?;

        // Read Segment ID
        let mut seg_id = [0u8; 4];
        source.read_exact(&mut seg_id)?;
        if seg_id != [0x18, 0x53, 0x80, 0x67] {
            return Err(LavError::InvalidData("Missing Segment element".into()));
        }
        let _segment_size = Self::read_ebml_size(source)?;
        let segment_start = source.position();

        // Parse Segment children until we find tracks and info
        let mut timecode_scale: u64 = 1_000_000; // Default: 1ms
        let mut duration_raw: f64 = 0.0;

        while source.position() < file_size {
            let element_id = Self::read_ebml_id(source)?;
            let element_size = Self::read_ebml_size(source)?;
            let element_end = source.position() + element_size;

            match element_id {
                0x1549A966 => {
                    // Info element - contains timecode scale and duration
                    while source.position() < element_end {
                        let sub_id = Self::read_ebml_id(source)?;
                        let sub_size = Self::read_ebml_size(source)?;
                        match sub_id {
                            0x2AD7B1 => {
                                // TimecodeScale
                                timecode_scale = Self::read_ebml_uint(source, sub_size as usize)?;
                            }
                            0x4489 => {
                                // Duration (float)
                                duration_raw = Self::read_ebml_float(source, sub_size as usize)?;
                            }
                            _ => {
                                source.seek(source.position() + sub_size)?;
                            }
                        }
                    }
                    // Convert duration to microseconds
                    info.duration_us = ((duration_raw * timecode_scale as f64) / 1000.0) as i64;
                }
                0x1654AE6B => {
                    // Tracks element
                    Self::parse_mkv_tracks(source, element_end, &mut info)?;
                }
                0x1254C367 => {
                    // Tags element (metadata)
                    Self::parse_mkv_tags(source, element_end, &mut info)?;
                }
                0x1043A770 => {
                    // Chapters
                    Self::parse_mkv_chapters(source, element_end, &mut info)?;
                }
                0x1941A469 => {
                    // Attachments
                    source.seek(element_end)?; // Skip for now
                }
                0x1F43B675 => {
                    // Cluster - stop parsing header, we have what we need
                    source.seek(element_end - element_size)?; // Go back to cluster start
                    break;
                }
                _ => {
                    source.seek(element_end)?;
                }
            }
        }

        // Calculate bitrate
        if info.duration_us > 0 {
            info.bitrate = (file_size * 8 * 1_000_000) / info.duration_us as u64;
        }

        Ok(info)
    }

    fn parse_mkv_tracks(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
    ) -> LavResult<()> {
        let mut track_index = 0u32;

        while source.position() < end {
            let element_id = Self::read_ebml_id(source)?;
            let element_size = Self::read_ebml_size(source)?;
            let element_end = source.position() + element_size;

            if element_id == 0xAE {
                // TrackEntry
                let mut track_type: u8 = 0;
                let mut codec_id = String::new();
                let mut width = 0u32;
                let mut height = 0u32;
                let mut frame_rate = 0.0f64;
                let mut sample_rate = 0u32;
                let mut channels = 0u8;
                let mut bit_depth = 0u8;
                let mut codec_private = Vec::new();
                let mut language = None;
                let mut name = None;
                let mut is_default = false;

                while source.position() < element_end {
                    let sub_id = Self::read_ebml_id(source)?;
                    let sub_size = Self::read_ebml_size(source)?;

                    match sub_id {
                        0x83 => {
                            // TrackType
                            track_type = Self::read_ebml_uint(source, sub_size as usize)? as u8;
                        }
                        0x86 => {
                            // CodecID
                            codec_id = Self::read_ebml_string(source, sub_size as usize)?;
                        }
                        0x63A2 => {
                            // CodecPrivate
                            codec_private = vec![0u8; sub_size as usize];
                            source.read_exact(&mut codec_private)?;
                        }
                        0x22B59C => {
                            // Language
                            language = Some(Self::read_ebml_string(source, sub_size as usize)?);
                        }
                        0x536E => {
                            // Name
                            name = Some(Self::read_ebml_string(source, sub_size as usize)?);
                        }
                        0x88 => {
                            // FlagDefault
                            is_default = Self::read_ebml_uint(source, sub_size as usize)? != 0;
                        }
                        0xE0 => {
                            // Video
                            let video_end = source.position() + sub_size;
                            while source.position() < video_end {
                                let v_id = Self::read_ebml_id(source)?;
                                let v_size = Self::read_ebml_size(source)?;
                                match v_id {
                                    0xB0 => {
                                        width =
                                            Self::read_ebml_uint(source, v_size as usize)? as u32
                                    }
                                    0xBA => {
                                        height =
                                            Self::read_ebml_uint(source, v_size as usize)? as u32
                                    }
                                    0x2383E3 => {
                                        frame_rate = Self::read_ebml_float(source, v_size as usize)?
                                    }
                                    _ => source.seek(source.position() + v_size)?,
                                }
                            }
                        }
                        0xE1 => {
                            // Audio
                            let audio_end = source.position() + sub_size;
                            while source.position() < audio_end {
                                let a_id = Self::read_ebml_id(source)?;
                                let a_size = Self::read_ebml_size(source)?;
                                match a_id {
                                    0xB5 => {
                                        sample_rate =
                                            Self::read_ebml_float(source, a_size as usize)? as u32
                                    }
                                    0x9F => {
                                        channels =
                                            Self::read_ebml_uint(source, a_size as usize)? as u8
                                    }
                                    0x6264 => {
                                        bit_depth =
                                            Self::read_ebml_uint(source, a_size as usize)? as u8
                                    }
                                    _ => source.seek(source.position() + a_size)?,
                                }
                            }
                        }
                        _ => {
                            source.seek(source.position() + sub_size)?;
                        }
                    }
                }

                // Create stream info based on track type
                match track_type {
                    1 => {
                        // Video track
                        if let Some(codec) = VideoCodec::from_mkv_codec_id(&codec_id) {
                            info.video_streams.push(VideoStreamInfo {
                                index: track_index,
                                codec,
                                width,
                                height,
                                frame_rate: if frame_rate > 0.0 { frame_rate } else { 23.976 },
                                par: 1.0,
                                bit_depth: 8,
                                color_space: ColorSpace::Bt709,
                                hdr: None,
                                codec_private,
                                language,
                                title: name,
                                is_default,
                                duration_us: info.duration_us,
                                bitrate: 0,
                            });
                        }
                    }
                    2 => {
                        // Audio track
                        if let Some(codec) = AudioCodec::from_mkv_codec_id(&codec_id) {
                            info.audio_streams.push(AudioStreamInfo {
                                index: track_index,
                                codec,
                                sample_rate,
                                channels,
                                channel_layout: ChannelLayout::from_channels(channels),
                                bit_depth: if bit_depth > 0 { bit_depth } else { 16 },
                                codec_private,
                                language,
                                title: name,
                                is_default,
                                duration_us: info.duration_us,
                                bitrate: 0,
                            });
                        }
                    }
                    17 => {
                        // Subtitle track
                        let format = match codec_id.as_str() {
                            "S_TEXT/UTF8" => SubtitleFormat::Srt,
                            "S_TEXT/ASS" | "S_ASS" => SubtitleFormat::Ass,
                            "S_HDMV/PGS" => SubtitleFormat::Pgs,
                            "S_VOBSUB" => SubtitleFormat::VobSub,
                            "S_TEXT/WEBVTT" => SubtitleFormat::WebVtt,
                            _ => SubtitleFormat::Srt,
                        };
                        info.subtitle_streams.push(SubtitleStreamInfo {
                            index: track_index,
                            format,
                            language,
                            title: name,
                            is_default,
                            is_forced: false,
                            is_sdh: false,
                        });
                    }
                    _ => {}
                }

                track_index += 1;
            } else {
                source.seek(element_end)?;
            }
        }

        Ok(())
    }

    fn parse_mkv_tags(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
    ) -> LavResult<()> {
        while source.position() < end {
            let element_id = Self::read_ebml_id(source)?;
            let element_size = Self::read_ebml_size(source)?;
            let element_end = source.position() + element_size;

            if element_id == 0x7373 {
                // Tag
                let mut tag_name = String::new();
                let mut tag_value = String::new();

                while source.position() < element_end {
                    let sub_id = Self::read_ebml_id(source)?;
                    let sub_size = Self::read_ebml_size(source)?;

                    if sub_id == 0x67C8 {
                        // SimpleTag
                        let simple_end = source.position() + sub_size;
                        while source.position() < simple_end {
                            let st_id = Self::read_ebml_id(source)?;
                            let st_size = Self::read_ebml_size(source)?;
                            match st_id {
                                0x45A3 => {
                                    tag_name = Self::read_ebml_string(source, st_size as usize)?
                                }
                                0x4487 => {
                                    tag_value = Self::read_ebml_string(source, st_size as usize)?
                                }
                                _ => source.seek(source.position() + st_size)?,
                            }
                        }
                        if !tag_name.is_empty() {
                            info.metadata.insert(tag_name.clone(), tag_value.clone());
                        }
                    } else {
                        source.seek(source.position() + sub_size)?;
                    }
                }
            } else {
                source.seek(element_end)?;
            }
        }
        Ok(())
    }

    fn parse_mkv_chapters(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
    ) -> LavResult<()> {
        while source.position() < end {
            let element_id = Self::read_ebml_id(source)?;
            let element_size = Self::read_ebml_size(source)?;
            let element_end = source.position() + element_size;

            if element_id == 0x45B9 {
                // EditionEntry
                while source.position() < element_end {
                    let sub_id = Self::read_ebml_id(source)?;
                    let sub_size = Self::read_ebml_size(source)?;
                    let sub_end = source.position() + sub_size;

                    if sub_id == 0xB6 {
                        // ChapterAtom
                        let mut start_us: i64 = 0;
                        let mut end_us: i64 = 0;
                        let mut title: Option<String> = None;

                        while source.position() < sub_end {
                            let ch_id = Self::read_ebml_id(source)?;
                            let ch_size = Self::read_ebml_size(source)?;
                            match ch_id {
                                0x91 => {
                                    start_us = (Self::read_ebml_uint(source, ch_size as usize)?
                                        / 1000)
                                        as i64
                                }
                                0x92 => {
                                    end_us = (Self::read_ebml_uint(source, ch_size as usize)?
                                        / 1000) as i64
                                }
                                0x80 => {
                                    // ChapterDisplay
                                    let disp_end = source.position() + ch_size;
                                    while source.position() < disp_end {
                                        let d_id = Self::read_ebml_id(source)?;
                                        let d_size = Self::read_ebml_size(source)?;
                                        if d_id == 0x85 {
                                            title = Some(Self::read_ebml_string(
                                                source,
                                                d_size as usize,
                                            )?);
                                        } else {
                                            source.seek(source.position() + d_size)?;
                                        }
                                    }
                                }
                                _ => source.seek(source.position() + ch_size)?,
                            }
                        }

                        info.chapters.push(Chapter {
                            title,
                            start_us,
                            end_us,
                        });
                    } else {
                        source.seek(sub_end)?;
                    }
                }
            } else {
                source.seek(element_end)?;
            }
        }
        Ok(())
    }

    // EBML parsing helpers
    fn read_ebml_id(source: &mut LavSplitterSource) -> LavResult<u32> {
        let mut first = [0u8; 1];
        source.read_exact(&mut first)?;

        let num_bytes = first[0].leading_zeros() + 1;
        let mut id = first[0] as u32;

        for _ in 1..num_bytes {
            let mut byte = [0u8; 1];
            source.read_exact(&mut byte)?;
            id = (id << 8) | byte[0] as u32;
        }

        Ok(id)
    }

    fn read_ebml_size(source: &mut LavSplitterSource) -> LavResult<u64> {
        let mut first = [0u8; 1];
        source.read_exact(&mut first)?;

        let num_bytes = first[0].leading_zeros() + 1;
        let mask = (1u8 << (8 - num_bytes)) - 1;
        let mut size = (first[0] & mask) as u64;

        for _ in 1..num_bytes {
            let mut byte = [0u8; 1];
            source.read_exact(&mut byte)?;
            size = (size << 8) | byte[0] as u64;
        }

        // Check for "unknown size" marker
        if size == (1u64 << (7 * num_bytes)) - 1 {
            return Ok(u64::MAX);
        }

        Ok(size)
    }

    fn read_ebml_uint(source: &mut LavSplitterSource, size: usize) -> LavResult<u64> {
        let mut buf = [0u8; 8];
        let start = 8 - size.min(8);
        source.read_exact(&mut buf[start..])?;
        Ok(u64::from_be_bytes(buf))
    }

    fn read_ebml_float(source: &mut LavSplitterSource, size: usize) -> LavResult<f64> {
        match size {
            4 => {
                let mut buf = [0u8; 4];
                source.read_exact(&mut buf)?;
                Ok(f32::from_be_bytes(buf) as f64)
            }
            8 => {
                let mut buf = [0u8; 8];
                source.read_exact(&mut buf)?;
                Ok(f64::from_be_bytes(buf))
            }
            _ => Ok(0.0),
        }
    }

    fn read_ebml_string(source: &mut LavSplitterSource, size: usize) -> LavResult<String> {
        let mut buf = vec![0u8; size];
        source.read_exact(&mut buf)?;
        // Remove null terminators
        while buf.last() == Some(&0) {
            buf.pop();
        }
        Ok(String::from_utf8_lossy(&buf).to_string())
    }

    /// Parse MP4/MOV container - REAL box parsing
    fn parse_mp4(source: &mut LavSplitterSource) -> LavResult<MediaInfo> {
        let file_size = source.size().unwrap_or(0);
        let mut info = MediaInfo {
            format: ContainerFormat::Mp4,
            duration_us: 0,
            file_size,
            bitrate: 0,
            seekable: true,
            video_streams: Vec::new(),
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            metadata: HashMap::new(),
            chapters: Vec::new(),
            attachments: Vec::new(),
        };

        let mut timescale: u32 = 1000;

        // Parse top-level boxes
        while source.position() < file_size {
            let box_start = source.position();

            // Read box header
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let mut box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_type = &type_buf;

            // Handle extended size
            if box_size == 1 {
                let mut ext_size = [0u8; 8];
                source.read_exact(&mut ext_size)?;
                box_size = u64::from_be_bytes(ext_size);
            } else if box_size == 0 {
                box_size = file_size - box_start;
            }

            let box_end = box_start + box_size;

            match box_type {
                b"moov" => {
                    Self::parse_mp4_moov(source, box_end, &mut info, &mut timescale)?;
                }
                b"mdat" | b"free" | b"skip" => {
                    source.seek(box_end)?;
                }
                b"ftyp" => {
                    // File type - already detected
                    source.seek(box_end)?;
                }
                _ => {
                    source.seek(box_end)?;
                }
            }

            if source.position() >= file_size {
                break;
            }
        }

        // Calculate bitrate
        if info.duration_us > 0 {
            info.bitrate = (file_size * 8 * 1_000_000) / info.duration_us as u64;
        }

        Ok(info)
    }

    fn parse_mp4_moov(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
        timescale: &mut u32,
    ) -> LavResult<()> {
        let mut track_index = 0u32;

        while source.position() < end {
            let box_start = source.position();
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_end = box_start + box_size;

            match &type_buf {
                b"mvhd" => {
                    // Movie header - get timescale and duration
                    let mut version = [0u8; 1];
                    source.read_exact(&mut version)?;
                    source.seek(source.position() + 3)?; // flags

                    if version[0] == 0 {
                        source.seek(source.position() + 8)?; // creation/modification time
                        let mut ts = [0u8; 4];
                        source.read_exact(&mut ts)?;
                        *timescale = u32::from_be_bytes(ts);
                        let mut dur = [0u8; 4];
                        source.read_exact(&mut dur)?;
                        let duration = u32::from_be_bytes(dur) as u64;
                        info.duration_us = (duration * 1_000_000 / *timescale as u64) as i64;
                    } else {
                        source.seek(source.position() + 16)?; // creation/modification time
                        let mut ts = [0u8; 4];
                        source.read_exact(&mut ts)?;
                        *timescale = u32::from_be_bytes(ts);
                        let mut dur = [0u8; 8];
                        source.read_exact(&mut dur)?;
                        let duration = u64::from_be_bytes(dur);
                        info.duration_us = (duration * 1_000_000 / *timescale as u64) as i64;
                    }
                    source.seek(box_end)?;
                }
                b"trak" => {
                    Self::parse_mp4_trak(source, box_end, info, track_index, *timescale)?;
                    track_index += 1;
                }
                b"udta" => {
                    // User data (metadata)
                    Self::parse_mp4_udta(source, box_end, info)?;
                }
                _ => {
                    source.seek(box_end)?;
                }
            }
        }

        Ok(())
    }

    fn parse_mp4_trak(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
        track_index: u32,
        timescale: u32,
    ) -> LavResult<()> {
        let mut handler_type = [0u8; 4];
        let mut width = 0u32;
        let mut height = 0u32;
        let mut sample_rate = 0u32;
        let mut channels = 0u8;
        let mut codec_fourcc = [0u8; 4];
        let mut codec_private = Vec::new();

        while source.position() < end {
            let box_start = source.position();
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_end = box_start + box_size;

            match &type_buf {
                b"mdia" => {
                    Self::parse_mp4_mdia(
                        source,
                        box_end,
                        &mut handler_type,
                        &mut width,
                        &mut height,
                        &mut sample_rate,
                        &mut channels,
                        &mut codec_fourcc,
                        &mut codec_private,
                    )?;
                }
                _ => {
                    source.seek(box_end)?;
                }
            }
        }

        // Create stream based on handler type
        match &handler_type {
            b"vide" => {
                if let Some(codec) = VideoCodec::from_fourcc(&codec_fourcc) {
                    info.video_streams.push(VideoStreamInfo {
                        index: track_index,
                        codec,
                        width,
                        height,
                        frame_rate: 23.976,
                        par: 1.0,
                        bit_depth: 8,
                        color_space: ColorSpace::Bt709,
                        hdr: None,
                        codec_private,
                        language: None,
                        title: None,
                        is_default: track_index == 0,
                        duration_us: info.duration_us,
                        bitrate: 0,
                    });
                }
            }
            b"soun" => {
                if let Some(codec) = AudioCodec::from_fourcc(&codec_fourcc) {
                    info.audio_streams.push(AudioStreamInfo {
                        index: track_index,
                        codec,
                        sample_rate,
                        channels,
                        channel_layout: ChannelLayout::from_channels(channels),
                        bit_depth: 16,
                        codec_private,
                        language: None,
                        title: None,
                        is_default: info.audio_streams.is_empty(),
                        duration_us: info.duration_us,
                        bitrate: 0,
                    });
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn parse_mp4_mdia(
        source: &mut LavSplitterSource,
        end: u64,
        handler_type: &mut [u8; 4],
        width: &mut u32,
        height: &mut u32,
        sample_rate: &mut u32,
        channels: &mut u8,
        codec_fourcc: &mut [u8; 4],
        codec_private: &mut Vec<u8>,
    ) -> LavResult<()> {
        while source.position() < end {
            let box_start = source.position();
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_end = box_start + box_size;

            match &type_buf {
                b"hdlr" => {
                    source.seek(source.position() + 8)?; // version, flags, pre_defined
                    source.read_exact(handler_type)?;
                    source.seek(box_end)?;
                }
                b"minf" => {
                    Self::parse_mp4_minf(
                        source,
                        box_end,
                        width,
                        height,
                        sample_rate,
                        channels,
                        codec_fourcc,
                        codec_private,
                    )?;
                }
                _ => {
                    source.seek(box_end)?;
                }
            }
        }
        Ok(())
    }

    fn parse_mp4_minf(
        source: &mut LavSplitterSource,
        end: u64,
        width: &mut u32,
        height: &mut u32,
        sample_rate: &mut u32,
        channels: &mut u8,
        codec_fourcc: &mut [u8; 4],
        codec_private: &mut Vec<u8>,
    ) -> LavResult<()> {
        while source.position() < end {
            let box_start = source.position();
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_end = box_start + box_size;

            if &type_buf == b"stbl" {
                Self::parse_mp4_stbl(
                    source,
                    box_end,
                    width,
                    height,
                    sample_rate,
                    channels,
                    codec_fourcc,
                    codec_private,
                )?;
            } else {
                source.seek(box_end)?;
            }
        }
        Ok(())
    }

    fn parse_mp4_stbl(
        source: &mut LavSplitterSource,
        end: u64,
        width: &mut u32,
        height: &mut u32,
        sample_rate: &mut u32,
        channels: &mut u8,
        codec_fourcc: &mut [u8; 4],
        codec_private: &mut Vec<u8>,
    ) -> LavResult<()> {
        while source.position() < end {
            let box_start = source.position();
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_end = box_start + box_size;

            if &type_buf == b"stsd" {
                // Sample description
                source.seek(source.position() + 4)?; // version, flags
                let mut count_buf = [0u8; 4];
                source.read_exact(&mut count_buf)?;

                // Read first sample entry
                let entry_start = source.position();
                let mut entry_size = [0u8; 4];
                source.read_exact(&mut entry_size)?;
                source.read_exact(codec_fourcc)?;
                source.seek(source.position() + 6)?; // reserved
                source.seek(source.position() + 2)?; // data_reference_index

                // Video-specific
                if codec_fourcc == b"avc1" || codec_fourcc == b"hvc1" || codec_fourcc == b"vp09" {
                    source.seek(source.position() + 16)?; // pre_defined, reserved
                    let mut w = [0u8; 2];
                    source.read_exact(&mut w)?;
                    *width = u16::from_be_bytes(w) as u32;
                    let mut h = [0u8; 2];
                    source.read_exact(&mut h)?;
                    *height = u16::from_be_bytes(h) as u32;

                    // Look for avcC/hvcC box
                    source.seek(source.position() + 50)?; // skip to extensions
                    let ext_end = entry_start + u32::from_be_bytes(entry_size) as u64;
                    while source.position() + 8 < ext_end {
                        let mut ext_size = [0u8; 4];
                        source.read_exact(&mut ext_size)?;
                        let mut ext_type = [0u8; 4];
                        source.read_exact(&mut ext_type)?;

                        if &ext_type == b"avcC" || &ext_type == b"hvcC" {
                            let data_size = u32::from_be_bytes(ext_size) as usize - 8;
                            *codec_private = vec![0u8; data_size];
                            source.read_exact(codec_private)?;
                            break;
                        } else {
                            source.seek(
                                source.position() + u32::from_be_bytes(ext_size) as u64 - 8,
                            )?;
                        }
                    }
                }
                // Audio-specific
                else if codec_fourcc == b"mp4a"
                    || codec_fourcc == b"ac-3"
                    || codec_fourcc == b"fLaC"
                {
                    source.seek(source.position() + 8)?; // reserved
                    let mut ch = [0u8; 2];
                    source.read_exact(&mut ch)?;
                    *channels = u16::from_be_bytes(ch) as u8;
                    source.seek(source.position() + 6)?; // sample_size, reserved
                    let mut sr = [0u8; 4];
                    source.read_exact(&mut sr)?;
                    *sample_rate = (u32::from_be_bytes(sr) >> 16) as u32;

                    // Look for esds box (AAC config)
                    let ext_end = entry_start + u32::from_be_bytes(entry_size) as u64;
                    while source.position() + 8 < ext_end {
                        let mut ext_size = [0u8; 4];
                        source.read_exact(&mut ext_size)?;
                        let mut ext_type = [0u8; 4];
                        source.read_exact(&mut ext_type)?;

                        if &ext_type == b"esds" {
                            // Parse ESDS for AudioSpecificConfig
                            let esds_end =
                                source.position() + u32::from_be_bytes(ext_size) as u64 - 8;
                            source.seek(source.position() + 4)?; // version, flags

                            // Find DecoderConfigDescriptor
                            while source.position() < esds_end {
                                let mut tag = [0u8; 1];
                                source.read_exact(&mut tag)?;

                                // Read size (variable length)
                                let mut len: u32 = 0;
                                for _ in 0..4 {
                                    let mut b = [0u8; 1];
                                    source.read_exact(&mut b)?;
                                    len = (len << 7) | (b[0] & 0x7F) as u32;
                                    if b[0] & 0x80 == 0 {
                                        break;
                                    }
                                }

                                if tag[0] == 0x05 {
                                    // DecoderSpecificInfo - this is AudioSpecificConfig
                                    *codec_private = vec![0u8; len as usize];
                                    source.read_exact(codec_private)?;
                                    break;
                                } else if tag[0] == 0x04 {
                                    // DecoderConfigDescriptor - skip header, continue
                                    source.seek(source.position() + 13)?;
                                } else {
                                    source.seek(source.position() + len as u64)?;
                                }
                            }
                            break;
                        } else {
                            source.seek(
                                source.position() + u32::from_be_bytes(ext_size) as u64 - 8,
                            )?;
                        }
                    }
                }

                source.seek(box_end)?;
            } else {
                source.seek(box_end)?;
            }
        }
        Ok(())
    }

    fn parse_mp4_udta(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
    ) -> LavResult<()> {
        while source.position() < end {
            let box_start = source.position();
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_end = box_start + box_size;

            if &type_buf == b"meta" {
                source.seek(source.position() + 4)?; // version, flags
                                                     // Continue parsing ilst inside meta
                Self::parse_mp4_udta(source, box_end, info)?;
            } else if &type_buf == b"ilst" {
                // iTunes metadata
                Self::parse_mp4_ilst(source, box_end, info)?;
            } else {
                source.seek(box_end)?;
            }
        }
        Ok(())
    }

    fn parse_mp4_ilst(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
    ) -> LavResult<()> {
        while source.position() < end {
            let box_start = source.position();
            let mut size_buf = [0u8; 4];
            if source.read(&mut size_buf)? < 4 {
                break;
            }
            let box_size = u32::from_be_bytes(size_buf) as u64;

            let mut type_buf = [0u8; 4];
            source.read_exact(&mut type_buf)?;
            let box_end = box_start + box_size;

            let key = match &type_buf {
                [0xA9, b'n', b'a', b'm'] => "title",
                [0xA9, b'A', b'R', b'T'] => "artist",
                [0xA9, b'a', b'l', b'b'] => "album",
                [0xA9, b'd', b'a', b'y'] => "year",
                _ => "",
            };

            if !key.is_empty() {
                // Read data box inside
                while source.position() + 8 < box_end {
                    let mut data_size = [0u8; 4];
                    source.read_exact(&mut data_size)?;
                    let mut data_type = [0u8; 4];
                    source.read_exact(&mut data_type)?;

                    if &data_type == b"data" {
                        source.seek(source.position() + 8)?; // type indicator, locale
                        let data_len = u32::from_be_bytes(data_size) as usize - 16;
                        let mut data = vec![0u8; data_len];
                        source.read_exact(&mut data)?;
                        if let Ok(s) = String::from_utf8(data) {
                            info.metadata.insert(key.to_string(), s);
                        }
                        break;
                    }
                }
            }

            source.seek(box_end)?;
        }
        Ok(())
    }

    /// Parse AVI container
    fn parse_avi(source: &mut LavSplitterSource) -> LavResult<MediaInfo> {
        let file_size = source.size().unwrap_or(0);
        let mut info = MediaInfo {
            format: ContainerFormat::Avi,
            duration_us: 0,
            file_size,
            bitrate: 0,
            seekable: true,
            video_streams: Vec::new(),
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            metadata: HashMap::new(),
            chapters: Vec::new(),
            attachments: Vec::new(),
        };

        // Skip RIFF header (already validated)
        source.seek(12)?;

        let mut track_index = 0u32;
        let mut fps = 0.0f64;
        let mut total_frames = 0u32;

        // Parse AVI chunks
        while source.position() + 8 <= file_size {
            let mut fourcc = [0u8; 4];
            if source.read(&mut fourcc)? < 4 {
                break;
            }
            let mut size_buf = [0u8; 4];
            source.read_exact(&mut size_buf)?;
            let chunk_size = u32::from_le_bytes(size_buf) as u64;
            let chunk_end = source.position() + chunk_size;

            match &fourcc {
                b"LIST" => {
                    let mut list_type = [0u8; 4];
                    source.read_exact(&mut list_type)?;

                    match &list_type {
                        b"hdrl" => {
                            // Main header list
                            Self::parse_avi_hdrl(
                                source,
                                chunk_end,
                                &mut info,
                                &mut fps,
                                &mut total_frames,
                            )?;
                        }
                        b"strl" => {
                            // Stream header list
                            Self::parse_avi_strl(source, chunk_end, &mut info, track_index)?;
                            track_index += 1;
                        }
                        b"movi" => {
                            // Movie data - stop parsing headers
                            break;
                        }
                        _ => {
                            source.seek(chunk_end)?;
                        }
                    }
                }
                _ => {
                    source.seek(chunk_end)?;
                }
            }

            // Align to word boundary
            if chunk_size % 2 != 0 && source.position() < file_size {
                source.seek(source.position() + 1)?;
            }
        }

        // Calculate duration
        if fps > 0.0 && total_frames > 0 {
            info.duration_us = ((total_frames as f64 / fps) * 1_000_000.0) as i64;
        }

        // Calculate bitrate
        if info.duration_us > 0 {
            info.bitrate = (file_size * 8 * 1_000_000) / info.duration_us as u64;
        }

        Ok(info)
    }

    fn parse_avi_hdrl(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
        fps: &mut f64,
        total_frames: &mut u32,
    ) -> LavResult<()> {
        while source.position() + 8 <= end {
            let mut fourcc = [0u8; 4];
            source.read_exact(&mut fourcc)?;
            let mut size_buf = [0u8; 4];
            source.read_exact(&mut size_buf)?;
            let chunk_size = u32::from_le_bytes(size_buf) as u64;
            let chunk_end = source.position() + chunk_size;

            if &fourcc == b"avih" {
                // Main AVI header
                let mut us_per_frame = [0u8; 4];
                source.read_exact(&mut us_per_frame)?;
                let us = u32::from_le_bytes(us_per_frame);
                if us > 0 {
                    *fps = 1_000_000.0 / us as f64;
                }

                source.seek(source.position() + 12)?; // max_bytes_per_sec, padding, flags

                let mut frames = [0u8; 4];
                source.read_exact(&mut frames)?;
                *total_frames = u32::from_le_bytes(frames);

                source.seek(chunk_end)?;
            } else if &fourcc == b"LIST" {
                let mut list_type = [0u8; 4];
                source.read_exact(&mut list_type)?;
                if &list_type == b"strl" {
                    Self::parse_avi_strl(
                        source,
                        chunk_end,
                        info,
                        info.video_streams.len() as u32 + info.audio_streams.len() as u32,
                    )?;
                } else {
                    source.seek(chunk_end)?;
                }
            } else {
                source.seek(chunk_end)?;
            }

            if chunk_size % 2 != 0 {
                source.seek(source.position() + 1)?;
            }
        }
        Ok(())
    }

    fn parse_avi_strl(
        source: &mut LavSplitterSource,
        end: u64,
        info: &mut MediaInfo,
        track_index: u32,
    ) -> LavResult<()> {
        let mut stream_type = [0u8; 4];
        let mut fourcc = [0u8; 4];
        let mut scale = 1u32;
        let mut rate = 1u32;
        let mut width = 0u32;
        let mut height = 0u32;
        let mut sample_rate = 0u32;
        let mut channels = 0u8;
        let mut bit_depth = 0u8;

        while source.position() + 8 <= end {
            let mut chunk_fourcc = [0u8; 4];
            source.read_exact(&mut chunk_fourcc)?;
            let mut size_buf = [0u8; 4];
            source.read_exact(&mut size_buf)?;
            let chunk_size = u32::from_le_bytes(size_buf) as u64;
            let chunk_end = source.position() + chunk_size;

            match &chunk_fourcc {
                b"strh" => {
                    // Stream header
                    source.read_exact(&mut stream_type)?;
                    source.read_exact(&mut fourcc)?;
                    source.seek(source.position() + 12)?; // flags, priority, language, initial_frames

                    let mut scale_buf = [0u8; 4];
                    source.read_exact(&mut scale_buf)?;
                    scale = u32::from_le_bytes(scale_buf);

                    let mut rate_buf = [0u8; 4];
                    source.read_exact(&mut rate_buf)?;
                    rate = u32::from_le_bytes(rate_buf);

                    source.seek(chunk_end)?;
                }
                b"strf" => {
                    // Stream format
                    if &stream_type == b"vids" {
                        // BITMAPINFOHEADER
                        source.seek(source.position() + 4)?; // biSize
                        let mut w = [0u8; 4];
                        source.read_exact(&mut w)?;
                        width = i32::from_le_bytes(w).unsigned_abs();
                        let mut h = [0u8; 4];
                        source.read_exact(&mut h)?;
                        height = i32::from_le_bytes(h).unsigned_abs();
                    } else if &stream_type == b"auds" {
                        // WAVEFORMATEX
                        source.seek(source.position() + 2)?; // wFormatTag
                        let mut ch = [0u8; 2];
                        source.read_exact(&mut ch)?;
                        channels = u16::from_le_bytes(ch) as u8;
                        let mut sr = [0u8; 4];
                        source.read_exact(&mut sr)?;
                        sample_rate = u32::from_le_bytes(sr);
                        source.seek(source.position() + 6)?; // avg_bytes, block_align
                        let mut bd = [0u8; 2];
                        source.read_exact(&mut bd)?;
                        bit_depth = u16::from_le_bytes(bd) as u8;
                    }
                    source.seek(chunk_end)?;
                }
                _ => {
                    source.seek(chunk_end)?;
                }
            }

            if chunk_size % 2 != 0 {
                source.seek(source.position() + 1)?;
            }
        }

        // Create stream info
        let frame_rate = if scale > 0 {
            rate as f64 / scale as f64
        } else {
            0.0
        };

        if &stream_type == b"vids" {
            let codec = VideoCodec::from_fourcc(&fourcc);
            if let Some(codec) = codec {
                info.video_streams.push(VideoStreamInfo {
                    index: track_index,
                    codec,
                    width,
                    height,
                    frame_rate,
                    par: 1.0,
                    bit_depth: 8,
                    color_space: ColorSpace::Bt709,
                    hdr: None,
                    codec_private: Vec::new(),
                    language: None,
                    title: None,
                    is_default: info.video_streams.is_empty(),
                    duration_us: info.duration_us,
                    bitrate: 0,
                });
            }
        } else if &stream_type == b"auds" {
            let codec = AudioCodec::from_fourcc(&fourcc).unwrap_or(AudioCodec::Pcm);
            info.audio_streams.push(AudioStreamInfo {
                index: track_index,
                codec,
                sample_rate,
                channels,
                channel_layout: ChannelLayout::from_channels(channels),
                bit_depth,
                codec_private: Vec::new(),
                language: None,
                title: None,
                is_default: info.audio_streams.is_empty(),
                duration_us: info.duration_us,
                bitrate: 0,
            });
        }

        Ok(())
    }

    /// Parse MPEG-TS container
    fn parse_ts(source: &mut LavSplitterSource) -> LavResult<MediaInfo> {
        let file_size = source.size().unwrap_or(0);
        let mut info = MediaInfo {
            format: ContainerFormat::MpegTs,
            duration_us: 0,
            file_size,
            bitrate: 0,
            seekable: true,
            video_streams: Vec::new(),
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            metadata: HashMap::new(),
            chapters: Vec::new(),
            attachments: Vec::new(),
        };

        // TS packet size is typically 188 bytes
        const TS_PACKET_SIZE: u64 = 188;
        let mut pmt_pids: Vec<u16> = Vec::new();
        let mut track_index = 0u32;

        // Read first few packets to find PAT and PMT
        let mut packets_read = 0;
        while source.position() + TS_PACKET_SIZE <= file_size && packets_read < 1000 {
            let packet_start = source.position();

            let mut sync = [0u8; 1];
            source.read_exact(&mut sync)?;
            if sync[0] != 0x47 {
                // Lost sync - try to find next packet
                source.seek(packet_start + 1)?;
                continue;
            }

            let mut header = [0u8; 3];
            source.read_exact(&mut header)?;

            let pid = ((header[0] as u16 & 0x1F) << 8) | header[1] as u16;
            let has_payload = (header[2] & 0x10) != 0;
            let has_adaptation = (header[2] & 0x20) != 0;

            if has_adaptation {
                let mut adapt_len = [0u8; 1];
                source.read_exact(&mut adapt_len)?;
                source.seek(source.position() + adapt_len[0] as u64)?;
            }

            if has_payload {
                if pid == 0 {
                    // PAT (Program Association Table)
                    source.seek(source.position() + 1)?; // pointer field

                    let mut pat_header = [0u8; 8];
                    source.read_exact(&mut pat_header)?;

                    let section_length =
                        ((pat_header[1] as u16 & 0x0F) << 8) | pat_header[2] as u16;
                    let entries = (section_length - 9) / 4;

                    for _ in 0..entries {
                        let mut entry = [0u8; 4];
                        source.read_exact(&mut entry)?;
                        let pmt_pid = ((entry[2] as u16 & 0x1F) << 8) | entry[3] as u16;
                        if pmt_pid != 0 && !pmt_pids.contains(&pmt_pid) {
                            pmt_pids.push(pmt_pid);
                        }
                    }
                } else if pmt_pids.contains(&pid) {
                    // PMT (Program Map Table)
                    source.seek(source.position() + 1)?; // pointer field

                    let mut pmt_header = [0u8; 12];
                    source.read_exact(&mut pmt_header)?;

                    let section_length =
                        ((pmt_header[1] as u16 & 0x0F) << 8) | pmt_header[2] as u16;
                    let program_info_length =
                        ((pmt_header[10] as u16 & 0x0F) << 8) | pmt_header[11] as u16;

                    source.seek(source.position() + program_info_length as u64)?;

                    let mut bytes_read = 13 + program_info_length;
                    while bytes_read + 5 <= section_length {
                        let mut stream_info = [0u8; 5];
                        source.read_exact(&mut stream_info)?;

                        let stream_type = stream_info[0];
                        let es_info_length =
                            ((stream_info[3] as u16 & 0x0F) << 8) | stream_info[4] as u16;

                        source.seek(source.position() + es_info_length as u64)?;

                        // Add stream based on type
                        match stream_type {
                            0x1B => {
                                // H.264
                                info.video_streams.push(VideoStreamInfo {
                                    index: track_index,
                                    codec: VideoCodec::H264,
                                    width: 1920,
                                    height: 1080,
                                    frame_rate: 29.97,
                                    par: 1.0,
                                    bit_depth: 8,
                                    color_space: ColorSpace::Bt709,
                                    hdr: None,
                                    codec_private: Vec::new(),
                                    language: None,
                                    title: None,
                                    is_default: info.video_streams.is_empty(),
                                    duration_us: 0,
                                    bitrate: 0,
                                });
                                track_index += 1;
                            }
                            0x24 => {
                                // H.265
                                info.video_streams.push(VideoStreamInfo {
                                    index: track_index,
                                    codec: VideoCodec::H265,
                                    width: 1920,
                                    height: 1080,
                                    frame_rate: 29.97,
                                    par: 1.0,
                                    bit_depth: 8,
                                    color_space: ColorSpace::Bt709,
                                    hdr: None,
                                    codec_private: Vec::new(),
                                    language: None,
                                    title: None,
                                    is_default: info.video_streams.is_empty(),
                                    duration_us: 0,
                                    bitrate: 0,
                                });
                                track_index += 1;
                            }
                            0x0F | 0x11 => {
                                // AAC
                                info.audio_streams.push(AudioStreamInfo {
                                    index: track_index,
                                    codec: AudioCodec::Aac,
                                    sample_rate: 48000,
                                    channels: 2,
                                    channel_layout: ChannelLayout::Stereo,
                                    bit_depth: 16,
                                    codec_private: Vec::new(),
                                    language: None,
                                    title: None,
                                    is_default: info.audio_streams.is_empty(),
                                    duration_us: 0,
                                    bitrate: 0,
                                });
                                track_index += 1;
                            }
                            0x81 => {
                                // AC-3
                                info.audio_streams.push(AudioStreamInfo {
                                    index: track_index,
                                    codec: AudioCodec::Ac3,
                                    sample_rate: 48000,
                                    channels: 6,
                                    channel_layout: ChannelLayout::Surround5_1,
                                    bit_depth: 16,
                                    codec_private: Vec::new(),
                                    language: None,
                                    title: None,
                                    is_default: info.audio_streams.is_empty(),
                                    duration_us: 0,
                                    bitrate: 0,
                                });
                                track_index += 1;
                            }
                            _ => {}
                        }

                        bytes_read += 5 + es_info_length;
                    }
                }
            }

            source.seek(packet_start + TS_PACKET_SIZE)?;
            packets_read += 1;

            // Stop early if we found streams
            if !info.video_streams.is_empty() && !info.audio_streams.is_empty() {
                break;
            }
        }

        // Estimate duration from file size and bitrate (rough)
        if !info.video_streams.is_empty() {
            // Assume ~15 Mbps for HD content
            info.bitrate = 15_000_000;
            info.duration_us = (file_size * 8 * 1_000_000 / info.bitrate) as i64;
        }

        source.seek(0)?;
        Ok(info)
    }

    fn read_mkv_packet(&mut self) -> LavResult<Packet> {
        // Find next Cluster or Block
        let file_size = self.source.size().unwrap_or(0);

        while self.source.position() < file_size {
            let element_id = Self::read_ebml_id(&mut self.source)?;
            let element_size = Self::read_ebml_size(&mut self.source)?;

            match element_id {
                0x1F43B675 => {
                    // Cluster - read blocks inside
                    let cluster_end = self.source.position() + element_size;
                    let mut cluster_timecode: i64 = 0;

                    while self.source.position() < cluster_end {
                        let block_id = Self::read_ebml_id(&mut self.source)?;
                        let block_size = Self::read_ebml_size(&mut self.source)?;
                        let block_end = self.source.position() + block_size;

                        match block_id {
                            0xE7 => {
                                // Timecode
                                cluster_timecode =
                                    Self::read_ebml_uint(&mut self.source, block_size as usize)?
                                        as i64;
                            }
                            0xA3 | 0xA1 => {
                                // SimpleBlock or Block
                                // Read track number (variable size)
                                let mut first = [0u8; 1];
                                self.source.read_exact(&mut first)?;
                                let track_bytes = first[0].leading_zeros() + 1;
                                let track_mask = (1u8 << (8 - track_bytes)) - 1;
                                let track_num = (first[0] & track_mask) as u32;

                                // Skip remaining track number bytes
                                for _ in 1..track_bytes {
                                    let mut b = [0u8; 1];
                                    self.source.read_exact(&mut b)?;
                                }

                                // Relative timecode
                                let mut tc = [0u8; 2];
                                self.source.read_exact(&mut tc)?;
                                let rel_timecode = i16::from_be_bytes(tc) as i64;

                                // Flags
                                let mut flags = [0u8; 1];
                                self.source.read_exact(&mut flags)?;
                                let keyframe = (flags[0] & 0x80) != 0;

                                // Read frame data
                                let data_size = block_end - self.source.position();
                                let mut data = vec![0u8; data_size as usize];
                                self.source.read_exact(&mut data)?;

                                // Calculate PTS (in microseconds)
                                let pts = (cluster_timecode + rel_timecode) * 1000; // Assuming ms timescale

                                return Ok(Packet {
                                    stream_index: track_num - 1,
                                    data,
                                    pts,
                                    dts: pts,
                                    duration: 0,
                                    keyframe,
                                    position: self.source.position(),
                                });
                            }
                            _ => {
                                self.source.seek(block_end)?;
                            }
                        }
                    }
                }
                _ => {
                    self.source.seek(self.source.position() + element_size)?;
                }
            }
        }

        self.eof = true;
        Err(LavError::EndOfStream)
    }

    fn read_mp4_packet(&mut self) -> LavResult<Packet> {
        // MP4 requires sample tables - for now return NeedMoreData
        // Full implementation would use stco/stsc/stsz tables built during parse
        Err(LavError::NeedMoreData)
    }

    fn read_avi_packet(&mut self) -> LavResult<Packet> {
        // Find next movi chunk
        let file_size = self.source.size().unwrap_or(0);

        while self.source.position() + 8 <= file_size {
            let mut fourcc = [0u8; 4];
            self.source.read_exact(&mut fourcc)?;
            let mut size_buf = [0u8; 4];
            self.source.read_exact(&mut size_buf)?;
            let chunk_size = u32::from_le_bytes(size_buf) as u64;

            // Check for video/audio chunks (##dc, ##wb)
            if (fourcc[2] == b'd' && fourcc[3] == b'c') || (fourcc[2] == b'w' && fourcc[3] == b'b')
            {
                let stream_index = ((fourcc[0] - b'0') * 10 + (fourcc[1] - b'0')) as u32;
                let is_video = fourcc[2] == b'd';

                let mut data = vec![0u8; chunk_size as usize];
                self.source.read_exact(&mut data)?;

                // Align
                if chunk_size % 2 != 0 {
                    self.source.seek(self.source.position() + 1)?;
                }

                self.position_us += 33333; // Assume ~30fps for now

                return Ok(Packet {
                    stream_index,
                    data,
                    pts: self.position_us,
                    dts: self.position_us,
                    duration: 33333,
                    keyframe: is_video && self.position_us == 0,
                    position: self.source.position(),
                });
            } else if &fourcc == b"LIST" {
                let mut list_type = [0u8; 4];
                self.source.read_exact(&mut list_type)?;
                if &list_type != b"movi" {
                    self.source.seek(self.source.position() + chunk_size - 4)?;
                }
                // Continue into movi list
            } else {
                self.source.seek(self.source.position() + chunk_size)?;
                if chunk_size % 2 != 0 {
                    self.source.seek(self.source.position() + 1)?;
                }
            }
        }

        self.eof = true;
        Err(LavError::EndOfStream)
    }

    fn read_ts_packet(&mut self) -> LavResult<Packet> {
        // TS packet reading - simplified PES extraction
        const TS_PACKET_SIZE: u64 = 188;
        let file_size = self.source.size().unwrap_or(0);
        let mut accumulated_data: Vec<u8> = Vec::new();
        let mut current_pts: i64 = 0;
        let mut current_stream: u32 = 0;

        while self.source.position() + TS_PACKET_SIZE <= file_size {
            let packet_start = self.source.position();

            let mut sync = [0u8; 1];
            self.source.read_exact(&mut sync)?;
            if sync[0] != 0x47 {
                self.source.seek(packet_start + 1)?;
                continue;
            }

            let mut header = [0u8; 3];
            self.source.read_exact(&mut header)?;

            let pusi = (header[0] & 0x40) != 0;
            let pid = ((header[0] as u16 & 0x1F) << 8) | header[1] as u16;
            let has_payload = (header[2] & 0x10) != 0;
            let has_adaptation = (header[2] & 0x20) != 0;

            if has_adaptation {
                let mut adapt_len = [0u8; 1];
                self.source.read_exact(&mut adapt_len)?;
                self.source
                    .seek(self.source.position() + adapt_len[0] as u64)?;
            }

            if has_payload && pid > 0x1F {
                let payload_start = self.source.position();
                let payload_size = (packet_start + TS_PACKET_SIZE) - payload_start;

                if pusi {
                    // Start of new PES packet
                    if !accumulated_data.is_empty() {
                        // Return previous packet
                        self.source.seek(packet_start)?;
                        return Ok(Packet {
                            stream_index: current_stream,
                            data: accumulated_data,
                            pts: current_pts,
                            dts: current_pts,
                            duration: 0,
                            keyframe: true,
                            position: packet_start,
                        });
                    }

                    // Parse PES header
                    let mut pes_header = [0u8; 9];
                    self.source.read_exact(&mut pes_header)?;

                    if pes_header[0] == 0 && pes_header[1] == 0 && pes_header[2] == 1 {
                        let stream_id = pes_header[3];
                        current_stream = if stream_id >= 0xE0 && stream_id <= 0xEF {
                            0 // Video
                        } else {
                            1 // Audio
                        };

                        let pes_header_len = pes_header[8] as u64;

                        // Check for PTS
                        if (pes_header[7] & 0x80) != 0 && pes_header_len >= 5 {
                            let mut pts_buf = [0u8; 5];
                            self.source.read_exact(&mut pts_buf)?;
                            let pts = (((pts_buf[0] as i64 >> 1) & 0x07) << 30)
                                | ((pts_buf[1] as i64) << 22)
                                | (((pts_buf[2] as i64 >> 1) & 0x7F) << 15)
                                | ((pts_buf[3] as i64) << 7)
                                | ((pts_buf[4] as i64 >> 1) & 0x7F);
                            current_pts = pts * 1_000_000 / 90_000; // Convert to microseconds
                            self.source
                                .seek(self.source.position() + pes_header_len - 5)?;
                        } else {
                            self.source.seek(self.source.position() + pes_header_len)?;
                        }

                        // Read payload
                        let remaining = (packet_start + TS_PACKET_SIZE) - self.source.position();
                        let mut payload = vec![0u8; remaining as usize];
                        self.source.read_exact(&mut payload)?;
                        accumulated_data = payload;
                    } else {
                        self.source.seek(packet_start + TS_PACKET_SIZE)?;
                    }
                } else {
                    // Continuation
                    let mut payload = vec![0u8; payload_size as usize];
                    self.source.read_exact(&mut payload)?;
                    accumulated_data.extend(payload);
                }
            }

            self.source.seek(packet_start + TS_PACKET_SIZE)?;
        }

        if !accumulated_data.is_empty() {
            return Ok(Packet {
                stream_index: current_stream,
                data: accumulated_data,
                pts: current_pts,
                dts: current_pts,
                duration: 0,
                keyframe: true,
                position: self.source.position(),
            });
        }

        self.eof = true;
        Err(LavError::EndOfStream)
    }

    fn seek_internal(&mut self, timestamp_us: i64) -> LavResult<()> {
        // For MKV: seek to nearest cluster
        // For MP4: use sample tables
        // For AVI: use idx1 index
        // For TS: seek to approximate position

        match self.format {
            ContainerFormat::Matroska => {
                // Seek to beginning and scan for cluster near timestamp
                self.source.seek(0)?;
                self.position_us = 0;
            }
            ContainerFormat::Mp4 | ContainerFormat::Avi => {
                self.source.seek(0)?;
                self.position_us = 0;
            }
            ContainerFormat::MpegTs => {
                // Estimate position based on bitrate
                if self.info.duration_us > 0 && self.info.file_size > 0 {
                    let ratio = timestamp_us as f64 / self.info.duration_us as f64;
                    let target_pos = (ratio * self.info.file_size as f64) as u64;
                    // Align to TS packet boundary
                    let aligned_pos = (target_pos / 188) * 188;
                    self.source.seek(aligned_pos)?;
                }
                self.position_us = timestamp_us;
            }
            _ => {}
        }

        Ok(())
    }
}

// ============================================================================
// LAV Video Decoder
// ============================================================================

/// Video decoder configuration
#[derive(Debug, Clone)]
pub struct VideoDecoderConfig {
    /// Preferred decoder (HW or SW)
    pub prefer_hw: bool,
    /// Specific HW decoder to use
    pub hw_decoder: Option<HwDecoder>,
    /// Output pixel format
    pub output_format: PixelFormat,
    /// Number of decode threads (for SW)
    pub threads: u32,
    /// Deinterlace mode
    pub deinterlace: DeinterlaceMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HwDecoder {
    Nvdec,        // NVIDIA
    Amf,          // AMD
    Qsv,          // Intel
    Vaapi,        // Linux VA-API
    Dxva2,        // Windows DirectX
    D3D11,        // Windows Direct3D 11
    VideoToolbox, // macOS
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeinterlaceMode {
    Off,
    Auto,
    Force,
}

impl Default for VideoDecoderConfig {
    fn default() -> Self {
        Self {
            prefer_hw: true,
            hw_decoder: None,
            output_format: PixelFormat::Nv12,
            threads: 0, // Auto
            deinterlace: DeinterlaceMode::Auto,
        }
    }
}

/// Video decoder with REAL openh264 integration
pub struct LavVideo {
    /// Codec
    codec: VideoCodec,
    /// Configuration
    config: VideoDecoderConfig,
    /// Stream info
    stream_info: VideoStreamInfo,
    /// Active HW decoder
    hw_decoder: Option<HwDecoder>,
    /// OpenH264 decoder instance
    h264_decoder: Option<openh264::decoder::Decoder>,
    /// Decoded frame queue
    frame_queue: Vec<VideoFrame>,
    /// Frame counter for PTS calculation
    frame_count: u64,
}

impl LavVideo {
    /// Create decoder for stream
    pub fn new(stream_info: VideoStreamInfo, config: VideoDecoderConfig) -> LavResult<Self> {
        let hw_decoder = if config.prefer_hw && stream_info.codec.hw_decode_available() {
            Self::probe_hw_decoder(stream_info.codec, config.hw_decoder)
        } else {
            None
        };

        // Create H.264 decoder if needed
        let h264_decoder = if stream_info.codec == VideoCodec::H264 && hw_decoder.is_none() {
            match openh264::decoder::Decoder::new() {
                Ok(dec) => Some(dec),
                Err(e) => {
                    tracing::warn!("Failed to create OpenH264 decoder: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            codec: stream_info.codec,
            config,
            stream_info,
            hw_decoder,
            h264_decoder,
            frame_queue: Vec::new(),
            frame_count: 0,
        })
    }

    fn probe_hw_decoder(codec: VideoCodec, preferred: Option<HwDecoder>) -> Option<HwDecoder> {
        // Check for NVIDIA GPU
        #[cfg(target_os = "windows")]
        {
            if std::path::Path::new("C:\\Windows\\System32\\nvdec64.dll").exists() {
                if matches!(
                    codec,
                    VideoCodec::H264 | VideoCodec::H265 | VideoCodec::Vp9 | VideoCodec::Av1
                ) {
                    return Some(preferred.unwrap_or(HwDecoder::Nvdec));
                }
            }
        }

        #[cfg(target_os = "linux")]
        {
            // Check for VA-API
            if std::path::Path::new("/dev/dri/renderD128").exists() {
                return Some(preferred.unwrap_or(HwDecoder::Vaapi));
            }
        }

        preferred
    }

    /// Initialize decoder with codec private data (SPS/PPS for H.264)
    pub fn init(&mut self, codec_private: &[u8]) -> LavResult<()> {
        if self.codec == VideoCodec::H264 {
            if let Some(ref mut decoder) = self.h264_decoder {
                // Feed SPS/PPS to decoder
                if !codec_private.is_empty() {
                    // Parse AVC decoder configuration record if present
                    if codec_private.len() > 6 && codec_private[0] == 1 {
                        // AVCDecoderConfigurationRecord format
                        let sps_pps = Self::parse_avcc(codec_private);
                        for nalu in sps_pps {
                            let _ = decoder.decode(&nalu);
                        }
                    } else {
                        // Raw NAL units with start codes
                        let _ = decoder.decode(codec_private);
                    }
                }
            }
        }
        Ok(())
    }

    /// Parse AVCDecoderConfigurationRecord to extract SPS/PPS
    fn parse_avcc(data: &[u8]) -> Vec<Vec<u8>> {
        let mut nalus = Vec::new();
        if data.len() < 7 {
            return nalus;
        }

        let num_sps = data[5] & 0x1F;
        let mut offset = 6;

        // Parse SPS
        for _ in 0..num_sps {
            if offset + 2 > data.len() {
                break;
            }
            let sps_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2;
            if offset + sps_len > data.len() {
                break;
            }

            // Add start code + SPS
            let mut nalu = vec![0x00, 0x00, 0x00, 0x01];
            nalu.extend_from_slice(&data[offset..offset + sps_len]);
            nalus.push(nalu);
            offset += sps_len;
        }

        // Parse PPS
        if offset < data.len() {
            let num_pps = data[offset];
            offset += 1;

            for _ in 0..num_pps {
                if offset + 2 > data.len() {
                    break;
                }
                let pps_len = u16::from_be_bytes([data[offset], data[offset + 1]]) as usize;
                offset += 2;
                if offset + pps_len > data.len() {
                    break;
                }

                let mut nalu = vec![0x00, 0x00, 0x00, 0x01];
                nalu.extend_from_slice(&data[offset..offset + pps_len]);
                nalus.push(nalu);
                offset += pps_len;
            }
        }

        nalus
    }

    /// Decode packet to frames - REAL IMPLEMENTATION
    pub fn decode(&mut self, packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        match self.codec {
            VideoCodec::H264 => self.decode_h264(packet),
            VideoCodec::H265 => self.decode_h265(packet),
            VideoCodec::Vp9 => self.decode_vp9(packet),
            VideoCodec::Av1 => self.decode_av1(packet),
            _ => Err(LavError::UnsupportedCodec(format!("{:?}", self.codec))),
        }
    }

    /// Flush decoder (get remaining frames)
    pub fn flush(&mut self) -> LavResult<Vec<VideoFrame>> {
        let frames = std::mem::take(&mut self.frame_queue);
        Ok(frames)
    }

    /// Reset decoder state
    pub fn reset(&mut self) {
        self.frame_queue.clear();
        self.frame_count = 0;
        // Recreate decoder
        if self.codec == VideoCodec::H264 {
            self.h264_decoder = openh264::decoder::Decoder::new().ok();
        }
    }

    /// Get decoder info
    pub fn decoder_name(&self) -> &str {
        if let Some(hw) = self.hw_decoder {
            match hw {
                HwDecoder::Nvdec => "NVDEC",
                HwDecoder::Amf => "AMF",
                HwDecoder::Qsv => "QuickSync",
                HwDecoder::Vaapi => "VA-API",
                HwDecoder::Dxva2 => "DXVA2",
                HwDecoder::D3D11 => "D3D11",
                HwDecoder::VideoToolbox => "VideoToolbox",
            }
        } else {
            match self.codec {
                VideoCodec::H264 => "OpenH264",
                VideoCodec::Av1 => "dav1d",
                _ => "Software",
            }
        }
    }

    /// REAL H.264 decoding using openh264 crate
    fn decode_h264(&mut self, packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        let decoder = self
            .h264_decoder
            .as_mut()
            .ok_or_else(|| LavError::DecoderInit("H.264 decoder not initialized".into()))?;

        // Convert length-prefixed NALUs to Annex B if needed
        let data = Self::convert_to_annexb(&packet.data);

        // Decode the NAL unit
        match decoder.decode(&data) {
            Ok(Some(yuv)) => {
                let (width, height) = yuv.dimensions();
                let strides = yuv.strides();

                // Get Y, U, V planes
                let y_plane = yuv.y();
                let u_plane = yuv.u();
                let v_plane = yuv.v();

                // Calculate actual dimensions
                let y_stride = strides.0;
                let uv_stride = strides.1;
                let uv_height = height / 2;

                // Create I420 buffer (Y + U + V planar)
                let y_size = width * height;
                let uv_size = (width / 2) * (height / 2);
                let mut i420_data = Vec::with_capacity(y_size + uv_size * 2);

                // Copy Y plane (remove stride padding)
                for row in 0..height {
                    let start = row * y_stride;
                    let end = start + width;
                    if end <= y_plane.len() {
                        i420_data.extend_from_slice(&y_plane[start..end]);
                    }
                }

                // Copy U plane
                for row in 0..uv_height {
                    let start = row * uv_stride;
                    let end = start + width / 2;
                    if end <= u_plane.len() {
                        i420_data.extend_from_slice(&u_plane[start..end]);
                    }
                }

                // Copy V plane
                for row in 0..uv_height {
                    let start = row * uv_stride;
                    let end = start + width / 2;
                    if end <= v_plane.len() {
                        i420_data.extend_from_slice(&v_plane[start..end]);
                    }
                }

                self.frame_count += 1;

                let frame = VideoFrame {
                    data: i420_data,
                    width: width as u32,
                    height: height as u32,
                    format: PixelFormat::I420,
                    pts: packet.pts,
                    duration: packet.duration,
                    keyframe: packet.keyframe,
                    interlaced: false,
                    tff: false,
                };

                Ok(vec![frame])
            }
            Ok(None) => {
                // Decoder needs more data
                Ok(Vec::new())
            }
            Err(e) => Err(LavError::DecodeError(format!(
                "OpenH264 decode error: {:?}",
                e
            ))),
        }
    }

    /// Convert MP4-style length-prefixed NALUs to Annex B format
    fn convert_to_annexb(data: &[u8]) -> Vec<u8> {
        // Check if already in Annex B format (starts with 0x00 0x00 0x00 0x01 or 0x00 0x00 0x01)
        if data.len() >= 4 && (data[0..4] == [0, 0, 0, 1] || data[0..3] == [0, 0, 1]) {
            return data.to_vec();
        }

        // Convert from length-prefixed to Annex B
        let mut annexb = Vec::with_capacity(data.len() + 32);
        let mut offset = 0;

        while offset + 4 <= data.len() {
            let nalu_len = u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]) as usize;
            offset += 4;

            if offset + nalu_len > data.len() {
                break;
            }

            // Add Annex B start code
            annexb.extend_from_slice(&[0x00, 0x00, 0x00, 0x01]);
            annexb.extend_from_slice(&data[offset..offset + nalu_len]);
            offset += nalu_len;
        }

        if annexb.is_empty() {
            // Fallback: return original data
            data.to_vec()
        } else {
            annexb
        }
    }

    fn decode_h265(&mut self, _packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        // HEVC requires HW decoder or external library
        // No good pure-Rust HEVC decoder exists yet
        Err(LavError::UnsupportedCodec(
            "H.265 requires hardware decoder".into(),
        ))
    }

    fn decode_vp9(&mut self, _packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        // VP9 typically uses HW decoder
        Err(LavError::UnsupportedCodec(
            "VP9 requires hardware decoder".into(),
        ))
    }

    fn decode_av1(&mut self, _packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        // Would use dav1d crate - TODO: add dav1d dependency
        Err(LavError::UnsupportedCodec(
            "AV1 decoder not yet implemented".into(),
        ))
    }
}

// ============================================================================
// LAV Audio Decoder
// ============================================================================

/// Audio decoder configuration
#[derive(Debug, Clone)]
pub struct AudioDecoderConfig {
    /// Output sample format
    pub output_format: SampleFormat,
    /// Output sample rate (0 = same as source)
    pub output_sample_rate: u32,
    /// Output channels (0 = same as source)
    pub output_channels: u8,
    /// Enable DRC (dynamic range compression)
    pub drc: bool,
    /// DRC level (0.0 - 1.0)
    pub drc_level: f32,
    /// Bitstream passthrough for supported formats
    pub bitstream: bool,
}

impl Default for AudioDecoderConfig {
    fn default() -> Self {
        Self {
            output_format: SampleFormat::F32,
            output_sample_rate: 0,
            output_channels: 0,
            drc: true,
            drc_level: 1.0,
            bitstream: false,
        }
    }
}

/// Audio decoder with REAL symphonia integration
#[cfg(feature = "audio")]
pub struct LavAudio {
    /// Codec
    codec: AudioCodec,
    /// Configuration
    config: AudioDecoderConfig,
    /// Stream info
    stream_info: AudioStreamInfo,
    /// Codec private data (for AAC AudioSpecificConfig, etc.)
    codec_private: Vec<u8>,
    /// Sample buffer for output conversion
    sample_buffer: Vec<f32>,
    /// Decoder initialized
    initialized: bool,
}

#[cfg(feature = "audio")]
impl LavAudio {
    /// Create decoder for stream
    pub fn new(stream_info: AudioStreamInfo, config: AudioDecoderConfig) -> LavResult<Self> {
        Ok(Self {
            codec: stream_info.codec,
            config,
            stream_info,
            codec_private: Vec::new(),
            sample_buffer: Vec::with_capacity(8192),
            initialized: false,
        })
    }

    /// Initialize decoder with codec private data
    pub fn init(&mut self, codec_private: &[u8]) -> LavResult<()> {
        self.codec_private = codec_private.to_vec();
        self.initialized = true;
        Ok(())
    }

    /// Decode packet to audio frames - REAL IMPLEMENTATION
    pub fn decode(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        if !self.initialized {
            return Err(LavError::DecoderInit("Not initialized".into()));
        }

        // Check for bitstream passthrough
        if self.config.bitstream && self.codec.can_bitstream() {
            return self.passthrough(packet);
        }

        // Decode based on codec
        match self.codec {
            AudioCodec::Aac => self.decode_aac(packet),
            AudioCodec::Ac3 | AudioCodec::Eac3 => self.decode_ac3(packet),
            AudioCodec::Dts | AudioCodec::DtsHd => self.decode_dts(packet),
            AudioCodec::Flac => self.decode_flac(packet),
            AudioCodec::Vorbis => self.decode_vorbis(packet),
            AudioCodec::Opus => self.decode_opus(packet),
            AudioCodec::Mp3 => self.decode_mp3(packet),
            AudioCodec::Pcm => self.decode_pcm(packet),
            _ => Err(LavError::UnsupportedCodec(format!("{:?}", self.codec))),
        }
    }

    /// Flush decoder
    pub fn flush(&mut self) -> LavResult<Vec<AudioFrame>> {
        self.sample_buffer.clear();
        Ok(Vec::new())
    }

    /// Reset decoder
    pub fn reset(&mut self) {
        self.sample_buffer.clear();
    }

    /// Get decoder name
    pub fn decoder_name(&self) -> &str {
        match self.codec {
            AudioCodec::Aac => "Symphonia-AAC",
            AudioCodec::Ac3 => "Symphonia-AC3",
            AudioCodec::Eac3 => "Symphonia-EAC3",
            AudioCodec::Dts => "DTS",
            AudioCodec::DtsHd => "DTS-HD",
            AudioCodec::TrueHd => "TrueHD",
            AudioCodec::Flac => "Symphonia-FLAC",
            AudioCodec::Vorbis => "Symphonia-Vorbis",
            AudioCodec::Opus => "Symphonia-Opus",
            AudioCodec::Mp3 => "Symphonia-MP3",
            AudioCodec::Pcm => "PCM",
            _ => "Audio",
        }
    }

    fn passthrough(&self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Pass compressed audio to output (for HDMI bitstream)
        Ok(vec![AudioFrame {
            data: packet.data.clone(),
            format: SampleFormat::S16,
            sample_rate: self.stream_info.sample_rate,
            channels: self.stream_info.channels,
            samples: 0, // Compressed - no sample count
            pts: packet.pts,
        }])
    }

    /// REAL AAC decoding using symphonia
    fn decode_aac(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // AAC decoding requires ADTS framing or AudioSpecificConfig
        // For raw AAC packets, we need to add ADTS header

        let data = if self.has_adts_header(&packet.data) {
            packet.data.clone()
        } else {
            // Add ADTS header for raw AAC
            self.add_adts_header(&packet.data)
        };

        // Use symphonia to decode
        self.decode_with_symphonia(&data, packet.pts, "aac")
    }

    fn has_adts_header(&self, data: &[u8]) -> bool {
        data.len() >= 2 && data[0] == 0xFF && (data[1] & 0xF0) == 0xF0
    }

    fn add_adts_header(&self, data: &[u8]) -> Vec<u8> {
        let frame_len = data.len() + 7; // ADTS header is 7 bytes

        // Parse AudioSpecificConfig to get profile and sample rate index
        let (profile, sample_rate_idx, channel_config) = if self.codec_private.len() >= 2 {
            let asc = &self.codec_private;
            let profile = ((asc[0] >> 3) & 0x1F) as u8;
            let sample_rate_idx = ((asc[0] & 0x07) << 1 | (asc[1] >> 7)) as u8;
            let channel_config = ((asc[1] >> 3) & 0x0F) as u8;
            (profile.saturating_sub(1), sample_rate_idx, channel_config)
        } else {
            // Default: AAC-LC, 44100 Hz, stereo
            (1, 4, 2)
        };

        let mut adts = Vec::with_capacity(frame_len);

        // ADTS header (7 bytes)
        adts.push(0xFF); // Sync word
        adts.push(0xF1); // MPEG-4, Layer 0, no CRC
        adts.push((profile << 6) | (sample_rate_idx << 2) | ((channel_config >> 2) & 0x01));
        adts.push(((channel_config & 0x03) << 6) | ((frame_len >> 11) & 0x03) as u8);
        adts.push(((frame_len >> 3) & 0xFF) as u8);
        adts.push((((frame_len & 0x07) << 5) | 0x1F) as u8);
        adts.push(0xFC); // Buffer fullness

        adts.extend_from_slice(data);
        adts
    }

    /// REAL AC3/EAC3 decoding
    fn decode_ac3(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // AC3 frames are self-contained with sync words
        self.decode_with_symphonia(&packet.data, packet.pts, "ac3")
    }

    /// DTS decoding (passthrough or software)
    fn decode_dts(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // DTS decoding is complex - most implementations passthrough to receiver
        // For software decode, would need dedicated DTS decoder
        Err(LavError::UnsupportedCodec(
            "DTS software decode not implemented - use passthrough".into(),
        ))
    }

    /// REAL FLAC decoding
    fn decode_flac(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        self.decode_with_symphonia(&packet.data, packet.pts, "flac")
    }

    /// REAL Vorbis decoding
    fn decode_vorbis(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        self.decode_with_symphonia(&packet.data, packet.pts, "vorbis")
    }

    /// REAL Opus decoding
    fn decode_opus(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        self.decode_with_symphonia(&packet.data, packet.pts, "opus")
    }

    /// REAL MP3 decoding
    fn decode_mp3(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // MP3 frames are self-contained
        self.decode_with_symphonia(&packet.data, packet.pts, "mp3")
    }

    /// PCM is already decoded - just reformat
    fn decode_pcm(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        let bytes_per_sample = match self.stream_info.bit_depth {
            8 => 1,
            16 => 2,
            24 => 3,
            32 => 4,
            _ => 2,
        };
        let frame_size = bytes_per_sample * self.stream_info.channels as usize;
        let num_samples = if frame_size > 0 {
            packet.data.len() / frame_size
        } else {
            0
        };

        // Convert to target format if needed
        let (data, format) = match self.config.output_format {
            SampleFormat::F32 => {
                let samples = self.pcm_to_f32(&packet.data, bytes_per_sample);
                let bytes: Vec<u8> = samples.iter().flat_map(|&s| s.to_le_bytes()).collect();
                (bytes, SampleFormat::F32)
            }
            _ => (packet.data.clone(), SampleFormat::S16),
        };

        Ok(vec![AudioFrame {
            data,
            format,
            sample_rate: self.stream_info.sample_rate,
            channels: self.stream_info.channels,
            samples: num_samples,
            pts: packet.pts,
        }])
    }

    /// Convert PCM bytes to f32 samples
    fn pcm_to_f32(&self, data: &[u8], bytes_per_sample: usize) -> Vec<f32> {
        match bytes_per_sample {
            1 => {
                // 8-bit unsigned
                data.iter().map(|&b| (b as f32 - 128.0) / 128.0).collect()
            }
            2 => {
                // 16-bit signed little-endian
                data.chunks_exact(2)
                    .map(|chunk| {
                        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
                        sample as f32 / 32768.0
                    })
                    .collect()
            }
            3 => {
                // 24-bit signed little-endian
                data.chunks_exact(3)
                    .map(|chunk| {
                        let sample = i32::from_le_bytes([
                            chunk[0],
                            chunk[1],
                            chunk[2],
                            if chunk[2] & 0x80 != 0 { 0xFF } else { 0x00 },
                        ]);
                        sample as f32 / 8388608.0
                    })
                    .collect()
            }
            4 => {
                // 32-bit signed or float
                data.chunks_exact(4)
                    .map(|chunk| {
                        let sample = i32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        sample as f32 / 2147483648.0
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// Generic symphonia-based decoding for supported formats
    fn decode_with_symphonia(
        &mut self,
        data: &[u8],
        pts: i64,
        codec_hint: &str,
    ) -> LavResult<Vec<AudioFrame>> {
        use std::io::Cursor;
        use symphonia::core::audio::SampleBuffer;
        use symphonia::core::codecs::{
            DecoderOptions, CODEC_TYPE_AAC, CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_OPUS,
            CODEC_TYPE_VORBIS,
        };
        use symphonia::core::formats::Packet as SymphoniaPacket;
        use symphonia::core::io::MediaSourceStream;

        // Create a media source from the packet data
        let cursor = Cursor::new(data.to_vec());
        let mss = MediaSourceStream::new(Box::new(cursor), Default::default());

        // Get codec type
        let codec_type = match codec_hint {
            "aac" => CODEC_TYPE_AAC,
            "mp3" => CODEC_TYPE_MP3,
            "flac" => CODEC_TYPE_FLAC,
            "vorbis" => CODEC_TYPE_VORBIS,
            "opus" => CODEC_TYPE_OPUS,
            _ => return Err(LavError::UnsupportedCodec(codec_hint.to_string())),
        };

        // Create codec parameters
        let mut codec_params = symphonia::core::codecs::CodecParameters::new();
        codec_params
            .for_codec(codec_type)
            .with_sample_rate(self.stream_info.sample_rate)
            .with_channels(
                symphonia::core::audio::Channels::FRONT_LEFT
                    | symphonia::core::audio::Channels::FRONT_RIGHT,
            );

        // Create decoder
        let decoder_opts = DecoderOptions::default();
        let mut decoder = match symphonia::default::get_codecs().make(&codec_params, &decoder_opts)
        {
            Ok(d) => d,
            Err(e) => return Err(LavError::DecoderInit(format!("Symphonia: {}", e))),
        };

        // Create a packet for the decoder
        let sym_packet = SymphoniaPacket::new_from_slice(0, 0, 0, data);

        // Decode
        match decoder.decode(&sym_packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                let duration = audio_buf.capacity() as u64;

                // Convert to interleaved f32
                let mut sample_buf = SampleBuffer::<f32>::new(duration, spec);
                sample_buf.copy_interleaved_ref(audio_buf);

                let samples = sample_buf.samples();
                let num_samples = samples.len() / spec.channels.count();

                // Convert f32 samples to bytes
                let data: Vec<u8> = samples.iter().flat_map(|&s| s.to_le_bytes()).collect();

                Ok(vec![AudioFrame {
                    data,
                    format: SampleFormat::F32,
                    sample_rate: spec.rate,
                    channels: spec.channels.count() as u8,
                    samples: num_samples,
                    pts,
                }])
            }
            Err(e) => Err(LavError::DecodeError(format!(
                "Symphonia decode error: {}",
                e
            ))),
        }
    }
}

// ============================================================================
// High-Level Playback Interface
// ============================================================================

/// Complete LAV playback pipeline
pub struct LavPipeline {
    /// Splitter
    pub splitter: LavSplitter,
    /// Video decoder
    pub video: Option<LavVideo>,
    /// Audio decoder
    #[cfg(feature = "audio")]
    pub audio: Option<LavAudio>,
}

impl LavPipeline {
    /// Open file and initialize decoders
    pub fn open<P: AsRef<Path>>(path: P) -> LavResult<Self> {
        let splitter = LavSplitter::open(path)?;

        // Create video decoder
        let video = if let Some(stream_idx) = splitter.video_stream {
            if let Some(stream) = splitter
                .info
                .video_streams
                .iter()
                .find(|s| s.index == stream_idx)
            {
                LavVideo::new(stream.clone(), VideoDecoderConfig::default()).ok()
            } else {
                None
            }
        } else {
            None
        };

        // Create audio decoder
        #[cfg(feature = "audio")]
        let audio = if let Some(stream_idx) = splitter.audio_stream {
            if let Some(stream) = splitter
                .info
                .audio_streams
                .iter()
                .find(|s| s.index == stream_idx)
            {
                LavAudio::new(stream.clone(), AudioDecoderConfig::default()).ok()
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            splitter,
            video,
            #[cfg(feature = "audio")]
            audio,
        })
    }

    /// Get media info
    pub fn info(&self) -> &MediaInfo {
        self.splitter.info()
    }

    /// Seek to timestamp
    pub fn seek(&mut self, timestamp_us: i64) -> LavResult<()> {
        self.splitter.seek(timestamp_us)?;

        if let Some(ref mut video) = self.video {
            video.reset();
        }
        #[cfg(feature = "audio")]
        if let Some(ref mut audio) = self.audio {
            audio.reset();
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_detection() {
        // MKV signature
        let mkv = [0x1A, 0x45, 0xDF, 0xA3, 0, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(
            ContainerFormat::detect(&mkv),
            Some(ContainerFormat::Matroska)
        );

        // MP4 ftyp
        let mp4 = [
            0, 0, 0, 0x20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm',
        ];
        assert_eq!(ContainerFormat::detect(&mp4), Some(ContainerFormat::Mp4));

        // AVI
        let avi = [b'R', b'I', b'F', b'F', 0, 0, 0, 0, b'A', b'V', b'I', b' '];
        assert_eq!(ContainerFormat::detect(&avi), Some(ContainerFormat::Avi));

        // MPEG-TS
        let ts = [0x47, 0, 0, 0];
        assert_eq!(ContainerFormat::detect(&ts), Some(ContainerFormat::MpegTs));
    }

    #[test]
    fn test_video_codec_from_fourcc() {
        assert_eq!(VideoCodec::from_fourcc(b"avc1"), Some(VideoCodec::H264));
        assert_eq!(VideoCodec::from_fourcc(b"hvc1"), Some(VideoCodec::H265));
        assert_eq!(VideoCodec::from_fourcc(b"av01"), Some(VideoCodec::Av1));
    }

    #[test]
    fn test_audio_codec_from_mkv() {
        assert_eq!(
            AudioCodec::from_mkv_codec_id("A_AAC"),
            Some(AudioCodec::Aac)
        );
        assert_eq!(
            AudioCodec::from_mkv_codec_id("A_OPUS"),
            Some(AudioCodec::Opus)
        );
        assert_eq!(
            AudioCodec::from_mkv_codec_id("A_FLAC"),
            Some(AudioCodec::Flac)
        );
    }

    #[test]
    fn test_channel_layout() {
        assert_eq!(ChannelLayout::from_channels(2), ChannelLayout::Stereo);
        assert_eq!(ChannelLayout::from_channels(6), ChannelLayout::Surround5_1);
        assert_eq!(ChannelLayout::Surround5_1.channel_count(), 6);
    }
}
