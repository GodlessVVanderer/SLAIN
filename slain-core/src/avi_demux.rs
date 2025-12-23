// AVI DEMUXER - Pure Rust AVI/RIFF Parser
//
// AVI is Microsoft's container from 1992.
// Still common for legacy content and screen recordings.
// Simple RIFF structure - easier than MP4.

use std::io::{Read, Seek, SeekFrom};
use serde::{Deserialize, Serialize};

// ============================================================================
// RIFF/AVI Constants
// ============================================================================

const RIFF: u32 = 0x46464952;  // "RIFF" little-endian
const AVI_: u32 = 0x20495641;  // "AVI " little-endian
const LIST: u32 = 0x5453494C;  // "LIST" little-endian
const HDRL: u32 = 0x6C726468;  // "hdrl" - header list
const AVIH: u32 = 0x68697661;  // "avih" - main AVI header
const STRL: u32 = 0x6C727473;  // "strl" - stream list
const STRH: u32 = 0x68727473;  // "strh" - stream header
const STRF: u32 = 0x66727473;  // "strf" - stream format
const STRN: u32 = 0x6E727473;  // "strn" - stream name
const MOVI: u32 = 0x69766F6D;  // "movi" - movie data
const IDX1: u32 = 0x31786469;  // "idx1" - index

// Stream types
const VIDS: u32 = 0x73646976;  // "vids" - video stream
const AUDS: u32 = 0x73647561;  // "auds" - audio stream
const TXTS: u32 = 0x73747874;  // "txts" - subtitle stream

// Video codecs (FourCC)
const XVID: u32 = 0x44495658;
const DIVX: u32 = 0x58564944;
const DX50: u32 = 0x30355844;
const H264: u32 = 0x34363248;
const AVC1: u32 = 0x31435641;
const X264: u32 = 0x34363278;
const MJPG: u32 = 0x47504A4D;
const I420: u32 = 0x30323449;  // Raw YUV
const YV12: u32 = 0x32315659;
const CVID: u32 = 0x64697663;  // Cinepak
const IV50: u32 = 0x30355649;  // Intel Indeo 5

// ============================================================================
// Structures
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AviInfo {
    pub duration_us: i64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub total_frames: u32,
    pub streams: Vec<AviStream>,
    pub has_index: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AviStream {
    pub index: u32,
    pub stream_type: StreamType,
    pub codec_fourcc: String,
    pub codec: CodecType,
    pub language: Option<String>,
    pub name: Option<String>,
    // Video-specific
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    pub bit_depth: Option<u8>,
    // Audio-specific
    pub sample_rate: Option<u32>,
    pub channels: Option<u8>,
    pub bits_per_sample: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamType {
    Video,
    Audio,
    Subtitle,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodecType {
    // Video
    H264,
    MPEG4,    // DivX/Xvid
    MJPEG,
    RawYUV,
    Cinepak,
    Indeo,
    // Audio
    PCM,
    MP3,
    AC3,
    AAC,
    // Unknown
    Unknown,
}

#[derive(Debug, Clone)]
pub struct AviPacket {
    pub stream_index: u32,
    pub pts: i64,
    pub dts: i64,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone)]
struct IndexEntry {
    stream_id: u16,
    flags: u32,
    offset: u64,
    size: u32,
}

// ============================================================================
// Main Header (AVIH)
// ============================================================================

#[derive(Debug, Clone, Default)]
struct MainHeader {
    microsec_per_frame: u32,
    max_bytes_per_sec: u32,
    padding_granularity: u32,
    flags: u32,
    total_frames: u32,
    initial_frames: u32,
    streams: u32,
    suggested_buffer_size: u32,
    width: u32,
    height: u32,
}

// ============================================================================
// Stream Header (STRH)
// ============================================================================

#[derive(Debug, Clone, Default)]
struct StreamHeader {
    fcc_type: u32,      // vids, auds, txts
    fcc_handler: u32,   // Codec FourCC
    flags: u32,
    priority: u16,
    language: u16,
    initial_frames: u32,
    scale: u32,
    rate: u32,
    start: u32,
    length: u32,
    suggested_buffer_size: u32,
    quality: u32,
    sample_size: u32,
    // rcFrame rectangle (ignored)
}

// ============================================================================
// AVI Demuxer
// ============================================================================

pub struct AviDemuxer<R: Read + Seek> {
    reader: R,
    info: AviInfo,
    streams: Vec<StreamHeader>,
    movi_offset: u64,
    movi_size: u64,
    index: Vec<IndexEntry>,
    current_position: u64,
    frame_counts: Vec<u32>,
}

impl<R: Read + Seek> AviDemuxer<R> {
    pub fn new(mut reader: R) -> Result<Self, String> {
        // Verify RIFF header
        let riff = read_u32_le(&mut reader)?;
        if riff != RIFF {
            return Err("Not a RIFF file".to_string());
        }
        
        let _file_size = read_u32_le(&mut reader)?;
        
        let avi = read_u32_le(&mut reader)?;
        if avi != AVI_ {
            return Err("Not an AVI file".to_string());
        }
        
        let mut demuxer = Self {
            reader,
            info: AviInfo {
                duration_us: 0,
                width: 0,
                height: 0,
                fps: 0.0,
                total_frames: 0,
                streams: Vec::new(),
                has_index: false,
            },
            streams: Vec::new(),
            movi_offset: 0,
            movi_size: 0,
            index: Vec::new(),
            current_position: 0,
            frame_counts: Vec::new(),
        };
        
        demuxer.parse_chunks()?;
        demuxer.build_info();
        
        Ok(demuxer)
    }
    
    fn parse_chunks(&mut self) -> Result<(), String> {
        let file_size = self.reader.seek(SeekFrom::End(0))
            .map_err(|e| format!("Seek error: {}", e))?;
        self.reader.seek(SeekFrom::Start(12))
            .map_err(|e| format!("Seek error: {}", e))?;
        
        while self.reader.stream_position().unwrap_or(file_size) < file_size - 8 {
            let fourcc = read_u32_le(&mut self.reader)?;
            let size = read_u32_le(&mut self.reader)?;
            
            match fourcc {
                LIST => {
                    let list_type = read_u32_le(&mut self.reader)?;
                    match list_type {
                        HDRL => self.parse_hdrl(size - 4)?,
                        MOVI => {
                            self.movi_offset = self.reader.stream_position()
                                .map_err(|e| format!("Position error: {}", e))?;
                            self.movi_size = (size - 4) as u64;
                            self.skip(size - 4)?;
                        }
                        _ => self.skip(size - 4)?,
                    }
                }
                IDX1 => {
                    self.parse_idx1(size)?;
                }
                _ => {
                    self.skip(size)?;
                }
            }
            
            // Align to word boundary
            if size % 2 == 1 {
                self.skip(1)?;
            }
        }
        
        Ok(())
    }
    
    fn parse_hdrl(&mut self, size: u32) -> Result<(), String> {
        let end = self.reader.stream_position().unwrap() + size as u64;
        
        while self.reader.stream_position().unwrap() < end {
            let fourcc = read_u32_le(&mut self.reader)?;
            let chunk_size = read_u32_le(&mut self.reader)?;
            
            match fourcc {
                AVIH => {
                    self.parse_avih(chunk_size)?;
                }
                LIST => {
                    let list_type = read_u32_le(&mut self.reader)?;
                    if list_type == STRL {
                        self.parse_strl(chunk_size - 4)?;
                    } else {
                        self.skip(chunk_size - 4)?;
                    }
                }
                _ => {
                    self.skip(chunk_size)?;
                }
            }
            
            if chunk_size % 2 == 1 {
                self.skip(1)?;
            }
        }
        
        Ok(())
    }
    
    fn parse_avih(&mut self, _size: u32) -> Result<(), String> {
        let header = MainHeader {
            microsec_per_frame: read_u32_le(&mut self.reader)?,
            max_bytes_per_sec: read_u32_le(&mut self.reader)?,
            padding_granularity: read_u32_le(&mut self.reader)?,
            flags: read_u32_le(&mut self.reader)?,
            total_frames: read_u32_le(&mut self.reader)?,
            initial_frames: read_u32_le(&mut self.reader)?,
            streams: read_u32_le(&mut self.reader)?,
            suggested_buffer_size: read_u32_le(&mut self.reader)?,
            width: read_u32_le(&mut self.reader)?,
            height: read_u32_le(&mut self.reader)?,
        };
        
        self.info.width = header.width;
        self.info.height = header.height;
        self.info.total_frames = header.total_frames;
        
        if header.microsec_per_frame > 0 {
            self.info.fps = 1_000_000.0 / header.microsec_per_frame as f64;
            self.info.duration_us = (header.total_frames as i64) * (header.microsec_per_frame as i64);
        }
        
        // Skip reserved fields
        self.skip(16)?;
        
        Ok(())
    }
    
    fn parse_strl(&mut self, size: u32) -> Result<(), String> {
        let end = self.reader.stream_position().unwrap() + size as u64;
        let mut stream_header = StreamHeader::default();
        let mut stream_info = AviStream {
            index: self.streams.len() as u32,
            stream_type: StreamType::Unknown,
            codec_fourcc: String::new(),
            codec: CodecType::Unknown,
            language: None,
            name: None,
            width: None,
            height: None,
            fps: None,
            bit_depth: None,
            sample_rate: None,
            channels: None,
            bits_per_sample: None,
        };
        
        while self.reader.stream_position().unwrap() < end {
            let fourcc = read_u32_le(&mut self.reader)?;
            let chunk_size = read_u32_le(&mut self.reader)?;
            
            match fourcc {
                STRH => {
                    stream_header = self.read_strh()?;
                    stream_info.stream_type = match stream_header.fcc_type {
                        VIDS => StreamType::Video,
                        AUDS => StreamType::Audio,
                        TXTS => StreamType::Subtitle,
                        _ => StreamType::Unknown,
                    };
                    stream_info.codec_fourcc = fourcc_to_string(stream_header.fcc_handler);
                    stream_info.codec = identify_codec(stream_header.fcc_handler, stream_info.stream_type);
                    
                    if stream_header.scale > 0 && stream_header.rate > 0 {
                        stream_info.fps = Some(stream_header.rate as f64 / stream_header.scale as f64);
                    }
                    
                    // Skip rest of strh if any
                    let read = 56;  // Size of StreamHeader we read
                    if chunk_size > read {
                        self.skip(chunk_size - read)?;
                    }
                }
                STRF => {
                    match stream_info.stream_type {
                        StreamType::Video => {
                            self.parse_video_format(&mut stream_info, chunk_size)?;
                        }
                        StreamType::Audio => {
                            self.parse_audio_format(&mut stream_info, chunk_size)?;
                        }
                        _ => {
                            self.skip(chunk_size)?;
                        }
                    }
                }
                STRN => {
                    let mut name = vec![0u8; chunk_size as usize];
                    self.reader.read_exact(&mut name).ok();
                    // Trim null terminator
                    if let Some(pos) = name.iter().position(|&b| b == 0) {
                        name.truncate(pos);
                    }
                    stream_info.name = String::from_utf8(name).ok();
                }
                _ => {
                    self.skip(chunk_size)?;
                }
            }
            
            if chunk_size % 2 == 1 {
                self.skip(1)?;
            }
        }
        
        self.streams.push(stream_header);
        self.info.streams.push(stream_info);
        self.frame_counts.push(0);
        
        Ok(())
    }
    
    fn read_strh(&mut self) -> Result<StreamHeader, String> {
        Ok(StreamHeader {
            fcc_type: read_u32_le(&mut self.reader)?,
            fcc_handler: read_u32_le(&mut self.reader)?,
            flags: read_u32_le(&mut self.reader)?,
            priority: read_u16_le(&mut self.reader)?,
            language: read_u16_le(&mut self.reader)?,
            initial_frames: read_u32_le(&mut self.reader)?,
            scale: read_u32_le(&mut self.reader)?,
            rate: read_u32_le(&mut self.reader)?,
            start: read_u32_le(&mut self.reader)?,
            length: read_u32_le(&mut self.reader)?,
            suggested_buffer_size: read_u32_le(&mut self.reader)?,
            quality: read_u32_le(&mut self.reader)?,
            sample_size: read_u32_le(&mut self.reader)?,
        })
    }
    
    fn parse_video_format(&mut self, stream: &mut AviStream, size: u32) -> Result<(), String> {
        // BITMAPINFOHEADER
        let _bih_size = read_u32_le(&mut self.reader)?;
        stream.width = Some(read_u32_le(&mut self.reader)? as u32);
        let height = read_u32_le(&mut self.reader)? as i32;
        stream.height = Some(height.unsigned_abs());
        let _planes = read_u16_le(&mut self.reader)?;
        stream.bit_depth = Some(read_u16_le(&mut self.reader)? as u8);
        let compression = read_u32_le(&mut self.reader)?;
        
        // Update codec from compression field if more specific
        if compression != 0 {
            stream.codec = identify_codec(compression, StreamType::Video);
            stream.codec_fourcc = fourcc_to_string(compression);
        }
        
        // Skip rest
        let read = 20;
        if size > read {
            self.skip(size - read)?;
        }
        
        Ok(())
    }
    
    fn parse_audio_format(&mut self, stream: &mut AviStream, size: u32) -> Result<(), String> {
        // WAVEFORMATEX
        let format_tag = read_u16_le(&mut self.reader)?;
        stream.channels = Some(read_u16_le(&mut self.reader)? as u8);
        stream.sample_rate = Some(read_u32_le(&mut self.reader)?);
        let _avg_bytes_per_sec = read_u32_le(&mut self.reader)?;
        let _block_align = read_u16_le(&mut self.reader)?;
        stream.bits_per_sample = Some(read_u16_le(&mut self.reader)?);
        
        stream.codec = match format_tag {
            0x0001 => CodecType::PCM,
            0x0055 => CodecType::MP3,
            0x2000 => CodecType::AC3,
            0x00FF => CodecType::AAC,
            _ => CodecType::Unknown,
        };
        
        // Skip rest (cbSize + extra data)
        let read = 16;
        if size > read {
            self.skip(size - read)?;
        }
        
        Ok(())
    }
    
    fn parse_idx1(&mut self, size: u32) -> Result<(), String> {
        self.info.has_index = true;
        let entries = size / 16;
        
        for _ in 0..entries {
            let chunk_id = read_u32_le(&mut self.reader)?;
            let flags = read_u32_le(&mut self.reader)?;
            let offset = read_u32_le(&mut self.reader)?;
            let chunk_size = read_u32_le(&mut self.reader)?;
            
            // Parse stream ID from chunk_id (e.g., "00dc" = video stream 0)
            let stream_id = ((chunk_id & 0xFF) - b'0' as u32) * 10 
                          + (((chunk_id >> 8) & 0xFF) - b'0' as u32);
            
            self.index.push(IndexEntry {
                stream_id: stream_id as u16,
                flags,
                offset: offset as u64,
                size: chunk_size,
            });
        }
        
        Ok(())
    }
    
    fn build_info(&mut self) {
        // Calculate duration from video stream if not set
        if self.info.duration_us == 0 {
            for (i, stream) in self.info.streams.iter().enumerate() {
                if stream.stream_type == StreamType::Video {
                    if let Some(fps) = stream.fps {
                        if fps > 0.0 {
                            let frames = self.streams.get(i)
                                .map(|s| s.length)
                                .unwrap_or(self.info.total_frames);
                            self.info.duration_us = (frames as f64 / fps * 1_000_000.0) as i64;
                            self.info.fps = fps;
                            break;
                        }
                    }
                }
            }
        }
    }
    
    fn skip(&mut self, n: u32) -> Result<(), String> {
        self.reader.seek(SeekFrom::Current(n as i64))
            .map_err(|e| format!("Seek error: {}", e))?;
        Ok(())
    }
    
    /// Get file info
    pub fn info(&self) -> &AviInfo {
        &self.info
    }
    
    /// Read next packet
    pub fn read_packet(&mut self) -> Option<AviPacket> {
        if self.index.is_empty() {
            // No index - read sequentially from movi
            self.read_packet_sequential()
        } else {
            // Use index
            self.read_packet_indexed()
        }
    }
    
    fn read_packet_indexed(&mut self) -> Option<AviPacket> {
        if self.current_position as usize >= self.index.len() {
            return None;
        }
        
        let entry = &self.index[self.current_position as usize];
        let stream_idx = entry.stream_id as usize;
        
        // Offset in idx1 is relative to movi start (after LIST/movi header)
        let abs_offset = self.movi_offset + entry.offset;
        
        self.reader.seek(SeekFrom::Start(abs_offset)).ok()?;
        
        // Read chunk header
        let _chunk_id = read_u32_le(&mut self.reader).ok()?;
        let chunk_size = read_u32_le(&mut self.reader).ok()?;
        
        // Read data
        let mut data = vec![0u8; chunk_size as usize];
        self.reader.read_exact(&mut data).ok()?;
        
        // Calculate PTS
        let frame_num = self.frame_counts.get(stream_idx).copied().unwrap_or(0);
        let pts = if let Some(stream) = self.info.streams.get(stream_idx) {
            if let Some(fps) = stream.fps {
                (frame_num as f64 / fps * 1_000_000.0) as i64
            } else {
                0
            }
        } else {
            0
        };
        
        // Update frame count
        if let Some(count) = self.frame_counts.get_mut(stream_idx) {
            *count += 1;
        }
        
        self.current_position += 1;
        
        Some(AviPacket {
            stream_index: stream_idx as u32,
            pts,
            dts: pts,
            keyframe: (entry.flags & 0x10) != 0,  // AVIIF_KEYFRAME
            data,
        })
    }
    
    fn read_packet_sequential(&mut self) -> Option<AviPacket> {
        if self.current_position == 0 {
            self.reader.seek(SeekFrom::Start(self.movi_offset)).ok()?;
        }
        
        let end = self.movi_offset + self.movi_size;
        
        loop {
            if self.reader.stream_position().ok()? >= end {
                return None;
            }
            
            let chunk_id = read_u32_le(&mut self.reader).ok()?;
            let chunk_size = read_u32_le(&mut self.reader).ok()?;
            
            // Check if this is a data chunk (##dc, ##db, ##wb, etc.)
            let b0 = (chunk_id & 0xFF) as u8;
            let b1 = ((chunk_id >> 8) & 0xFF) as u8;
            
            if b0.is_ascii_digit() && b1.is_ascii_digit() {
                let stream_idx = ((b0 - b'0') * 10 + (b1 - b'0')) as usize;
                
                let mut data = vec![0u8; chunk_size as usize];
                self.reader.read_exact(&mut data).ok()?;
                
                // Align
                if chunk_size % 2 == 1 {
                    self.reader.seek(SeekFrom::Current(1)).ok()?;
                }
                
                let frame_num = self.frame_counts.get(stream_idx).copied().unwrap_or(0);
                let pts = if let Some(stream) = self.info.streams.get(stream_idx) {
                    if let Some(fps) = stream.fps {
                        (frame_num as f64 / fps * 1_000_000.0) as i64
                    } else {
                        0
                    }
                } else {
                    0
                };
                
                if let Some(count) = self.frame_counts.get_mut(stream_idx) {
                    *count += 1;
                }
                
                self.current_position += 1;
                
                return Some(AviPacket {
                    stream_index: stream_idx as u32,
                    pts,
                    dts: pts,
                    keyframe: true,  // Can't know without parsing
                    data,
                });
            } else if chunk_id == LIST {
                // Skip LIST chunks in movi (rec lists, etc.)
                let _list_type = read_u32_le(&mut self.reader).ok()?;
                // Don't skip - the contents might be data chunks
            } else {
                // Skip unknown chunks
                self.reader.seek(SeekFrom::Current(chunk_size as i64)).ok()?;
                if chunk_size % 2 == 1 {
                    self.reader.seek(SeekFrom::Current(1)).ok()?;
                }
            }
        }
    }
    
    /// Seek to timestamp (microseconds)
    pub fn seek(&mut self, timestamp_us: i64) -> Result<(), String> {
        if self.index.is_empty() {
            return Err("Cannot seek without index".to_string());
        }
        
        // Find video stream
        let video_stream = self.info.streams.iter()
            .position(|s| s.stream_type == StreamType::Video);
        
        if let Some(stream_idx) = video_stream {
            let fps = self.info.streams[stream_idx].fps.unwrap_or(30.0);
            let target_frame = (timestamp_us as f64 / 1_000_000.0 * fps) as u32;
            
            // Find nearest keyframe at or before target
            let mut best_idx = 0;
            let mut best_frame = 0u32;
            let mut frame_count = 0u32;
            
            for (i, entry) in self.index.iter().enumerate() {
                if entry.stream_id as usize == stream_idx {
                    if (entry.flags & 0x10) != 0 {  // Keyframe
                        if frame_count <= target_frame {
                            best_idx = i;
                            best_frame = frame_count;
                        }
                    }
                    frame_count += 1;
                }
            }
            
            self.current_position = best_idx as u64;
            
            // Reset frame counts
            for count in &mut self.frame_counts {
                *count = 0;
            }
            self.frame_counts[stream_idx] = best_frame;
        }
        
        Ok(())
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn read_u16_le<R: Read>(reader: &mut R) -> Result<u16, String> {
    let mut buf = [0u8; 2];
    reader.read_exact(&mut buf).map_err(|e| format!("Read error: {}", e))?;
    Ok(u16::from_le_bytes(buf))
}

fn read_u32_le<R: Read>(reader: &mut R) -> Result<u32, String> {
    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf).map_err(|e| format!("Read error: {}", e))?;
    Ok(u32::from_le_bytes(buf))
}

fn fourcc_to_string(fourcc: u32) -> String {
    let bytes = fourcc.to_le_bytes();
    bytes.iter()
        .filter(|&&b| b.is_ascii_graphic() || b == b' ')
        .map(|&b| b as char)
        .collect()
}

fn identify_codec(fourcc: u32, stream_type: StreamType) -> CodecType {
    match stream_type {
        StreamType::Video => {
            let upper = fourcc.to_ascii_uppercase();
            match upper {
                H264 | AVC1 | X264 => CodecType::H264,
                XVID | DIVX | DX50 => CodecType::MPEG4,
                MJPG => CodecType::MJPEG,
                I420 | YV12 => CodecType::RawYUV,
                CVID => CodecType::Cinepak,
                IV50 => CodecType::Indeo,
                _ => {
                    // Check case-insensitive
                    let s = fourcc_to_string(fourcc).to_uppercase();
                    if s.contains("264") || s.contains("AVC") {
                        CodecType::H264
                    } else if s.contains("XVID") || s.contains("DIVX") || s.contains("MP4") {
                        CodecType::MPEG4
                    } else {
                        CodecType::Unknown
                    }
                }
            }
        }
        StreamType::Audio => CodecType::Unknown,  // Handled by format_tag
        _ => CodecType::Unknown,
    }
}

trait AsciiUppercase {
    fn to_ascii_uppercase(&self) -> Self;
}

impl AsciiUppercase for u32 {
    fn to_ascii_uppercase(&self) -> Self {
        let bytes = self.to_le_bytes();
        let upper: [u8; 4] = [
            bytes[0].to_ascii_uppercase(),
            bytes[1].to_ascii_uppercase(),
            bytes[2].to_ascii_uppercase(),
            bytes[3].to_ascii_uppercase(),
        ];
        u32::from_le_bytes(upper)
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================




pub fn avi_probe(path: String) -> Result<serde_json::Value, String> {
    use std::fs::File;
    
    let file = File::open(&path).map_err(|e| format!("Open error: {}", e))?;
    let demuxer = AviDemuxer::new(file)?;
    
    serde_json::to_value(demuxer.info()).map_err(|e| format!("JSON error: {}", e))
}


pub fn avi_description() -> String {
    r#"
AVI DEMUXER - Pure Rust RIFF/AVI Parser

Microsoft's AVI container from 1992.
Still common for legacy content.

SUPPORTED:
• Video: H.264, MPEG-4 (DivX/Xvid), MJPEG, raw YUV
• Audio: PCM, MP3, AC3, AAC
• Index seeking (idx1)

STRUCTURE:
RIFF 'AVI '
├── LIST 'hdrl'
│   ├── avih (main header)
│   └── LIST 'strl' (per stream)
│       ├── strh (stream header)
│       └── strf (stream format)
├── LIST 'movi'
│   ├── 00dc (video data)
│   └── 01wb (audio data)
└── idx1 (index)

Simple little-endian RIFF structure.
Much easier than ISO-BMFF (MP4).
"#.to_string()
}
