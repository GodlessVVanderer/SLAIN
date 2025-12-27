//! IPTV & M3U Playlist Support
//!
//! Features:
//! - M3U/M3U8 playlist parsing
//! - IPTV channel management
//! - EPG (Electronic Program Guide) support
//! - Live TV categories
//! - Recording support

use chrono::TimeZone;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Channel Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IptvChannel {
    pub id: String,
    pub name: String,
    pub stream_url: String,
    pub logo_url: Option<String>,
    pub group: Option<String>,
    pub epg_id: Option<String>,
    pub country: Option<String>,
    pub language: Option<String>,
    pub is_favorite: bool,
    pub last_watched: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelGroup {
    pub name: String,
    pub channels: Vec<IptvChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IptvPlaylist {
    pub name: String,
    pub source_url: Option<String>,
    pub channels: Vec<IptvChannel>,
    pub groups: Vec<String>,
    pub last_updated: i64,
}

// ============================================================================
// M3U Parser
// ============================================================================

pub fn parse_m3u(content: &str) -> Result<IptvPlaylist, String> {
    let mut channels = Vec::new();
    let mut groups = std::collections::HashSet::new();
    let mut lines = content.lines().peekable();

    // Check for #EXTM3U header
    if let Some(first) = lines.next() {
        if !first.trim().starts_with("#EXTM3U") {
            return Err("Invalid M3U file: missing #EXTM3U header".to_string());
        }
    }

    let mut current_info: Option<ExtInf> = None;

    for line in lines {
        let line = line.trim();

        if line.is_empty() {
            continue;
        }

        if line.starts_with("#EXTINF:") {
            current_info = Some(parse_extinf(line)?);
        } else if !line.starts_with('#') {
            // This is a URL
            if let Some(info) = current_info.take() {
                if let Some(ref group) = info.group {
                    groups.insert(group.clone());
                }

                channels.push(IptvChannel {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: info.name,
                    stream_url: line.to_string(),
                    logo_url: info.logo,
                    group: info.group,
                    epg_id: info.tvg_id,
                    country: info.tvg_country,
                    language: info.tvg_language,
                    is_favorite: false,
                    last_watched: None,
                });
            }
        }
    }

    Ok(IptvPlaylist {
        name: "Imported Playlist".to_string(),
        source_url: None,
        channels,
        groups: groups.into_iter().collect(),
        last_updated: chrono::Utc::now().timestamp(),
    })
}

#[derive(Debug)]
struct ExtInf {
    duration: i32,
    name: String,
    tvg_id: Option<String>,
    tvg_name: Option<String>,
    tvg_logo: Option<String>,
    tvg_country: Option<String>,
    tvg_language: Option<String>,
    group: Option<String>,
    logo: Option<String>,
}

fn parse_extinf(line: &str) -> Result<ExtInf, String> {
    // Format: #EXTINF:duration tvg-id="..." tvg-name="..." tvg-logo="..." group-title="...",Channel Name

    let content = line.strip_prefix("#EXTINF:").unwrap_or(line);

    // Find the comma that separates attributes from channel name
    let (attrs_part, name) = if let Some(comma_pos) = content.rfind(',') {
        (
            &content[..comma_pos],
            content[comma_pos + 1..].trim().to_string(),
        )
    } else {
        (content, "Unknown Channel".to_string())
    };

    // Parse duration (first number)
    let duration: i32 = attrs_part
        .split_whitespace()
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(-1);

    // Parse attributes
    let tvg_id = extract_attribute(attrs_part, "tvg-id");
    let tvg_name = extract_attribute(attrs_part, "tvg-name");
    let tvg_logo = extract_attribute(attrs_part, "tvg-logo");
    let tvg_country = extract_attribute(attrs_part, "tvg-country");
    let tvg_language = extract_attribute(attrs_part, "tvg-language");
    let group = extract_attribute(attrs_part, "group-title");

    Ok(ExtInf {
        duration,
        name,
        tvg_id,
        tvg_name,
        tvg_logo: tvg_logo.clone(),
        tvg_country,
        tvg_language,
        group,
        logo: tvg_logo,
    })
}

fn extract_attribute(text: &str, attr_name: &str) -> Option<String> {
    let pattern = format!("{}=\"", attr_name);
    if let Some(start) = text.find(&pattern) {
        let start = start + pattern.len();
        if let Some(end) = text[start..].find('"') {
            return Some(text[start..start + end].to_string());
        }
    }
    None
}

// ============================================================================
// EPG (Electronic Program Guide)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgProgram {
    pub channel_id: String,
    pub title: String,
    pub description: Option<String>,
    pub start_time: i64,
    pub end_time: i64,
    pub category: Option<String>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EpgData {
    pub channels: HashMap<String, Vec<EpgProgram>>,
    pub last_updated: i64,
}

/// Parse XMLTV format EPG
pub fn parse_xmltv(content: &str) -> Result<EpgData, String> {
    let mut channels: HashMap<String, Vec<EpgProgram>> = HashMap::new();
    let program_re = Regex::new(
        r#"<programme[^>]*channel="(?P<channel>[^"]+)"[^>]*start="(?P<start>[^"]+)"[^>]*stop="(?P<stop>[^"]+)"[^>]*>(?s:.*?)</programme>"#,
    )
    .map_err(|e| format!("XMLTV regex error: {}", e))?;
    let title_re = Regex::new(r#"<title[^>]*>(?P<title>[^<]+)</title>"#)
        .map_err(|e| format!("XMLTV regex error: {}", e))?;
    let desc_re = Regex::new(r#"<desc[^>]*>(?P<desc>[^<]+)</desc>"#)
        .map_err(|e| format!("XMLTV regex error: {}", e))?;
    let category_re = Regex::new(r#"<category[^>]*>(?P<cat>[^<]+)</category>"#)
        .map_err(|e| format!("XMLTV regex error: {}", e))?;
    let icon_re = Regex::new(r#"<icon[^>]*src="(?P<src>[^"]+)""#)
        .map_err(|e| format!("XMLTV regex error: {}", e))?;

    for caps in program_re.captures_iter(content) {
        let channel_id = caps
            .name("channel")
            .map(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let start_time = parse_xmltv_time(caps.name("start").map(|m| m.as_str()).unwrap_or(""))?;
        let end_time = parse_xmltv_time(caps.name("stop").map(|m| m.as_str()).unwrap_or(""))?;
        let block = caps.get(0).map(|m| m.as_str()).unwrap_or("");
        let title = title_re
            .captures(block)
            .and_then(|c| c.name("title").map(|m| m.as_str().to_string()))
            .unwrap_or_else(|| "Unknown".to_string());
        let description = desc_re
            .captures(block)
            .and_then(|c| c.name("desc").map(|m| m.as_str().to_string()));
        let category = category_re
            .captures(block)
            .and_then(|c| c.name("cat").map(|m| m.as_str().to_string()));
        let icon = icon_re
            .captures(block)
            .and_then(|c| c.name("src").map(|m| m.as_str().to_string()));

        channels
            .entry(channel_id.clone())
            .or_default()
            .push(EpgProgram {
                channel_id,
                title,
                description,
                start_time,
                end_time,
                category,
                icon,
            });
    }

    Ok(EpgData {
        channels,
        last_updated: chrono::Utc::now().timestamp(),
    })
}

fn parse_xmltv_time(raw: &str) -> Result<i64, String> {
    // Format: YYYYMMDDHHMMSS + optional timezone, we parse first 14 digits.
    let digits: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 14 {
        return Err("Invalid XMLTV time format".to_string());
    }
    let year: i32 = digits[0..4].parse().map_err(|_| "Invalid year")?;
    let month: u32 = digits[4..6].parse().map_err(|_| "Invalid month")?;
    let day: u32 = digits[6..8].parse().map_err(|_| "Invalid day")?;
    let hour: u32 = digits[8..10].parse().map_err(|_| "Invalid hour")?;
    let minute: u32 = digits[10..12].parse().map_err(|_| "Invalid minute")?;
    let second: u32 = digits[12..14].parse().map_err(|_| "Invalid second")?;
    let dt = chrono::NaiveDate::from_ymd_opt(year, month, day)
        .ok_or("Invalid date")?
        .and_hms_opt(hour, minute, second)
        .ok_or("Invalid time")?;
    Ok(chrono::Utc.from_utc_datetime(&dt).timestamp())
}

/// Get current program for a channel
pub fn get_current_program<'a>(epg: &'a EpgData, channel_id: &str) -> Option<&'a EpgProgram> {
    let now = chrono::Utc::now().timestamp();

    epg.channels
        .get(channel_id)?
        .iter()
        .find(|p| p.start_time <= now && p.end_time > now)
}

/// Get upcoming programs for a channel
pub fn get_upcoming_programs<'a>(
    epg: &'a EpgData,
    channel_id: &str,
    limit: usize,
) -> Vec<&'a EpgProgram> {
    let now = chrono::Utc::now().timestamp();

    epg.channels
        .get(channel_id)
        .map(|programs| {
            programs
                .iter()
                .filter(|p| p.start_time >= now)
                .take(limit)
                .collect()
        })
        .unwrap_or_default()
}

// ============================================================================
// IPTV Manager
// ============================================================================

pub struct IptvManager {
    playlists: Vec<IptvPlaylist>,
    epg_data: Option<EpgData>,
    favorites: Vec<String>, // Channel IDs
}

impl IptvManager {
    pub fn new() -> Self {
        Self {
            playlists: Vec::new(),
            epg_data: None,
            favorites: Vec::new(),
        }
    }

    /// Load playlist from URL
    pub async fn load_playlist_url(&mut self, url: &str) -> Result<(), String> {
        let content = reqwest::get(url)
            .await
            .map_err(|e| format!("Failed to fetch playlist: {}", e))?
            .text()
            .await
            .map_err(|e| format!("Failed to read playlist: {}", e))?;

        let mut playlist = parse_m3u(&content)?;
        playlist.source_url = Some(url.to_string());
        playlist.name = url.split('/').last().unwrap_or("Playlist").to_string();

        self.playlists.push(playlist);
        Ok(())
    }

    /// Load playlist from file
    pub fn load_playlist_file(&mut self, path: &str) -> Result<(), String> {
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;

        let mut playlist = parse_m3u(&content)?;
        playlist.name = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Playlist".to_string());

        self.playlists.push(playlist);
        Ok(())
    }

    /// Load EPG from URL
    pub async fn load_epg_url(&mut self, url: &str) -> Result<(), String> {
        let content = reqwest::get(url)
            .await
            .map_err(|e| format!("Failed to fetch EPG: {}", e))?
            .text()
            .await
            .map_err(|e| format!("Failed to read EPG: {}", e))?;

        self.epg_data = Some(parse_xmltv(&content)?);
        Ok(())
    }

    /// Get all channels
    pub fn get_all_channels(&self) -> Vec<&IptvChannel> {
        self.playlists
            .iter()
            .flat_map(|p| p.channels.iter())
            .collect()
    }

    /// Get channels by group
    pub fn get_channels_by_group(&self, group: &str) -> Vec<&IptvChannel> {
        self.playlists
            .iter()
            .flat_map(|p| p.channels.iter())
            .filter(|c| c.group.as_deref() == Some(group))
            .collect()
    }

    /// Get all groups
    pub fn get_all_groups(&self) -> Vec<String> {
        let groups: std::collections::HashSet<_> = self
            .playlists
            .iter()
            .flat_map(|p| p.groups.iter().cloned())
            .collect();

        let mut sorted: Vec<_> = groups.into_iter().collect();
        sorted.sort();
        sorted
    }

    /// Search channels
    pub fn search_channels(&self, query: &str) -> Vec<&IptvChannel> {
        let query = query.to_lowercase();

        self.get_all_channels()
            .into_iter()
            .filter(|c| c.name.to_lowercase().contains(&query))
            .collect()
    }

    /// Toggle favorite
    pub fn toggle_favorite(&mut self, channel_id: &str) {
        if self.favorites.contains(&channel_id.to_string()) {
            self.favorites.retain(|id| id != channel_id);
        } else {
            self.favorites.push(channel_id.to_string());
        }
    }

    /// Get favorites
    pub fn get_favorites(&self) -> Vec<&IptvChannel> {
        self.get_all_channels()
            .into_iter()
            .filter(|c| self.favorites.contains(&c.id))
            .collect()
    }
}

// ============================================================================
// Public Rust API
// ============================================================================

use once_cell::sync::Lazy;
use std::sync::Mutex;

static IPTV_MANAGER: Lazy<Mutex<IptvManager>> = Lazy::new(|| Mutex::new(IptvManager::new()));

pub async fn load_iptv_playlist_url(url: String) -> Result<(), String> {
    let mut manager = IPTV_MANAGER.lock().map_err(|e| e.to_string())?;
    manager.load_playlist_url(&url).await
}

pub fn load_iptv_playlist_file(path: String) -> Result<(), String> {
    let mut manager = IPTV_MANAGER.lock().map_err(|e| e.to_string())?;
    manager.load_playlist_file(&path)
}

pub fn get_iptv_channels() -> Vec<IptvChannel> {
    let manager = match IPTV_MANAGER.lock() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    manager.get_all_channels().into_iter().cloned().collect()
}

pub fn get_iptv_groups() -> Vec<String> {
    let manager = match IPTV_MANAGER.lock() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    manager.get_all_groups()
}

pub fn get_iptv_channels_by_group(group: String) -> Vec<IptvChannel> {
    let manager = match IPTV_MANAGER.lock() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    manager
        .get_channels_by_group(&group)
        .into_iter()
        .cloned()
        .collect()
}

pub fn search_iptv_channels(query: String) -> Vec<IptvChannel> {
    let manager = match IPTV_MANAGER.lock() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };

    manager
        .search_channels(&query)
        .into_iter()
        .cloned()
        .collect()
}

pub fn toggle_iptv_favorite(channel_id: String) -> Result<(), String> {
    let mut manager = IPTV_MANAGER.lock().map_err(|e| e.to_string())?;
    manager.toggle_favorite(&channel_id);
    Ok(())
}

pub fn parse_m3u_content(content: String) -> Result<IptvPlaylist, String> {
    parse_m3u(&content)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_m3u_basic() {
        let content = "#EXTM3U\n#EXTINF:-1 tvg-id=\"chan1\" tvg-logo=\"http://logo\" group-title=\"News\",Channel One\nhttp://example.com/stream1\n";
        let playlist = parse_m3u(content).expect("parse m3u");
        assert_eq!(playlist.channels.len(), 1);
        let channel = &playlist.channels[0];
        assert_eq!(channel.name, "Channel One");
        assert_eq!(channel.stream_url, "http://example.com/stream1");
        assert_eq!(channel.logo_url.as_deref(), Some("http://logo"));
        assert_eq!(channel.group.as_deref(), Some("News"));
        assert_eq!(channel.epg_id.as_deref(), Some("chan1"));
        assert!(playlist.groups.contains(&"News".to_string()));
    }

    #[test]
    fn parse_m3u_requires_header() {
        let content = "#EXTINF:-1,Missing Header\nhttp://example.com/stream\n";
        let err = parse_m3u(content).expect_err("missing header should error");
        assert!(err.contains("#EXTM3U"));
    }

    #[test]
    fn manager_grouping_and_favorites() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let temp_dir = std::env::temp_dir().join(format!("slain_iptv_{}", now));
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let playlist_path = temp_dir.join("playlist.m3u");
        let content = "\
#EXTM3U\n\
#EXTINF:-1 group-title=\"Sports\",Sports One\n\
http://example.com/sports1\n\
#EXTINF:-1 group-title=\"News\",News One\n\
http://example.com/news1\n";
        fs::write(&playlist_path, content).expect("write playlist");

        let mut manager = IptvManager::new();
        manager
            .load_playlist_file(playlist_path.to_str().unwrap())
            .expect("load playlist");

        let groups = manager.get_all_groups();
        assert_eq!(groups, vec!["News".to_string(), "Sports".to_string()]);

        let channels = manager.get_all_channels();
        assert_eq!(channels.len(), 2);
        let first_id = channels[0].id.clone();
        manager.toggle_favorite(&first_id);
        let favorites = manager.get_favorites();
        assert_eq!(favorites.len(), 1);
        assert_eq!(favorites[0].id, first_id);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn parse_xmltv_basic() {
        let xml = r#"
            <tv>
              <programme start="20240101000000 +0000" stop="20240101003000 +0000" channel="ch1">
                <title>Morning News</title>
                <desc>Top stories.</desc>
                <category>News</category>
                <icon src="http://example.com/icon.png" />
              </programme>
            </tv>
        "#;
        let epg = parse_xmltv(xml).expect("parse xmltv");
        let programs = epg.channels.get("ch1").expect("channel");
        assert_eq!(programs.len(), 1);
        let program = &programs[0];
        assert_eq!(program.title, "Morning News");
        assert_eq!(program.description.as_deref(), Some("Top stories."));
        assert_eq!(program.category.as_deref(), Some("News"));
        assert_eq!(program.icon.as_deref(), Some("http://example.com/icon.png"));
        assert!(program.start_time < program.end_time);
    }
}
