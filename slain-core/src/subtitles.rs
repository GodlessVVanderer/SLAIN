//! Subtitle Support
//!
//! Formats:
//! - SRT (SubRip Text)
//! - ASS/SSA (Advanced SubStation Alpha)
//! - VobSub (DVD bitmap subtitles)
//! - PGS (Blu-ray bitmap subtitles)
//! - WebVTT
//! - Closed Captions (CEA-608/708)
//!
//! Features:
//! - Auto-detection from video file
//! - External subtitle loading
//! - OpenSubtitles.org search
//! - Subtitle timing adjustment
//! - Style customization

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Subtitle Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    pub source: SubtitleSource,
    pub format: SubtitleFormat,
    pub language: String,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    pub is_hearing_impaired: bool,
    pub cues: Vec<SubtitleCue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SubtitleSource {
    Embedded { track_index: u32 },
    External { path: String },
    OpenSubtitles { id: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SubtitleFormat {
    Srt,
    Ass,
    Ssa,
    VobSub,
    Pgs,
    WebVtt,
    Cea608,
    Cea708,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleCue {
    pub start_time: f64,    // seconds
    pub end_time: f64,
    pub text: String,
    pub style: Option<SubtitleStyle>,
    pub position: Option<SubtitlePosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleStyle {
    pub font_name: Option<String>,
    pub font_size: Option<f32>,
    pub color: Option<String>,          // "#FFFFFF"
    pub outline_color: Option<String>,
    pub outline_width: Option<f32>,
    pub shadow_color: Option<String>,
    pub shadow_depth: Option<f32>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for SubtitleStyle {
    fn default() -> Self {
        Self {
            font_name: Some("Arial".to_string()),
            font_size: Some(48.0),
            color: Some("#FFFFFF".to_string()),
            outline_color: Some("#000000".to_string()),
            outline_width: Some(2.0),
            shadow_color: Some("#000000".to_string()),
            shadow_depth: Some(2.0),
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitlePosition {
    pub x: f32,         // 0.0 - 1.0
    pub y: f32,         // 0.0 - 1.0 (0 = top, 1 = bottom)
    pub alignment: TextAlignment,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum TextAlignment {
    Left,
    Center,
    Right,
}

// ============================================================================
// SRT Parser
// ============================================================================

pub fn parse_srt(content: &str) -> Result<Vec<SubtitleCue>, String> {
    let mut cues = Vec::new();
    let mut lines = content.lines().peekable();
    
    while lines.peek().is_some() {
        // Skip empty lines and index number
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() || line.trim().parse::<u32>().is_ok() {
                lines.next();
            } else {
                break;
            }
        }
        
        // Parse timestamp line: "00:00:01,234 --> 00:00:04,567"
        let timestamp_line = match lines.next() {
            Some(l) => l,
            None => break,
        };
        
        let (start, end) = parse_srt_timestamp(timestamp_line)?;
        
        // Collect text lines until empty line
        let mut text_lines = Vec::new();
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                break;
            }
            text_lines.push(lines.next().unwrap().to_string());
        }
        
        if !text_lines.is_empty() {
            cues.push(SubtitleCue {
                start_time: start,
                end_time: end,
                text: text_lines.join("\n"),
                style: None,
                position: None,
            });
        }
    }
    
    Ok(cues)
}

fn parse_srt_timestamp(line: &str) -> Result<(f64, f64), String> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return Err("Invalid timestamp format".to_string());
    }
    
    let start = parse_srt_time(parts[0].trim())?;
    let end = parse_srt_time(parts[1].trim())?;
    
    Ok((start, end))
}

fn parse_srt_time(time_str: &str) -> Result<f64, String> {
    // Format: "00:00:01,234" or "00:00:01.234"
    let time_str = time_str.replace(',', ".");
    let parts: Vec<&str> = time_str.split(':').collect();
    
    if parts.len() != 3 {
        return Err("Invalid time format".to_string());
    }
    
    let hours: f64 = parts[0].parse().map_err(|_| "Invalid hours")?;
    let minutes: f64 = parts[1].parse().map_err(|_| "Invalid minutes")?;
    let seconds: f64 = parts[2].parse().map_err(|_| "Invalid seconds")?;
    
    Ok(hours * 3600.0 + minutes * 60.0 + seconds)
}

// ============================================================================
// ASS/SSA Parser
// ============================================================================

pub fn parse_ass(content: &str) -> Result<Vec<SubtitleCue>, String> {
    let mut cues = Vec::new();
    let mut in_events = false;
    
    for line in content.lines() {
        let line = line.trim();
        
        if line == "[Events]" {
            in_events = true;
            continue;
        }
        
        if line.starts_with('[') && line != "[Events]" {
            in_events = false;
            continue;
        }
        
        if in_events && line.starts_with("Dialogue:") {
            if let Ok(cue) = parse_ass_dialogue(line) {
                cues.push(cue);
            }
        }
    }
    
    Ok(cues)
}

fn parse_ass_dialogue(line: &str) -> Result<SubtitleCue, String> {
    // Format: Dialogue: 0,0:00:01.00,0:00:04.00,Default,,0,0,0,,Text here
    let content = line.strip_prefix("Dialogue:").unwrap_or(line);
    let parts: Vec<&str> = content.splitn(10, ',').collect();
    
    if parts.len() < 10 {
        return Err("Invalid dialogue format".to_string());
    }
    
    let start = parse_ass_time(parts[1].trim())?;
    let end = parse_ass_time(parts[2].trim())?;
    let text = parts[9].replace("\\N", "\n").replace("\\n", "\n");
    
    // Strip ASS formatting codes like {\i1}, {\b1}, etc.
    let text = strip_ass_codes(&text);
    
    Ok(SubtitleCue {
        start_time: start,
        end_time: end,
        text,
        style: None,
        position: None,
    })
}

fn parse_ass_time(time_str: &str) -> Result<f64, String> {
    // Format: "0:00:01.00"
    let parts: Vec<&str> = time_str.split(':').collect();
    
    if parts.len() != 3 {
        return Err("Invalid ASS time format".to_string());
    }
    
    let hours: f64 = parts[0].parse().map_err(|_| "Invalid hours")?;
    let minutes: f64 = parts[1].parse().map_err(|_| "Invalid minutes")?;
    let seconds: f64 = parts[2].parse().map_err(|_| "Invalid seconds")?;
    
    Ok(hours * 3600.0 + minutes * 60.0 + seconds)
}

fn strip_ass_codes(text: &str) -> String {
    let re = regex::Regex::new(r"\{[^}]*\}").unwrap();
    re.replace_all(text, "").to_string()
}

// ============================================================================
// WebVTT Parser
// ============================================================================

pub fn parse_vtt(content: &str) -> Result<Vec<SubtitleCue>, String> {
    let mut cues = Vec::new();
    let mut lines = content.lines().peekable();
    
    // Skip WEBVTT header
    while let Some(line) = lines.peek() {
        if line.contains("-->") {
            break;
        }
        lines.next();
    }
    
    while lines.peek().is_some() {
        // Skip empty lines and cue identifiers
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() || (!line.contains("-->") && !line.trim().starts_with("NOTE")) {
                lines.next();
            } else {
                break;
            }
        }
        
        // Parse timestamp
        let timestamp_line = match lines.next() {
            Some(l) if l.contains("-->") => l,
            _ => break,
        };
        
        let (start, end) = parse_vtt_timestamp(timestamp_line)?;
        
        // Collect text
        let mut text_lines = Vec::new();
        while let Some(line) = lines.peek() {
            if line.trim().is_empty() {
                break;
            }
            text_lines.push(lines.next().unwrap().to_string());
        }
        
        if !text_lines.is_empty() {
            cues.push(SubtitleCue {
                start_time: start,
                end_time: end,
                text: text_lines.join("\n"),
                style: None,
                position: None,
            });
        }
    }
    
    Ok(cues)
}

fn parse_vtt_timestamp(line: &str) -> Result<(f64, f64), String> {
    let parts: Vec<&str> = line.split("-->").collect();
    if parts.len() != 2 {
        return Err("Invalid VTT timestamp".to_string());
    }
    
    let start = parse_vtt_time(parts[0].trim())?;
    let end_part = parts[1].split_whitespace().next().unwrap_or("");
    let end = parse_vtt_time(end_part)?;
    
    Ok((start, end))
}

fn parse_vtt_time(time_str: &str) -> Result<f64, String> {
    // Format: "00:00:01.234" or "00:01.234"
    let parts: Vec<&str> = time_str.split(':').collect();
    
    match parts.len() {
        2 => {
            let minutes: f64 = parts[0].parse().map_err(|_| "Invalid minutes")?;
            let seconds: f64 = parts[1].parse().map_err(|_| "Invalid seconds")?;
            Ok(minutes * 60.0 + seconds)
        }
        3 => {
            let hours: f64 = parts[0].parse().map_err(|_| "Invalid hours")?;
            let minutes: f64 = parts[1].parse().map_err(|_| "Invalid minutes")?;
            let seconds: f64 = parts[2].parse().map_err(|_| "Invalid seconds")?;
            Ok(hours * 3600.0 + minutes * 60.0 + seconds)
        }
        _ => Err("Invalid VTT time format".to_string()),
    }
}

// ============================================================================
// Auto-detect External Subtitles
// ============================================================================

pub fn find_external_subtitles(video_path: &str) -> Vec<SubtitleFile> {
    let video_path = PathBuf::from(video_path);
    let parent = match video_path.parent() {
        Some(p) => p,
        None => return Vec::new(),
    };
    
    let video_stem = video_path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    
    let subtitle_extensions = ["srt", "ass", "ssa", "sub", "vtt"];
    let mut found = Vec::new();
    
    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let path = entry.path();
            
            if let Some(ext) = path.extension() {
                let ext_str = ext.to_string_lossy().to_lowercase();
                
                if subtitle_extensions.contains(&ext_str.as_str()) {
                    let file_stem = path.file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    
                    // Check if subtitle matches video name
                    if file_stem.starts_with(&video_stem) {
                        // Try to extract language from filename
                        // e.g., "Movie.en.srt", "Movie.eng.srt"
                        let lang = extract_language_from_filename(&file_stem, &video_stem);
                        
                        found.push(SubtitleFile {
                            path: path.to_string_lossy().to_string(),
                            format: match ext_str.as_str() {
                                "srt" => SubtitleFormat::Srt,
                                "ass" | "ssa" => SubtitleFormat::Ass,
                                "vtt" => SubtitleFormat::WebVtt,
                                _ => SubtitleFormat::Srt,
                            },
                            language: lang,
                        });
                    }
                }
            }
        }
    }
    
    found
}

fn extract_language_from_filename(sub_name: &str, video_name: &str) -> String {
    let suffix = sub_name.strip_prefix(video_name).unwrap_or("");
    let parts: Vec<&str> = suffix.split('.').filter(|s| !s.is_empty()).collect();
    
    if let Some(lang) = parts.first() {
        // Common language codes
        match lang.to_lowercase().as_str() {
            "en" | "eng" | "english" => "English".to_string(),
            "es" | "spa" | "spanish" => "Spanish".to_string(),
            "fr" | "fra" | "french" => "French".to_string(),
            "de" | "deu" | "german" => "German".to_string(),
            "ja" | "jpn" | "japanese" => "Japanese".to_string(),
            "ko" | "kor" | "korean" => "Korean".to_string(),
            "zh" | "chi" | "chinese" => "Chinese".to_string(),
            "pt" | "por" | "portuguese" => "Portuguese".to_string(),
            "ru" | "rus" | "russian" => "Russian".to_string(),
            "it" | "ita" | "italian" => "Italian".to_string(),
            "ar" | "ara" | "arabic" => "Arabic".to_string(),
            _ => lang.to_string(),
        }
    } else {
        "Unknown".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubtitleFile {
    pub path: String,
    pub format: SubtitleFormat,
    pub language: String,
}

// ============================================================================
// OpenSubtitles.org API
// ============================================================================

const OPENSUBTITLES_API: &str = "https://api.opensubtitles.com/api/v1";

pub struct OpenSubtitlesClient {
    api_key: String,
    client: reqwest::Client,
}

impl OpenSubtitlesClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::new(),
        }
    }
    
    /// Search for subtitles by movie hash and filesize
    pub async fn search_by_hash(&self, hash: &str, filesize: u64) -> Result<Vec<OpenSubtitle>, String> {
        let url = format!(
            "{}/subtitles?moviehash={}&moviebytesize={}",
            OPENSUBTITLES_API, hash, filesize
        );
        
        self.search_request(&url).await
    }
    
    /// Search by IMDB ID
    pub async fn search_by_imdb(&self, imdb_id: &str, language: Option<&str>) -> Result<Vec<OpenSubtitle>, String> {
        let mut url = format!(
            "{}/subtitles?imdb_id={}",
            OPENSUBTITLES_API, imdb_id.trim_start_matches("tt")
        );
        
        if let Some(lang) = language {
            url.push_str(&format!("&languages={}", lang));
        }
        
        self.search_request(&url).await
    }
    
    /// Search by query
    pub async fn search_by_query(&self, query: &str, language: Option<&str>) -> Result<Vec<OpenSubtitle>, String> {
        let mut url = format!(
            "{}/subtitles?query={}",
            OPENSUBTITLES_API, urlencoding::encode(query)
        );
        
        if let Some(lang) = language {
            url.push_str(&format!("&languages={}", lang));
        }
        
        self.search_request(&url).await
    }
    
    async fn search_request(&self, url: &str) -> Result<Vec<OpenSubtitle>, String> {
        let response = self.client
            .get(url)
            .header("Api-Key", &self.api_key)
            .header("User-Agent", "SLAIN/1.0")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        
        let result: OpenSubtitlesResponse = response
            .json()
            .await
            .map_err(|e| e.to_string())?;
        
        Ok(result.data)
    }
    
    /// Download subtitle
    pub async fn download(&self, file_id: u64) -> Result<String, String> {
        let url = format!("{}/download", OPENSUBTITLES_API);
        
        let response = self.client
            .post(&url)
            .header("Api-Key", &self.api_key)
            .json(&serde_json::json!({ "file_id": file_id }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        
        let result: DownloadResponse = response
            .json()
            .await
            .map_err(|e| e.to_string())?;
        
        // Fetch the actual subtitle content
        let content = self.client
            .get(&result.link)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        
        Ok(content)
    }
}

#[derive(Debug, Deserialize)]
struct OpenSubtitlesResponse {
    data: Vec<OpenSubtitle>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSubtitle {
    pub id: String,
    pub attributes: OpenSubtitleAttributes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSubtitleAttributes {
    pub language: String,
    pub download_count: u64,
    pub hearing_impaired: bool,
    pub fps: Option<f32>,
    pub votes: i32,
    pub ratings: f32,
    pub release: Option<String>,
    pub files: Vec<OpenSubtitleFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenSubtitleFile {
    pub file_id: u64,
    pub file_name: String,
}

#[derive(Debug, Deserialize)]
struct DownloadResponse {
    link: String,
}

// ============================================================================
// Subtitle Timing Adjustment
// ============================================================================

/// Shift all subtitles by offset (positive = delay, negative = earlier)
pub fn shift_subtitles(cues: &mut [SubtitleCue], offset_seconds: f64) {
    for cue in cues {
        cue.start_time = (cue.start_time + offset_seconds).max(0.0);
        cue.end_time = (cue.end_time + offset_seconds).max(0.0);
    }
}

/// Scale subtitle timing (for framerate conversion)
pub fn scale_subtitles(cues: &mut [SubtitleCue], factor: f64) {
    for cue in cues {
        cue.start_time *= factor;
        cue.end_time *= factor;
    }
}

// ============================================================================
// Public API
// ============================================================================


pub fn load_subtitle_file(path: String) -> Result<Vec<SubtitleCue>, String> {
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read subtitle file: {}", e))?;
    
    let ext = PathBuf::from(&path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    
    match ext.as_str() {
        "srt" => parse_srt(&content),
        "ass" | "ssa" => parse_ass(&content),
        "vtt" => parse_vtt(&content),
        _ => Err("Unsupported subtitle format".to_string()),
    }
}


pub fn find_subtitles_for_video(video_path: String) -> Vec<SubtitleFile> {
    find_external_subtitles(&video_path)
}


pub async fn search_opensubtitles(
    query: String,
    imdb_id: Option<String>,
    language: Option<String>,
    api_key: String,
) -> Result<Vec<OpenSubtitle>, String> {
    let client = OpenSubtitlesClient::new(&api_key);
    
    if let Some(imdb) = imdb_id {
        client.search_by_imdb(&imdb, language.as_deref()).await
    } else {
        client.search_by_query(&query, language.as_deref()).await
    }
}


pub async fn download_opensubtitle(
    file_id: u64,
    save_path: String,
    api_key: String,
) -> Result<String, String> {
    let client = OpenSubtitlesClient::new(&api_key);
    let content = client.download(file_id).await?;
    
    std::fs::write(&save_path, &content)
        .map_err(|e| format!("Failed to save subtitle: {}", e))?;
    
    Ok(save_path)
}
