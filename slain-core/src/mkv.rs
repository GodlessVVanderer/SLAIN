// MKV (Matroska) container tools for SLAIN Video Player
// Handles track enumeration, chapters, metadata, attachments, and demuxing

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};


// ============================================================================
// EBML/Matroska Element IDs
// ============================================================================

mod element_ids {
    // EBML Header
    pub const EBML: u32 = 0x1A45DFA3;
    pub const EBML_VERSION: u32 = 0x4286;
    pub const EBML_READ_VERSION: u32 = 0x42F7;
    pub const EBML_MAX_ID_LENGTH: u32 = 0x42F2;
    pub const EBML_MAX_SIZE_LENGTH: u32 = 0x42F3;
    pub const DOC_TYPE: u32 = 0x4282;
    pub const DOC_TYPE_VERSION: u32 = 0x4287;
    pub const DOC_TYPE_READ_VERSION: u32 = 0x4285;

    // Segment
    pub const SEGMENT: u32 = 0x18538067;

    // Segment Information
    pub const SEGMENT_INFO: u32 = 0x1549A966;
    pub const SEGMENT_UID: u32 = 0x73A4;
    pub const SEGMENT_FILENAME: u32 = 0x7384;
    pub const TITLE: u32 = 0x7BA9;
    pub const MUXING_APP: u32 = 0x4D80;
    pub const WRITING_APP: u32 = 0x5741;
    pub const DURATION: u32 = 0x4489;
    pub const DATE_UTC: u32 = 0x4461;
    pub const TIMECODE_SCALE: u32 = 0x2AD7B1;

    // Tracks
    pub const TRACKS: u32 = 0x1654AE6B;
    pub const TRACK_ENTRY: u32 = 0xAE;
    pub const TRACK_NUMBER: u32 = 0xD7;
    pub const TRACK_UID: u32 = 0x73C5;
    pub const TRACK_TYPE: u32 = 0x83;
    pub const FLAG_ENABLED: u32 = 0xB9;
    pub const FLAG_DEFAULT: u32 = 0x88;
    pub const FLAG_FORCED: u32 = 0x55AA;
    pub const FLAG_LACING: u32 = 0x9C;
    pub const NAME: u32 = 0x536E;
    pub const LANGUAGE: u32 = 0x22B59C;
    pub const LANGUAGE_BCP47: u32 = 0x22B59D;
    pub const CODEC_ID: u32 = 0x86;
    pub const CODEC_PRIVATE: u32 = 0x63A2;
    pub const CODEC_NAME: u32 = 0x258688;
    pub const CODEC_DELAY: u32 = 0x56AA;
    pub const SEEK_PRE_ROLL: u32 = 0x56BB;

    // Video
    pub const VIDEO: u32 = 0xE0;
    pub const PIXEL_WIDTH: u32 = 0xB0;
    pub const PIXEL_HEIGHT: u32 = 0xBA;
    pub const DISPLAY_WIDTH: u32 = 0x54B0;
    pub const DISPLAY_HEIGHT: u32 = 0x54BA;
    pub const DISPLAY_UNIT: u32 = 0x54B2;
    pub const FRAME_RATE: u32 = 0x2383E3;
    pub const COLOR: u32 = 0x55B0;
    pub const MATRIX_COEFFICIENTS: u32 = 0x55B1;
    pub const BITS_PER_CHANNEL: u32 = 0x55B2;
    pub const CHROMA_SUBSAMPLING_HORZ: u32 = 0x55B3;
    pub const CHROMA_SUBSAMPLING_VERT: u32 = 0x55B4;
    pub const TRANSFER_CHARACTERISTICS: u32 = 0x55BA;
    pub const PRIMARIES: u32 = 0x55BB;
    pub const MAX_CLL: u32 = 0x55BC;
    pub const MAX_FALL: u32 = 0x55BD;

    // Audio
    pub const AUDIO: u32 = 0xE1;
    pub const SAMPLING_FREQUENCY: u32 = 0xB5;
    pub const OUTPUT_SAMPLING_FREQUENCY: u32 = 0x78B5;
    pub const CHANNELS: u32 = 0x9F;
    pub const BIT_DEPTH: u32 = 0x6264;

    // Content Encodings (for compressed/encrypted tracks)
    pub const CONTENT_ENCODINGS: u32 = 0x6D80;
    pub const CONTENT_ENCODING: u32 = 0x6240;
    pub const CONTENT_ENCODING_ORDER: u32 = 0x5031;
    pub const CONTENT_ENCODING_SCOPE: u32 = 0x5032;
    pub const CONTENT_ENCODING_TYPE: u32 = 0x5033;
    pub const CONTENT_COMPRESSION: u32 = 0x5034;
    pub const CONTENT_COMP_ALGO: u32 = 0x4254;
    pub const CONTENT_COMP_SETTINGS: u32 = 0x4255;

    // Chapters
    pub const CHAPTERS: u32 = 0x1043A770;
    pub const EDITION_ENTRY: u32 = 0x45B9;
    pub const EDITION_UID: u32 = 0x45BC;
    pub const EDITION_FLAG_HIDDEN: u32 = 0x45BD;
    pub const EDITION_FLAG_DEFAULT: u32 = 0x45DB;
    pub const EDITION_FLAG_ORDERED: u32 = 0x45DD;
    pub const CHAPTER_ATOM: u32 = 0xB6;
    pub const CHAPTER_UID: u32 = 0x73C4;
    pub const CHAPTER_STRING_UID: u32 = 0x5654;
    pub const CHAPTER_TIME_START: u32 = 0x91;
    pub const CHAPTER_TIME_END: u32 = 0x92;
    pub const CHAPTER_FLAG_HIDDEN: u32 = 0x98;
    pub const CHAPTER_FLAG_ENABLED: u32 = 0x4598;
    pub const CHAPTER_DISPLAY: u32 = 0x80;
    pub const CHAP_STRING: u32 = 0x85;
    pub const CHAP_LANGUAGE: u32 = 0x437C;
    pub const CHAP_LANGUAGE_BCP47: u32 = 0x437D;
    pub const CHAP_COUNTRY: u32 = 0x437E;

    // Attachments
    pub const ATTACHMENTS: u32 = 0x1941A469;
    pub const ATTACHED_FILE: u32 = 0x61A7;
    pub const FILE_DESCRIPTION: u32 = 0x467E;
    pub const FILE_NAME: u32 = 0x466E;
    pub const FILE_MIME_TYPE: u32 = 0x4660;
    pub const FILE_DATA: u32 = 0x465C;
    pub const FILE_UID: u32 = 0x46AE;

    // Tags
    pub const TAGS: u32 = 0x1254C367;
    pub const TAG: u32 = 0x7373;
    pub const TARGETS: u32 = 0x63C0;
    pub const TARGET_TYPE_VALUE: u32 = 0x68CA;
    pub const TARGET_TYPE: u32 = 0x63CA;
    pub const TAG_TRACK_UID: u32 = 0x63C5;
    pub const TAG_EDITION_UID: u32 = 0x63C9;
    pub const TAG_CHAPTER_UID: u32 = 0x63C4;
    pub const TAG_ATTACHMENT_UID: u32 = 0x63C6;
    pub const SIMPLE_TAG: u32 = 0x67C8;
    pub const TAG_NAME: u32 = 0x45A3;
    pub const TAG_LANGUAGE: u32 = 0x447A;
    pub const TAG_LANGUAGE_BCP47: u32 = 0x447B;
    pub const TAG_DEFAULT: u32 = 0x4484;
    pub const TAG_STRING: u32 = 0x4487;
    pub const TAG_BINARY: u32 = 0x4485;

    // Cues (seeking index)
    pub const CUES: u32 = 0x1C53BB6B;
    pub const CUE_POINT: u32 = 0xBB;
    pub const CUE_TIME: u32 = 0xB3;
    pub const CUE_TRACK_POSITIONS: u32 = 0xB7;
    pub const CUE_TRACK: u32 = 0xF7;
    pub const CUE_CLUSTER_POSITION: u32 = 0xF1;
    pub const CUE_RELATIVE_POSITION: u32 = 0xF0;
    pub const CUE_DURATION: u32 = 0xB2;
    pub const CUE_BLOCK_NUMBER: u32 = 0x5378;

    // Clusters
    pub const CLUSTER: u32 = 0x1F43B675;
    pub const TIMECODE: u32 = 0xE7;
    pub const SIMPLE_BLOCK: u32 = 0xA3;
    pub const BLOCK_GROUP: u32 = 0xA0;
    pub const BLOCK: u32 = 0xA1;
    pub const BLOCK_DURATION: u32 = 0x9B;
    pub const REFERENCE_BLOCK: u32 = 0xFB;
}

// ============================================================================
// Data Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkvInfo {
    pub file_path: String,
    pub file_size: u64,
    pub duration_ms: u64,
    pub title: Option<String>,
    pub muxing_app: Option<String>,
    pub writing_app: Option<String>,
    pub date_utc: Option<String>,
    pub timecode_scale: u64,
    pub tracks: Vec<MkvTrack>,
    pub chapters: Vec<MkvChapter>,
    pub attachments: Vec<MkvAttachment>,
    pub tags: HashMap<String, String>,
    pub has_cues: bool,
    pub cues: Vec<CuePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MkvTrack {
    Video(VideoTrack),
    Audio(AudioTrack),
    Subtitle(SubtitleTrack),
    Other(OtherTrack),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoTrack {
    pub track_number: u64,
    pub track_uid: u64,
    pub codec_id: String,
    pub codec_name: Option<String>,
    pub name: Option<String>,
    pub language: String,
    pub enabled: bool,
    pub default: bool,
    pub forced: bool,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub display_width: Option<u32>,
    pub display_height: Option<u32>,
    pub frame_rate: Option<f64>,
    pub color_space: Option<ColorInfo>,
    pub hdr_info: Option<HdrInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioTrack {
    pub track_number: u64,
    pub track_uid: u64,
    pub codec_id: String,
    pub codec_name: Option<String>,
    pub name: Option<String>,
    pub language: String,
    pub enabled: bool,
    pub default: bool,
    pub forced: bool,
    pub sample_rate: f64,
    pub channels: u32,
    pub bit_depth: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleTrack {
    pub track_number: u64,
    pub track_uid: u64,
    pub codec_id: String,
    pub codec_name: Option<String>,
    pub name: Option<String>,
    pub language: String,
    pub enabled: bool,
    pub default: bool,
    pub forced: bool,
    pub text_based: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtherTrack {
    pub track_number: u64,
    pub track_uid: u64,
    pub track_type: u8,
    pub codec_id: String,
    pub name: Option<String>,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorInfo {
    pub matrix_coefficients: Option<u8>,
    pub bits_per_channel: Option<u8>,
    pub chroma_subsampling_horz: Option<u8>,
    pub chroma_subsampling_vert: Option<u8>,
    pub transfer_characteristics: Option<u8>,
    pub primaries: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HdrInfo {
    pub max_cll: Option<u32>,
    pub max_fall: Option<u32>,
    pub is_hdr: bool,
    pub hdr_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkvChapter {
    pub uid: u64,
    pub string_uid: Option<String>,
    pub title: String,
    pub language: String,
    pub start_time_ms: u64,
    pub end_time_ms: Option<u64>,
    pub hidden: bool,
    pub enabled: bool,
    pub nested: Vec<MkvChapter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MkvAttachment {
    pub uid: u64,
    pub filename: String,
    pub mime_type: String,
    pub description: Option<String>,
    pub size: u64,
    pub data_offset: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CuePoint {
    pub time_ms: u64,
    pub track: u64,
    pub cluster_position: u64,
    pub relative_position: Option<u64>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedAttachment {
    pub filename: String,
    pub output_path: String,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackSelection {
    pub video: Option<u64>,
    pub audio: Vec<u64>,
    pub subtitles: Vec<u64>,
}

// ============================================================================
// EBML Parser
// ============================================================================

struct EbmlReader<R: Read + Seek> {
    reader: BufReader<R>,
    position: u64,
}

impl<R: Read + Seek> EbmlReader<R> {
    fn new(reader: R) -> Self {
        Self {
            reader: BufReader::new(reader),
            position: 0,
        }
    }

    fn read_vint(&mut self) -> Result<(u64, usize), String> {
        let mut first_byte = [0u8; 1];
        self.reader.read_exact(&mut first_byte)
            .map_err(|e| format!("Failed to read VINT: {}", e))?;
        self.position += 1;

        let leading_zeros = first_byte[0].leading_zeros();
        if leading_zeros > 7 {
            return Err("Invalid VINT".to_string());
        }

        let length = (leading_zeros + 1) as usize;
        let mut value = (first_byte[0] & (0xFF >> leading_zeros)) as u64;

        for _ in 1..length {
            let mut byte = [0u8; 1];
            self.reader.read_exact(&mut byte)
                .map_err(|e| format!("Failed to read VINT: {}", e))?;
            self.position += 1;
            value = (value << 8) | byte[0] as u64;
        }

        Ok((value, length))
    }

    fn read_element_id(&mut self) -> Result<(u32, usize), String> {
        let mut first_byte = [0u8; 1];
        self.reader.read_exact(&mut first_byte)
            .map_err(|e| format!("Failed to read element ID: {}", e))?;
        self.position += 1;

        let leading_zeros = first_byte[0].leading_zeros();
        if leading_zeros > 3 {
            return Err("Invalid element ID".to_string());
        }

        let length = (leading_zeros + 1) as usize;
        let mut id = first_byte[0] as u32;

        for _ in 1..length {
            let mut byte = [0u8; 1];
            self.reader.read_exact(&mut byte)
                .map_err(|e| format!("Failed to read element ID: {}", e))?;
            self.position += 1;
            id = (id << 8) | byte[0] as u32;
        }

        Ok((id, length))
    }

    fn read_element_size(&mut self) -> Result<(u64, usize), String> {
        let (size, len) = self.read_vint()?;
        // Check for unknown size marker
        let max_for_length = (1u64 << (7 * len)) - 1;
        if size == max_for_length {
            Ok((u64::MAX, len)) // Unknown size
        } else {
            Ok((size, len))
        }
    }

    fn read_uint(&mut self, size: usize) -> Result<u64, String> {
        let mut value = 0u64;
        for _ in 0..size {
            let mut byte = [0u8; 1];
            self.reader.read_exact(&mut byte)
                .map_err(|e| format!("Failed to read uint: {}", e))?;
            self.position += 1;
            value = (value << 8) | byte[0] as u64;
        }
        Ok(value)
    }

    fn read_sint(&mut self, size: usize) -> Result<i64, String> {
        let unsigned = self.read_uint(size)?;
        let shift = 64 - (size * 8);
        Ok((unsigned as i64) << shift >> shift)
    }

    fn read_float(&mut self, size: usize) -> Result<f64, String> {
        match size {
            4 => {
                let bits = self.read_uint(4)? as u32;
                Ok(f32::from_bits(bits) as f64)
            }
            8 => {
                let bits = self.read_uint(8)?;
                Ok(f64::from_bits(bits))
            }
            _ => Err(format!("Invalid float size: {}", size)),
        }
    }

    fn read_string(&mut self, size: usize) -> Result<String, String> {
        let mut bytes = vec![0u8; size];
        self.reader.read_exact(&mut bytes)
            .map_err(|e| format!("Failed to read string: {}", e))?;
        self.position += size as u64;
        
        // Remove null bytes and decode as UTF-8
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8(bytes[..end].to_vec())
            .map_err(|e| format!("Invalid UTF-8 string: {}", e))
    }

    fn read_binary(&mut self, size: usize) -> Result<Vec<u8>, String> {
        let mut bytes = vec![0u8; size];
        self.reader.read_exact(&mut bytes)
            .map_err(|e| format!("Failed to read binary: {}", e))?;
        self.position += size as u64;
        Ok(bytes)
    }

    fn skip(&mut self, size: u64) -> Result<(), String> {
        self.reader.seek(SeekFrom::Current(size as i64))
            .map_err(|e| format!("Failed to skip: {}", e))?;
        self.position += size;
        Ok(())
    }

    fn seek(&mut self, pos: u64) -> Result<(), String> {
        self.reader.seek(SeekFrom::Start(pos))
            .map_err(|e| format!("Failed to seek: {}", e))?;
        self.position = pos;
        Ok(())
    }

    fn position(&self) -> u64 {
        self.position
    }
}

// ============================================================================
// MKV Parser
// ============================================================================

pub struct MkvParser {
    timecode_scale: u64,
    segment_start: u64,
}

impl MkvParser {
    pub fn new() -> Self {
        Self {
            timecode_scale: 1_000_000, // Default: 1ms
            segment_start: 0,
        }
    }

    pub fn parse<P: AsRef<Path>>(&mut self, path: P) -> Result<MkvInfo, String> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|e| format!("Failed to open file: {}", e))?;
        let file_size = file.metadata()
            .map_err(|e| format!("Failed to get file metadata: {}", e))?.len();

        let mut reader = EbmlReader::new(file);

        // Parse EBML header
        self.parse_ebml_header(&mut reader)?;

        let mut info = MkvInfo {
            file_path: path.to_string_lossy().to_string(),
            file_size,
            duration_ms: 0,
            title: None,
            muxing_app: None,
            writing_app: None,
            date_utc: None,
            timecode_scale: self.timecode_scale,
            tracks: Vec::new(),
            chapters: Vec::new(),
            attachments: Vec::new(),
            tags: HashMap::new(),
            has_cues: false,
            cues: Vec::new(),
        };

        // Parse Segment
        let (segment_id, _) = reader.read_element_id()?;
        if segment_id != element_ids::SEGMENT {
            return Err("Expected Segment element".to_string());
        }
        let (segment_size, _) = reader.read_element_size()?;
        self.segment_start = reader.position();
        let segment_end = if segment_size == u64::MAX {
            file_size
        } else {
            self.segment_start + segment_size
        };

        // Parse segment children
        while reader.position() < segment_end {
            let (id, _) = reader.read_element_id()?;
            let (size, _) = reader.read_element_size()?;
            let element_end = reader.position() + size;

            match id {
                element_ids::SEGMENT_INFO => {
                    self.parse_segment_info(&mut reader, &mut info, size)?;
                }
                element_ids::TRACKS => {
                    self.parse_tracks(&mut reader, &mut info, size)?;
                }
                element_ids::CHAPTERS => {
                    self.parse_chapters(&mut reader, &mut info, size)?;
                }
                element_ids::ATTACHMENTS => {
                    self.parse_attachments(&mut reader, &mut info, size)?;
                }
                element_ids::TAGS => {
                    self.parse_tags(&mut reader, &mut info, size)?;
                }
                element_ids::CUES => {
                    info.has_cues = true;
                    reader.skip(size)?;
                }
                element_ids::CLUSTER => {
                    // Stop parsing at first cluster - we have all metadata
                    break;
                }
                _ => {
                    reader.skip(size)?;
                }
            }

            if reader.position() < element_end {
                reader.seek(element_end)?;
            }
        }

        info.timecode_scale = self.timecode_scale;
        Ok(info)
    }

    fn parse_ebml_header<R: Read + Seek>(&mut self, reader: &mut EbmlReader<R>) -> Result<(), String> {
        let (id, _) = reader.read_element_id()?;
        if id != element_ids::EBML {
            return Err("Not an EBML file".to_string());
        }

        let (size, _) = reader.read_element_size()?;
        let header_end = reader.position() + size;

        let mut doc_type = String::new();

        while reader.position() < header_end {
            let (child_id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            match child_id {
                element_ids::DOC_TYPE => {
                    doc_type = reader.read_string(child_size as usize)?;
                }
                _ => {
                    reader.skip(child_size)?;
                }
            }
        }

        if doc_type != "matroska" && doc_type != "webm" {
            return Err(format!("Unsupported document type: {}", doc_type));
        }

        Ok(())
    }

    fn parse_segment_info<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;
        let mut duration_raw: Option<f64> = None;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            match id {
                element_ids::TIMECODE_SCALE => {
                    self.timecode_scale = reader.read_uint(child_size as usize)?;
                }
                element_ids::DURATION => {
                    duration_raw = Some(reader.read_float(child_size as usize)?);
                }
                element_ids::TITLE => {
                    info.title = Some(reader.read_string(child_size as usize)?);
                }
                element_ids::MUXING_APP => {
                    info.muxing_app = Some(reader.read_string(child_size as usize)?);
                }
                element_ids::WRITING_APP => {
                    info.writing_app = Some(reader.read_string(child_size as usize)?);
                }
                element_ids::DATE_UTC => {
                    let nanos = reader.read_sint(child_size as usize)?;
                    // Convert from nanoseconds since 2001-01-01 to ISO date
                    let epoch_2001 = 978307200i64; // Unix timestamp of 2001-01-01
                    let unix_secs = epoch_2001 + (nanos / 1_000_000_000);
                    info.date_utc = Some(format_unix_timestamp(unix_secs));
                }
                _ => {
                    reader.skip(child_size)?;
                }
            }
        }

        if let Some(duration) = duration_raw {
            info.duration_ms = ((duration * self.timecode_scale as f64) / 1_000_000.0) as u64;
        }

        Ok(())
    }

    fn parse_tracks<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            if id == element_ids::TRACK_ENTRY {
                if let Ok(track) = self.parse_track_entry(reader, child_size) {
                    info.tracks.push(track);
                }
            } else {
                reader.skip(child_size)?;
            }
        }

        Ok(())
    }

    fn parse_track_entry<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        size: u64,
    ) -> Result<MkvTrack, String> {
        let end = reader.position() + size;

        let mut track_number: u64 = 0;
        let mut track_uid: u64 = 0;
        let mut track_type: u8 = 0;
        let mut codec_id = String::new();
        let mut codec_name: Option<String> = None;
        let mut name: Option<String> = None;
        let mut language = "und".to_string();
        let mut enabled = true;
        let mut default = true;
        let mut forced = false;

        // Video-specific
        let mut pixel_width: u32 = 0;
        let mut pixel_height: u32 = 0;
        let mut display_width: Option<u32> = None;
        let mut display_height: Option<u32> = None;
        let mut frame_rate: Option<f64> = None;
        let mut color_info: Option<ColorInfo> = None;
        let mut hdr_info: Option<HdrInfo> = None;

        // Audio-specific
        let mut sample_rate: f64 = 0.0;
        let mut channels: u32 = 0;
        let mut bit_depth: Option<u32> = None;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;
            let child_end = reader.position() + child_size;

            match id {
                element_ids::TRACK_NUMBER => {
                    track_number = reader.read_uint(child_size as usize)?;
                }
                element_ids::TRACK_UID => {
                    track_uid = reader.read_uint(child_size as usize)?;
                }
                element_ids::TRACK_TYPE => {
                    track_type = reader.read_uint(child_size as usize)? as u8;
                }
                element_ids::CODEC_ID => {
                    codec_id = reader.read_string(child_size as usize)?;
                }
                element_ids::CODEC_NAME => {
                    codec_name = Some(reader.read_string(child_size as usize)?);
                }
                element_ids::NAME => {
                    name = Some(reader.read_string(child_size as usize)?);
                }
                element_ids::LANGUAGE => {
                    language = reader.read_string(child_size as usize)?;
                }
                element_ids::LANGUAGE_BCP47 => {
                    language = reader.read_string(child_size as usize)?;
                }
                element_ids::FLAG_ENABLED => {
                    enabled = reader.read_uint(child_size as usize)? != 0;
                }
                element_ids::FLAG_DEFAULT => {
                    default = reader.read_uint(child_size as usize)? != 0;
                }
                element_ids::FLAG_FORCED => {
                    forced = reader.read_uint(child_size as usize)? != 0;
                }
                element_ids::VIDEO => {
                    // Parse video settings
                    while reader.position() < child_end {
                        let (vid_id, _) = reader.read_element_id()?;
                        let (vid_size, _) = reader.read_element_size()?;

                        match vid_id {
                            element_ids::PIXEL_WIDTH => {
                                pixel_width = reader.read_uint(vid_size as usize)? as u32;
                            }
                            element_ids::PIXEL_HEIGHT => {
                                pixel_height = reader.read_uint(vid_size as usize)? as u32;
                            }
                            element_ids::DISPLAY_WIDTH => {
                                display_width = Some(reader.read_uint(vid_size as usize)? as u32);
                            }
                            element_ids::DISPLAY_HEIGHT => {
                                display_height = Some(reader.read_uint(vid_size as usize)? as u32);
                            }
                            element_ids::FRAME_RATE => {
                                frame_rate = Some(reader.read_float(vid_size as usize)?);
                            }
                            element_ids::COLOR => {
                                let (ci, hi) = self.parse_color_info(reader, vid_size)?;
                                color_info = Some(ci);
                                hdr_info = hi;
                            }
                            _ => {
                                reader.skip(vid_size)?;
                            }
                        }
                    }
                }
                element_ids::AUDIO => {
                    // Parse audio settings
                    while reader.position() < child_end {
                        let (aud_id, _) = reader.read_element_id()?;
                        let (aud_size, _) = reader.read_element_size()?;

                        match aud_id {
                            element_ids::SAMPLING_FREQUENCY => {
                                sample_rate = reader.read_float(aud_size as usize)?;
                            }
                            element_ids::CHANNELS => {
                                channels = reader.read_uint(aud_size as usize)? as u32;
                            }
                            element_ids::BIT_DEPTH => {
                                bit_depth = Some(reader.read_uint(aud_size as usize)? as u32);
                            }
                            _ => {
                                reader.skip(aud_size)?;
                            }
                        }
                    }
                }
                _ => {
                    reader.skip(child_size)?;
                }
            }

            if reader.position() < child_end {
                reader.seek(child_end)?;
            }
        }

        match track_type {
            1 => Ok(MkvTrack::Video(VideoTrack {
                track_number,
                track_uid,
                codec_id,
                codec_name,
                name,
                language,
                enabled,
                default,
                forced,
                pixel_width,
                pixel_height,
                display_width,
                display_height,
                frame_rate,
                color_space: color_info,
                hdr_info,
            })),
            2 => Ok(MkvTrack::Audio(AudioTrack {
                track_number,
                track_uid,
                codec_id,
                codec_name,
                name,
                language,
                enabled,
                default,
                forced,
                sample_rate,
                channels,
                bit_depth,
            })),
            17 => Ok(MkvTrack::Subtitle(SubtitleTrack {
                track_number,
                track_uid,
                codec_id: codec_id.clone(),
                codec_name,
                name,
                language,
                enabled,
                default,
                forced,
                text_based: is_text_subtitle(&codec_id),
            })),
            _ => Ok(MkvTrack::Other(OtherTrack {
                track_number,
                track_uid,
                track_type,
                codec_id,
                name,
                language,
            })),
        }
    }

    fn parse_color_info<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        size: u64,
    ) -> Result<(ColorInfo, Option<HdrInfo>), String> {
        let end = reader.position() + size;

        let mut color = ColorInfo {
            matrix_coefficients: None,
            bits_per_channel: None,
            chroma_subsampling_horz: None,
            chroma_subsampling_vert: None,
            transfer_characteristics: None,
            primaries: None,
        };

        let mut max_cll: Option<u32> = None;
        let mut max_fall: Option<u32> = None;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            match id {
                element_ids::MATRIX_COEFFICIENTS => {
                    color.matrix_coefficients = Some(reader.read_uint(child_size as usize)? as u8);
                }
                element_ids::BITS_PER_CHANNEL => {
                    color.bits_per_channel = Some(reader.read_uint(child_size as usize)? as u8);
                }
                element_ids::CHROMA_SUBSAMPLING_HORZ => {
                    color.chroma_subsampling_horz = Some(reader.read_uint(child_size as usize)? as u8);
                }
                element_ids::CHROMA_SUBSAMPLING_VERT => {
                    color.chroma_subsampling_vert = Some(reader.read_uint(child_size as usize)? as u8);
                }
                element_ids::TRANSFER_CHARACTERISTICS => {
                    color.transfer_characteristics = Some(reader.read_uint(child_size as usize)? as u8);
                }
                element_ids::PRIMARIES => {
                    color.primaries = Some(reader.read_uint(child_size as usize)? as u8);
                }
                element_ids::MAX_CLL => {
                    max_cll = Some(reader.read_uint(child_size as usize)? as u32);
                }
                element_ids::MAX_FALL => {
                    max_fall = Some(reader.read_uint(child_size as usize)? as u32);
                }
                _ => {
                    reader.skip(child_size)?;
                }
            }
        }

        // Determine HDR format
        let hdr_info = if max_cll.is_some() || max_fall.is_some() || 
            color.transfer_characteristics == Some(16) || // PQ
            color.transfer_characteristics == Some(18)    // HLG
        {
            let hdr_format = match color.transfer_characteristics {
                Some(16) => Some("HDR10".to_string()),
                Some(18) => Some("HLG".to_string()),
                _ => None,
            };
            Some(HdrInfo {
                max_cll,
                max_fall,
                is_hdr: true,
                hdr_format,
            })
        } else {
            None
        };

        Ok((color, hdr_info))
    }

    fn parse_chapters<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            if id == element_ids::EDITION_ENTRY {
                self.parse_edition_entry(reader, info, child_size)?;
            } else {
                reader.skip(child_size)?;
            }
        }

        Ok(())
    }

    fn parse_edition_entry<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            if id == element_ids::CHAPTER_ATOM {
                if let Ok(chapter) = self.parse_chapter_atom(reader, child_size) {
                    info.chapters.push(chapter);
                }
            } else {
                reader.skip(child_size)?;
            }
        }

        Ok(())
    }

    fn parse_chapter_atom<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        size: u64,
    ) -> Result<MkvChapter, String> {
        let end = reader.position() + size;

        let mut uid: u64 = 0;
        let mut string_uid: Option<String> = None;
        let mut title = String::new();
        let mut language = "eng".to_string();
        let mut start_time_ns: u64 = 0;
        let mut end_time_ns: Option<u64> = None;
        let mut hidden = false;
        let mut enabled = true;
        let mut nested = Vec::new();

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;
            let child_end = reader.position() + child_size;

            match id {
                element_ids::CHAPTER_UID => {
                    uid = reader.read_uint(child_size as usize)?;
                }
                element_ids::CHAPTER_STRING_UID => {
                    string_uid = Some(reader.read_string(child_size as usize)?);
                }
                element_ids::CHAPTER_TIME_START => {
                    start_time_ns = reader.read_uint(child_size as usize)?;
                }
                element_ids::CHAPTER_TIME_END => {
                    end_time_ns = Some(reader.read_uint(child_size as usize)?);
                }
                element_ids::CHAPTER_FLAG_HIDDEN => {
                    hidden = reader.read_uint(child_size as usize)? != 0;
                }
                element_ids::CHAPTER_FLAG_ENABLED => {
                    enabled = reader.read_uint(child_size as usize)? != 0;
                }
                element_ids::CHAPTER_DISPLAY => {
                    // Parse chapter display (title, language)
                    while reader.position() < child_end {
                        let (disp_id, _) = reader.read_element_id()?;
                        let (disp_size, _) = reader.read_element_size()?;

                        match disp_id {
                            element_ids::CHAP_STRING => {
                                title = reader.read_string(disp_size as usize)?;
                            }
                            element_ids::CHAP_LANGUAGE | element_ids::CHAP_LANGUAGE_BCP47 => {
                                language = reader.read_string(disp_size as usize)?;
                            }
                            _ => {
                                reader.skip(disp_size)?;
                            }
                        }
                    }
                }
                element_ids::CHAPTER_ATOM => {
                    // Nested chapter
                    if let Ok(nested_chapter) = self.parse_chapter_atom(reader, child_size) {
                        nested.push(nested_chapter);
                    }
                }
                _ => {
                    reader.skip(child_size)?;
                }
            }

            if reader.position() < child_end {
                reader.seek(child_end)?;
            }
        }

        // Convert nanoseconds to milliseconds
        let start_time_ms = start_time_ns / 1_000_000;
        let end_time_ms = end_time_ns.map(|ns| ns / 1_000_000);

        Ok(MkvChapter {
            uid,
            string_uid,
            title,
            language,
            start_time_ms,
            end_time_ms,
            hidden,
            enabled,
            nested,
        })
    }

    fn parse_attachments<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            if id == element_ids::ATTACHED_FILE {
                if let Ok(attachment) = self.parse_attached_file(reader, child_size) {
                    info.attachments.push(attachment);
                }
            } else {
                reader.skip(child_size)?;
            }
        }

        Ok(())
    }

    fn parse_attached_file<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        size: u64,
    ) -> Result<MkvAttachment, String> {
        let end = reader.position() + size;

        let mut uid: u64 = 0;
        let mut filename = String::new();
        let mut mime_type = String::new();
        let mut description: Option<String> = None;
        let mut data_size: u64 = 0;
        let mut data_offset: u64 = 0;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            match id {
                element_ids::FILE_UID => {
                    uid = reader.read_uint(child_size as usize)?;
                }
                element_ids::FILE_NAME => {
                    filename = reader.read_string(child_size as usize)?;
                }
                element_ids::FILE_MIME_TYPE => {
                    mime_type = reader.read_string(child_size as usize)?;
                }
                element_ids::FILE_DESCRIPTION => {
                    description = Some(reader.read_string(child_size as usize)?);
                }
                element_ids::FILE_DATA => {
                    data_offset = reader.position();
                    data_size = child_size;
                    reader.skip(child_size)?;
                }
                _ => {
                    reader.skip(child_size)?;
                }
            }
        }

        Ok(MkvAttachment {
            uid,
            filename,
            mime_type,
            description,
            size: data_size,
            data_offset,
        })
    }

    fn parse_tags<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            if id == element_ids::TAG {
                self.parse_tag(reader, info, child_size)?;
            } else {
                reader.skip(child_size)?;
            }
        }

        Ok(())
    }

    fn parse_tag<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            if id == element_ids::SIMPLE_TAG {
                self.parse_simple_tag(reader, info, child_size)?;
            } else {
                reader.skip(child_size)?;
            }
        }

        Ok(())
    }

    fn parse_simple_tag<R: Read + Seek>(
        &mut self,
        reader: &mut EbmlReader<R>,
        info: &mut MkvInfo,
        size: u64,
    ) -> Result<(), String> {
        let end = reader.position() + size;

        let mut tag_name = String::new();
        let mut tag_value = String::new();

        while reader.position() < end {
            let (id, _) = reader.read_element_id()?;
            let (child_size, _) = reader.read_element_size()?;

            match id {
                element_ids::TAG_NAME => {
                    tag_name = reader.read_string(child_size as usize)?;
                }
                element_ids::TAG_STRING => {
                    tag_value = reader.read_string(child_size as usize)?;
                }
                element_ids::SIMPLE_TAG => {
                    // Nested simple tag
                    self.parse_simple_tag(reader, info, child_size)?;
                }
                _ => {
                    reader.skip(child_size)?;
                }
            }
        }

        if !tag_name.is_empty() {
            info.tags.insert(tag_name, tag_value);
        }

        Ok(())
    }
}

// ============================================================================
// Attachment Extraction
// ============================================================================

pub fn extract_attachment<P: AsRef<Path>, Q: AsRef<Path>>(
    mkv_path: P,
    attachment: &MkvAttachment,
    output_dir: Q,
) -> Result<ExtractedAttachment, String> {
    let mkv_path = mkv_path.as_ref();
    let output_dir = output_dir.as_ref();

    // Create output directory if needed
    std::fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output directory: {}", e))?;

    let output_path = output_dir.join(&attachment.filename);

    // Open MKV file and seek to attachment data
    let mut file = File::open(mkv_path)
        .map_err(|e| format!("Failed to open MKV file: {}", e))?;
    file.seek(SeekFrom::Start(attachment.data_offset))
        .map_err(|e| format!("Failed to seek to attachment: {}", e))?;

    // Read attachment data
    let mut data = vec![0u8; attachment.size as usize];
    file.read_exact(&mut data)
        .map_err(|e| format!("Failed to read attachment data: {}", e))?;

    // Write to output file
    std::fs::write(&output_path, &data)
        .map_err(|e| format!("Failed to write attachment: {}", e))?;

    Ok(ExtractedAttachment {
        filename: attachment.filename.clone(),
        output_path: output_path.to_string_lossy().to_string(),
        size: attachment.size,
    })
}

pub fn extract_all_fonts<P: AsRef<Path>>(
    mkv_path: P,
    output_dir: P,
) -> Result<Vec<ExtractedAttachment>, String> {
    let mkv_path = mkv_path.as_ref();
    
    let mut parser = MkvParser::new();
    let info = parser.parse(mkv_path)?;

    let font_extensions = ["ttf", "otf", "ttc", "woff", "woff2"];
    let font_mimes = ["font/", "application/x-font", "application/font"];

    let mut extracted = Vec::new();

    for attachment in &info.attachments {
        let is_font = font_extensions.iter().any(|ext| {
            attachment.filename.to_lowercase().ends_with(ext)
        }) || font_mimes.iter().any(|mime| {
            attachment.mime_type.to_lowercase().starts_with(mime)
        });

        if is_font {
            match extract_attachment(mkv_path, attachment, &output_dir) {
                Ok(ext) => extracted.push(ext),
                Err(e) => eprintln!("Failed to extract {}: {}", attachment.filename, e),
            }
        }
    }

    Ok(extracted)
}

// ============================================================================
// Utility Functions
// ============================================================================

fn is_text_subtitle(codec_id: &str) -> bool {
    matches!(codec_id, 
        "S_TEXT/UTF8" | "S_TEXT/SSA" | "S_TEXT/ASS" | 
        "S_TEXT/WEBVTT" | "S_TEXT/USF" | "S_KATE"
    )
}

fn format_unix_timestamp(timestamp: i64) -> String {
    // Simple ISO date formatting
    let secs_per_day = 86400i64;
    let secs_per_hour = 3600i64;
    let secs_per_min = 60i64;
    
    let days_since_epoch = timestamp / secs_per_day;
    let remaining_secs = timestamp % secs_per_day;
    
    let hours = remaining_secs / secs_per_hour;
    let mins = (remaining_secs % secs_per_hour) / secs_per_min;
    let secs = remaining_secs % secs_per_min;
    
    // Approximate year/month/day calculation
    let mut year = 1970;
    let mut days = days_since_epoch;
    
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    
    let month_days = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    
    let mut month = 1;
    for &md in &month_days {
        if days < md as i64 {
            break;
        }
        days -= md as i64;
        month += 1;
    }
    
    let day = days + 1;
    
    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, hours, mins, secs)
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ============================================================================
// Public API
// ============================================================================


pub async fn mkv_get_info(path: String) -> Result<MkvInfo, String> {
    let mut parser = MkvParser::new();
    parser.parse(&path)
}


pub async fn mkv_get_tracks(path: String) -> Result<Vec<MkvTrack>, String> {
    let mut parser = MkvParser::new();
    let info = parser.parse(&path)?;
    Ok(info.tracks)
}


pub async fn mkv_get_chapters(path: String) -> Result<Vec<MkvChapter>, String> {
    let mut parser = MkvParser::new();
    let info = parser.parse(&path)?;
    Ok(info.chapters)
}


pub async fn mkv_get_attachments(path: String) -> Result<Vec<MkvAttachment>, String> {
    let mut parser = MkvParser::new();
    let info = parser.parse(&path)?;
    Ok(info.attachments)
}


pub async fn mkv_extract_attachment(
    mkv_path: String,
    attachment_uid: u64,
    output_dir: String,
) -> Result<ExtractedAttachment, String> {
    let mut parser = MkvParser::new();
    let info = parser.parse(&mkv_path)?;

    let attachment = info.attachments.iter()
        .find(|a| a.uid == attachment_uid)
        .ok_or_else(|| format!("Attachment with UID {} not found", attachment_uid))?;

    extract_attachment(&mkv_path, attachment, &output_dir)
}


pub async fn mkv_extract_fonts(
    mkv_path: String,
    output_dir: String,
) -> Result<Vec<ExtractedAttachment>, String> {
    extract_all_fonts(&mkv_path, &output_dir)
}


pub async fn mkv_get_default_tracks(path: String) -> Result<TrackSelection, String> {
    let mut parser = MkvParser::new();
    let info = parser.parse(&path)?;

    let mut selection = TrackSelection {
        video: None,
        audio: Vec::new(),
        subtitles: Vec::new(),
    };

    for track in &info.tracks {
        match track {
            MkvTrack::Video(v) => {
                if selection.video.is_none() && v.default && v.enabled {
                    selection.video = Some(v.track_number);
                }
            }
            MkvTrack::Audio(a) => {
                if a.default && a.enabled {
                    selection.audio.push(a.track_number);
                }
            }
            MkvTrack::Subtitle(s) => {
                if (s.default || s.forced) && s.enabled {
                    selection.subtitles.push(s.track_number);
                }
            }
            _ => {}
        }
    }

    // If no default video, pick first enabled
    if selection.video.is_none() {
        for track in &info.tracks {
            if let MkvTrack::Video(v) = track {
                if v.enabled {
                    selection.video = Some(v.track_number);
                    break;
                }
            }
        }
    }

    // If no default audio, pick first enabled
    if selection.audio.is_empty() {
        for track in &info.tracks {
            if let MkvTrack::Audio(a) = track {
                if a.enabled {
                    selection.audio.push(a.track_number);
                    break;
                }
            }
        }
    }

    Ok(selection)
}


pub async fn mkv_format_duration(duration_ms: u64) -> String {
    let total_secs = duration_ms / 1000;
    let hours = total_secs / 3600;
    let mins = (total_secs % 3600) / 60;
    let secs = total_secs % 60;
    let ms = duration_ms % 1000;

    if hours > 0 {
        format!("{}:{:02}:{:02}.{:03}", hours, mins, secs, ms)
    } else {
        format!("{:02}:{:02}.{:03}", mins, secs, ms)
    }
}

// ============================================================================
// MKV Demuxer - Frame Packet Reading
// ============================================================================

/// Packet from MKV demuxer
#[derive(Debug, Clone)]
pub struct MkvPacket {
    pub track_number: u64,
    pub pts_ms: i64,
    pub duration_ms: Option<i64>,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

/// MKV demuxer for reading frame packets
pub struct MkvDemuxer<R: Read + Seek> {
    reader: R,
    info: MkvInfo,
    timecode_scale: u64,
    cluster_timecode: i64,
    current_position: u64,
    file_size: u64,
}

impl<R: Read + Seek> MkvDemuxer<R> {
    /// Create new demuxer from reader and parsed info
    pub fn new(mut reader: R, info: MkvInfo) -> Result<Self, String> {
        let file_size = reader.seek(SeekFrom::End(0))
            .map_err(|e| format!("Seek error: {}", e))?;
        
        // Seek to first cluster
        let cluster_pos = info.cues.first()
            .map(|c| c.cluster_position)
            .unwrap_or(0);
        
        reader.seek(SeekFrom::Start(cluster_pos))
            .map_err(|e| format!("Seek error: {}", e))?;
        
        Ok(Self {
            reader,
            timecode_scale: info.timecode_scale,
            info,
            cluster_timecode: 0,
            current_position: cluster_pos,
            file_size,
        })
    }
    
    /// Get media info
    pub fn info(&self) -> &MkvInfo {
        &self.info
    }
    
    /// Find video track number
    pub fn video_track(&self) -> Option<u64> {
        for track in &self.info.tracks {
            if let MkvTrack::Video(v) = track {
                return Some(v.track_number);
            }
        }
        None
    }
    
    /// Find audio track number  
    pub fn audio_track(&self) -> Option<u64> {
        for track in &self.info.tracks {
            if let MkvTrack::Audio(a) = track {
                return Some(a.track_number);
            }
        }
        None
    }
    
    /// Read next packet
    pub fn read_packet(&mut self) -> Option<MkvPacket> {
        loop {
            if self.current_position >= self.file_size {
                return None;
            }
            
            // Read element header
            let (id, size) = match self.read_element_header() {
                Ok(h) => h,
                Err(_) => return None,
            };
            
            match id {
                element_ids::CLUSTER => {
                    // Enter cluster, read timecode
                    self.cluster_timecode = 0;
                }
                element_ids::TIMECODE => {
                    // Read cluster timecode
                    if let Ok(tc) = self.read_uint(size as usize) {
                        self.cluster_timecode = tc as i64;
                    }
                }
                element_ids::SIMPLE_BLOCK => {
                    // Parse SimpleBlock
                    if let Ok(packet) = self.parse_simple_block(size) {
                        return Some(packet);
                    }
                }
                element_ids::BLOCK_GROUP => {
                    // Parse BlockGroup (contains Block + metadata)
                    if let Ok(Some(packet)) = self.parse_block_group(size) {
                        return Some(packet);
                    }
                }
                _ => {
                    // Skip unknown element
                    let _ = self.reader.seek(SeekFrom::Current(size as i64));
                }
            }
            
            self.current_position = self.reader.stream_position().unwrap_or(self.file_size);
        }
    }
    
    /// Seek to time in milliseconds
    pub fn seek(&mut self, time_ms: u64) -> Result<(), String> {
        // Find nearest cue point
        let mut best_cue = None;
        for cue in &self.info.cues {
            if cue.time_ms <= time_ms {
                best_cue = Some(cue.clone());
            } else {
                break;
            }
        }
        
        if let Some(cue) = best_cue {
            self.reader.seek(SeekFrom::Start(cue.cluster_position))
                .map_err(|e| format!("Seek error: {}", e))?;
            self.current_position = cue.cluster_position;
            self.cluster_timecode = (cue.time_ms as i64 * 1_000_000) / self.timecode_scale as i64;
        }
        
        Ok(())
    }
    
    fn read_element_header(&mut self) -> Result<(u32, u64), String> {
        // Read variable-length element ID
        let id = self.read_vint_id()?;
        // Read variable-length size
        let size = self.read_vint_size()?;
        Ok((id, size))
    }
    
    fn read_vint_id(&mut self) -> Result<u32, String> {
        let mut buf = [0u8; 1];
        self.reader.read_exact(&mut buf)
            .map_err(|e| format!("Read error: {}", e))?;
        
        let first = buf[0];
        let len = first.leading_zeros() + 1;
        
        let mut value = first as u32;
        for _ in 1..len {
            self.reader.read_exact(&mut buf)
                .map_err(|e| format!("Read error: {}", e))?;
            value = (value << 8) | buf[0] as u32;
        }
        
        Ok(value)
    }
    
    fn read_vint_size(&mut self) -> Result<u64, String> {
        let mut buf = [0u8; 1];
        self.reader.read_exact(&mut buf)
            .map_err(|e| format!("Read error: {}", e))?;
        
        let first = buf[0];
        let len = first.leading_zeros() + 1;
        let mask = (1u8 << (8 - len)) - 1;
        
        let mut value = (first & mask) as u64;
        for _ in 1..len {
            self.reader.read_exact(&mut buf)
                .map_err(|e| format!("Read error: {}", e))?;
            value = (value << 8) | buf[0] as u64;
        }
        
        Ok(value)
    }
    
    fn read_uint(&mut self, size: usize) -> Result<u64, String> {
        let mut buf = vec![0u8; size];
        self.reader.read_exact(&mut buf)
            .map_err(|e| format!("Read error: {}", e))?;
        
        let mut value = 0u64;
        for b in buf {
            value = (value << 8) | b as u64;
        }
        Ok(value)
    }
    
    fn parse_simple_block(&mut self, size: u64) -> Result<MkvPacket, String> {
        let start_pos = self.reader.stream_position()
            .map_err(|e| format!("Position error: {}", e))?;
        
        // Read track number (variable int without length marker)
        let track_number = self.read_vint_size()?;
        
        // Read relative timecode (2 bytes, signed)
        let mut tc_buf = [0u8; 2];
        self.reader.read_exact(&mut tc_buf)
            .map_err(|e| format!("Read error: {}", e))?;
        let relative_timecode = i16::from_be_bytes(tc_buf) as i64;
        
        // Read flags (1 byte)
        let mut flags = [0u8; 1];
        self.reader.read_exact(&mut flags)
            .map_err(|e| format!("Read error: {}", e))?;
        let keyframe = (flags[0] & 0x80) != 0;
        
        // Calculate header size
        let current = self.reader.stream_position()
            .map_err(|e| format!("Position error: {}", e))?;
        let header_size = current - start_pos;
        let data_size = size - header_size;
        
        // Read frame data
        let mut data = vec![0u8; data_size as usize];
        self.reader.read_exact(&mut data)
            .map_err(|e| format!("Read error: {}", e))?;
        
        // Calculate PTS
        let pts_ns = (self.cluster_timecode + relative_timecode) * self.timecode_scale as i64;
        let pts_ms = pts_ns / 1_000_000;
        
        Ok(MkvPacket {
            track_number,
            pts_ms,
            duration_ms: None,
            keyframe,
            data,
        })
    }
    
    fn parse_block_group(&mut self, size: u64) -> Result<Option<MkvPacket>, String> {
        let end_pos = self.reader.stream_position()
            .map_err(|e| format!("Position error: {}", e))? + size;
        
        let mut packet: Option<MkvPacket> = None;
        let mut duration_ms: Option<i64> = None;
        let mut is_keyframe = true; // Assume keyframe unless reference block found
        
        while self.reader.stream_position().unwrap_or(end_pos) < end_pos {
            let (id, elem_size) = self.read_element_header()?;
            
            match id {
                element_ids::BLOCK => {
                    // Parse like SimpleBlock but without keyframe flag in data
                    if let Ok(mut p) = self.parse_simple_block(elem_size) {
                        p.keyframe = true; // Will be updated by reference block
                        packet = Some(p);
                    }
                }
                element_ids::BLOCK_DURATION => {
                    if let Ok(d) = self.read_uint(elem_size as usize) {
                        let dur_ns = d * self.timecode_scale;
                        duration_ms = Some((dur_ns / 1_000_000) as i64);
                    }
                }
                element_ids::REFERENCE_BLOCK => {
                    // If reference block exists, this is not a keyframe
                    is_keyframe = false;
                    let _ = self.reader.seek(SeekFrom::Current(elem_size as i64));
                }
                _ => {
                    let _ = self.reader.seek(SeekFrom::Current(elem_size as i64));
                }
            }
        }
        
        if let Some(mut p) = packet {
            p.keyframe = is_keyframe;
            p.duration_ms = duration_ms;
            Ok(Some(p))
        } else {
            Ok(None)
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration() {
        assert_eq!(mkv_format_duration_sync(0), "00:00.000");
        assert_eq!(mkv_format_duration_sync(1500), "00:01.500");
        assert_eq!(mkv_format_duration_sync(61000), "01:01.000");
        assert_eq!(mkv_format_duration_sync(3661500), "1:01:01.500");
    }

    fn mkv_format_duration_sync(duration_ms: u64) -> String {
        let total_secs = duration_ms / 1000;
        let hours = total_secs / 3600;
        let mins = (total_secs % 3600) / 60;
        let secs = total_secs % 60;
        let ms = duration_ms % 1000;

        if hours > 0 {
            format!("{}:{:02}:{:02}.{:03}", hours, mins, secs, ms)
        } else {
            format!("{:02}:{:02}.{:03}", mins, secs, ms)
        }
    }

    #[test]
    fn test_is_text_subtitle() {
        assert!(is_text_subtitle("S_TEXT/UTF8"));
        assert!(is_text_subtitle("S_TEXT/ASS"));
        assert!(is_text_subtitle("S_TEXT/SSA"));
        assert!(!is_text_subtitle("S_HDMV/PGS"));
        assert!(!is_text_subtitle("S_VOBSUB"));
    }
}
