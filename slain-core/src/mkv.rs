// MKV (Matroska/WebM) demuxer using matroska-demuxer crate
// Provides track info and frame packet reading

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek};
use std::path::Path;

use matroska_demuxer::{MatroskaFile, Frame, TrackType, TrackEntry};

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
pub struct TrackSelection {
    pub video: Option<u64>,
    pub audio: Vec<u64>,
    pub subtitles: Vec<u64>,
}

// ============================================================================
// Helper function to convert track entry
// ============================================================================

fn convert_track(track: &TrackEntry) -> MkvTrack {
    let track_number = track.track_number().get();
    let track_uid = track.track_uid().get();
    let codec_id = track.codec_id().to_string();
    let name = track.name().map(|s| s.to_string());
    let language = track.language().unwrap_or("und").to_string();
    let enabled = track.flag_enabled();
    let default = track.flag_default();
    let forced = track.flag_forced();

    match track.track_type() {
        TrackType::Video => {
            if let Some(video) = track.video() {
                MkvTrack::Video(VideoTrack {
                    track_number,
                    track_uid,
                    codec_id,
                    codec_name: None,
                    name,
                    language,
                    enabled,
                    default,
                    forced,
                    pixel_width: video.pixel_width().get() as u32,
                    pixel_height: video.pixel_height().get() as u32,
                    display_width: video.display_width().map(|w| w.get() as u32),
                    display_height: video.display_height().map(|h| h.get() as u32),
                    frame_rate: None,
                    color_space: None,
                    hdr_info: None,
                })
            } else {
                MkvTrack::Other(OtherTrack {
                    track_number,
                    track_uid,
                    track_type: 1,
                    codec_id,
                    name,
                    language,
                })
            }
        }
        TrackType::Audio => {
            if let Some(audio) = track.audio() {
                MkvTrack::Audio(AudioTrack {
                    track_number,
                    track_uid,
                    codec_id,
                    codec_name: None,
                    name,
                    language,
                    enabled,
                    default,
                    forced,
                    sample_rate: audio.sampling_frequency(),
                    channels: audio.channels().get() as u32,
                    bit_depth: audio.bit_depth().map(|d| d.get() as u32),
                })
            } else {
                MkvTrack::Other(OtherTrack {
                    track_number,
                    track_uid,
                    track_type: 2,
                    codec_id,
                    name,
                    language,
                })
            }
        }
        TrackType::Subtitle => {
            let text_based = codec_id.starts_with("S_TEXT");
            MkvTrack::Subtitle(SubtitleTrack {
                track_number,
                track_uid,
                codec_id,
                codec_name: None,
                name,
                language,
                enabled,
                default,
                forced,
                text_based,
            })
        }
        _ => MkvTrack::Other(OtherTrack {
            track_number,
            track_uid,
            track_type: 0,
            codec_id,
            name,
            language,
        }),
    }
}

// ============================================================================
// MKV Parser
// ============================================================================

pub struct MkvParser;

impl MkvParser {
    pub fn new() -> Self {
        Self
    }

    pub fn parse<P: AsRef<Path>>(&mut self, path: P) -> Result<MkvInfo, String> {
        let path = path.as_ref();
        let file = File::open(path)
            .map_err(|e| format!("Failed to open file: {}", e))?;
        let file_size = file
            .metadata()
            .map_err(|e| format!("Failed to get file metadata: {}", e))?
            .len();

        let mkv = MatroskaFile::open(file)
            .map_err(|e| format!("Failed to parse MKV: {:?}", e))?;

        // Get duration in nanoseconds, convert to ms
        let duration_ns = mkv.info().duration().unwrap_or(0.0) as u64;
        let duration_ms = duration_ns / 1_000_000;

        // Get timecode scale
        let timecode_scale = mkv.info().timestamp_scale().get();

        // Convert tracks
        let tracks: Vec<MkvTrack> = mkv.tracks().iter().map(convert_track).collect();

        Ok(MkvInfo {
            file_path: path.to_string_lossy().to_string(),
            file_size,
            duration_ms,
            title: mkv.info().title().map(|s| s.to_string()),
            muxing_app: mkv.info().muxing_app().map(|s| s.to_string()),
            writing_app: mkv.info().writing_app().map(|s| s.to_string()),
            date_utc: None,
            timecode_scale,
            tracks,
            chapters: Vec::new(),
            attachments: Vec::new(),
            tags: HashMap::new(),
            has_cues: false,
            cues: Vec::new(),
        })
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
    mkv: MatroskaFile<R>,
    frame: Frame,
    info: MkvInfo,
    video_track: Option<u64>,
    audio_track: Option<u64>,
}

impl MkvDemuxer<File> {
    /// Create new demuxer from file path
    pub fn open<P: AsRef<Path>>(path: P, info: MkvInfo) -> Result<Self, String> {
        let file = File::open(path.as_ref())
            .map_err(|e| format!("Failed to open file: {}", e))?;
        Self::new(file, info)
    }
}

impl<R: Read + Seek> MkvDemuxer<R> {
    /// Create from reader
    pub fn new(reader: R, info: MkvInfo) -> Result<Self, String> {
        let mkv = MatroskaFile::open(reader)
            .map_err(|e| format!("Failed to open MKV: {:?}", e))?;

        // Find video and audio tracks
        let mut video_track = None;
        let mut audio_track = None;

        for track in mkv.tracks() {
            if video_track.is_none() && track.track_type() == TrackType::Video {
                video_track = Some(track.track_number().get());
            }
            if audio_track.is_none() && track.track_type() == TrackType::Audio {
                audio_track = Some(track.track_number().get());
            }
        }

        Ok(Self {
            mkv,
            frame: Frame::default(),
            info,
            video_track,
            audio_track,
        })
    }

    /// Get media info
    pub fn info(&self) -> &MkvInfo {
        &self.info
    }

    /// Find video track number
    pub fn video_track(&self) -> Option<u64> {
        self.video_track
    }

    /// Find audio track number
    pub fn audio_track(&self) -> Option<u64> {
        self.audio_track
    }

    /// Read next packet
    pub fn read_packet(&mut self) -> Option<MkvPacket> {
        match self.mkv.next_frame(&mut self.frame) {
            Ok(true) => {
                // Timestamp is in nanoseconds
                let pts_ns = self.frame.timestamp as i64;
                let pts_ms = pts_ns / 1_000_000;

                Some(MkvPacket {
                    track_number: self.frame.track as u64,
                    pts_ms,
                    duration_ms: None,
                    keyframe: self.frame.is_keyframe.unwrap_or(false),
                    data: self.frame.data.clone(),
                })
            }
            Ok(false) => None, // End of file
            Err(e) => {
                tracing::warn!("MKV read error: {:?}", e);
                None
            }
        }
    }

    /// Seek (not implemented - matroska-demuxer doesn't support seeking)
    pub fn seek(&mut self, _time_ms: u64) -> Result<(), String> {
        Ok(())
    }
}

// ============================================================================
// Public API Functions
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
// Public Rust API
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

    // Fallback to first available
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
