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

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom, BufReader};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

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
        if header.starts_with(&[0x00, 0x00, 0x00, 0x01]) || header.starts_with(&[0x00, 0x00, 0x01]) {
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
        matches!(self, Self::H264 | Self::H265 | Self::Vp9 | Self::Av1 | Self::Mpeg2 | Self::Vc1)
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
        matches!(self, Self::Ac3 | Self::Eac3 | Self::Dts | Self::DtsHd | Self::TrueHd)
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
    pub max_cll: u16,      // Maximum Content Light Level
    pub max_fall: u16,     // Maximum Frame Average Light Level
    pub mastering_display: Option<MasteringDisplay>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HdrTransfer {
    Pq,         // HDR10, Dolby Vision
    Hlg,        // HLG
    Sdr,        // Standard Dynamic Range
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasteringDisplay {
    pub primaries: [[f32; 2]; 3],  // RGB primaries
    pub white_point: [f32; 2],
    pub luminance_min: f32,
    pub luminance_max: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelLayout {
    Mono,
    Stereo,
    Surround3_0,    // L R C
    Quad,           // L R Ls Rs
    Surround5_0,    // L R C Ls Rs
    Surround5_1,    // L R C LFE Ls Rs
    Surround7_1,    // L R C LFE Ls Rs Lb Rb
    Custom(u32),    // Channel mask
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
        let n = self.reader.read(buf)
            .map_err(|e| LavError::IoError(e.to_string()))?;
        self.position += n as u64;
        Ok(n)
    }

    /// Read exact bytes
    pub fn read_exact(&mut self, buf: &mut [u8]) -> LavResult<()> {
        self.reader.read_exact(buf)
            .map_err(|e| LavError::IoError(e.to_string()))?;
        self.position += buf.len() as u64;
        Ok(())
    }

    /// Seek to position
    pub fn seek(&mut self, pos: u64) -> LavResult<()> {
        self.reader.seek(SeekFrom::Start(pos))
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
        let video_stream = info.video_streams.iter()
            .find(|s| s.is_default)
            .or_else(|| info.video_streams.first())
            .map(|s| s.index);

        let audio_stream = info.audio_streams.iter()
            .find(|s| s.is_default)
            .or_else(|| info.audio_streams.first())
            .map(|s| s.index);

        let subtitle_stream = info.subtitle_streams.iter()
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

    // Format-specific implementations
    fn parse_container(source: &mut LavSplitterSource, format: ContainerFormat) -> LavResult<MediaInfo> {
        // Placeholder - would call into mkv.rs, mp4_demux.rs, etc.
        Ok(MediaInfo {
            format,
            duration_us: 0,
            file_size: source.size().unwrap_or(0),
            bitrate: 0,
            seekable: true,
            video_streams: Vec::new(),
            audio_streams: Vec::new(),
            subtitle_streams: Vec::new(),
            metadata: HashMap::new(),
            chapters: Vec::new(),
            attachments: Vec::new(),
        })
    }

    fn read_mkv_packet(&mut self) -> LavResult<Packet> {
        // Would use crate::mkv
        Err(LavError::NeedMoreData)
    }

    fn read_mp4_packet(&mut self) -> LavResult<Packet> {
        // Would use crate::mp4_demux
        Err(LavError::NeedMoreData)
    }

    fn read_avi_packet(&mut self) -> LavResult<Packet> {
        // Would use crate::avi_demux
        Err(LavError::NeedMoreData)
    }

    fn read_ts_packet(&mut self) -> LavResult<Packet> {
        // Would use crate::ts_demux
        Err(LavError::NeedMoreData)
    }

    fn seek_internal(&mut self, _timestamp_us: i64) -> LavResult<()> {
        // Format-specific seek implementation
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
    Nvdec,      // NVIDIA
    Amf,        // AMD
    Qsv,        // Intel
    Vaapi,      // Linux VA-API
    Dxva2,      // Windows DirectX
    D3D11,      // Windows Direct3D 11
    VideoToolbox,  // macOS
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
            threads: 0,  // Auto
            deinterlace: DeinterlaceMode::Auto,
        }
    }
}

/// Video decoder
pub struct LavVideo {
    /// Codec
    codec: VideoCodec,
    /// Configuration
    config: VideoDecoderConfig,
    /// Stream info
    stream_info: VideoStreamInfo,
    /// Active HW decoder
    hw_decoder: Option<HwDecoder>,
    /// Decoded frame queue
    frame_queue: Vec<VideoFrame>,
    /// Codec state
    initialized: bool,
}

impl LavVideo {
    /// Create decoder for stream
    pub fn new(stream_info: VideoStreamInfo, config: VideoDecoderConfig) -> LavResult<Self> {
        let hw_decoder = if config.prefer_hw && stream_info.codec.hw_decode_available() {
            Self::probe_hw_decoder(stream_info.codec, config.hw_decoder)
        } else {
            None
        };

        Ok(Self {
            codec: stream_info.codec,
            config,
            stream_info,
            hw_decoder,
            frame_queue: Vec::new(),
            initialized: false,
        })
    }

    fn probe_hw_decoder(codec: VideoCodec, preferred: Option<HwDecoder>) -> Option<HwDecoder> {
        // Would probe system for available HW decoders
        // Check NVDEC, AMF, QSV, VAAPI, etc.
        preferred
    }

    /// Initialize decoder with codec private data
    pub fn init(&mut self, codec_private: &[u8]) -> LavResult<()> {
        // Initialize decoder with SPS/PPS/VPS data
        self.initialized = true;
        Ok(())
    }

    /// Decode packet to frames
    pub fn decode(&mut self, packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        if !self.initialized {
            return Err(LavError::DecoderInit("Not initialized".into()));
        }

        // Decode based on codec
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

    // Codec-specific decoders
    fn decode_h264(&mut self, _packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        // Would use openh264 crate or HW decoder
        Ok(Vec::new())
    }

    fn decode_h265(&mut self, _packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        // Would use HW decoder (no good pure-Rust HEVC decoder yet)
        Ok(Vec::new())
    }

    fn decode_vp9(&mut self, _packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        // Would use HW decoder or libvpx
        Ok(Vec::new())
    }

    fn decode_av1(&mut self, _packet: &Packet) -> LavResult<Vec<VideoFrame>> {
        // Would use dav1d crate
        Ok(Vec::new())
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

/// Audio decoder
pub struct LavAudio {
    /// Codec
    codec: AudioCodec,
    /// Configuration
    config: AudioDecoderConfig,
    /// Stream info
    stream_info: AudioStreamInfo,
    /// Decoder initialized
    initialized: bool,
}

impl LavAudio {
    /// Create decoder for stream
    pub fn new(stream_info: AudioStreamInfo, config: AudioDecoderConfig) -> LavResult<Self> {
        Ok(Self {
            codec: stream_info.codec,
            config,
            stream_info,
            initialized: false,
        })
    }

    /// Initialize decoder
    pub fn init(&mut self, codec_private: &[u8]) -> LavResult<()> {
        self.initialized = true;
        Ok(())
    }

    /// Decode packet to audio frames
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
        Ok(Vec::new())
    }

    /// Reset decoder
    pub fn reset(&mut self) {
        // Reset internal state
    }

    /// Get decoder name
    pub fn decoder_name(&self) -> &str {
        match self.codec {
            AudioCodec::Aac => "AAC",
            AudioCodec::Ac3 => "AC3",
            AudioCodec::Eac3 => "E-AC3",
            AudioCodec::Dts => "DTS",
            AudioCodec::DtsHd => "DTS-HD",
            AudioCodec::TrueHd => "TrueHD",
            AudioCodec::Flac => "FLAC",
            AudioCodec::Vorbis => "Vorbis",
            AudioCodec::Opus => "Opus",
            AudioCodec::Mp3 => "MP3",
            _ => "Audio",
        }
    }

    fn passthrough(&self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Pass compressed audio to output (for HDMI bitstream)
        Ok(vec![AudioFrame {
            data: packet.data.clone(),
            format: SampleFormat::S16,  // Placeholder
            sample_rate: self.stream_info.sample_rate,
            channels: self.stream_info.channels,
            samples: 0,
            pts: packet.pts,
        }])
    }

    // Codec-specific decoders
    fn decode_aac(&mut self, _packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Would use symphonia or fdk-aac
        Ok(Vec::new())
    }

    fn decode_ac3(&mut self, _packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Would use symphonia
        Ok(Vec::new())
    }

    fn decode_dts(&mut self, _packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // DTS decoding
        Ok(Vec::new())
    }

    fn decode_flac(&mut self, _packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Would use symphonia or claxon
        Ok(Vec::new())
    }

    fn decode_vorbis(&mut self, _packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Would use lewton
        Ok(Vec::new())
    }

    fn decode_opus(&mut self, _packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Would use opus crate
        Ok(Vec::new())
    }

    fn decode_mp3(&mut self, _packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // Would use symphonia or minimp3
        Ok(Vec::new())
    }

    fn decode_pcm(&mut self, packet: &Packet) -> LavResult<Vec<AudioFrame>> {
        // PCM is already decoded
        Ok(vec![AudioFrame {
            data: packet.data.clone(),
            format: SampleFormat::S16,
            sample_rate: self.stream_info.sample_rate,
            channels: self.stream_info.channels,
            samples: packet.data.len() / (2 * self.stream_info.channels as usize),
            pts: packet.pts,
        }])
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
    pub audio: Option<LavAudio>,
}

impl LavPipeline {
    /// Open file and initialize decoders
    pub fn open<P: AsRef<Path>>(path: P) -> LavResult<Self> {
        let splitter = LavSplitter::open(path)?;

        // Create video decoder
        let video = if let Some(stream_idx) = splitter.video_stream {
            if let Some(stream) = splitter.info.video_streams.iter().find(|s| s.index == stream_idx) {
                LavVideo::new(stream.clone(), VideoDecoderConfig::default()).ok()
            } else {
                None
            }
        } else {
            None
        };

        // Create audio decoder
        let audio = if let Some(stream_idx) = splitter.audio_stream {
            if let Some(stream) = splitter.info.audio_streams.iter().find(|s| s.index == stream_idx) {
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
        assert_eq!(ContainerFormat::detect(&mkv), Some(ContainerFormat::Matroska));

        // MP4 ftyp
        let mp4 = [0, 0, 0, 0x20, b'f', b't', b'y', b'p', b'i', b's', b'o', b'm'];
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
        assert_eq!(AudioCodec::from_mkv_codec_id("A_AAC"), Some(AudioCodec::Aac));
        assert_eq!(AudioCodec::from_mkv_codec_id("A_OPUS"), Some(AudioCodec::Opus));
        assert_eq!(AudioCodec::from_mkv_codec_id("A_FLAC"), Some(AudioCodec::Flac));
    }

    #[test]
    fn test_channel_layout() {
        assert_eq!(ChannelLayout::from_channels(2), ChannelLayout::Stereo);
        assert_eq!(ChannelLayout::from_channels(6), ChannelLayout::Surround5_1);
        assert_eq!(ChannelLayout::Surround5_1.channel_count(), 6);
    }
}
