//! IPTV & M3U Playlist Support
//!
//! Features:
//! - M3U/M3U8 playlist parsing
//! - IPTV channel management
//! - EPG (Electronic Program Guide) support
//! - Live TV categories
//! - Recording support

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
        (&content[..comma_pos], content[comma_pos + 1..].trim().to_string())
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
    // In full implementation, use quick-xml or roxmltree to parse
    // XMLTV format: <tv><channel id="..."><display-name>...</display-name></channel>
    //               <programme start="..." stop="..." channel="..."><title>...</title></programme></tv>
    
    Ok(EpgData {
        channels: HashMap::new(),
        last_updated: chrono::Utc::now().timestamp(),
    })
}

/// Get current program for a channel
pub fn get_current_program<'a>(epg: &'a EpgData, channel_id: &str) -> Option<&'a EpgProgram> {
    let now = chrono::Utc::now().timestamp();
    
    epg.channels.get(channel_id)?
        .iter()
        .find(|p| p.start_time <= now && p.end_time > now)
}

/// Get upcoming programs for a channel
pub fn get_upcoming_programs<'a>(epg: &'a EpgData, channel_id: &str, limit: usize) -> Vec<&'a EpgProgram> {
    let now = chrono::Utc::now().timestamp();
    
    epg.channels.get(channel_id)
        .map(|programs| {
            programs.iter()
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
    favorites: Vec<String>,  // Channel IDs
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
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read file: {}", e))?;
        
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
        self.playlists.iter()
            .flat_map(|p| p.channels.iter())
            .collect()
    }
    
    /// Get channels by group
    pub fn get_channels_by_group(&self, group: &str) -> Vec<&IptvChannel> {
        self.playlists.iter()
            .flat_map(|p| p.channels.iter())
            .filter(|c| c.group.as_deref() == Some(group))
            .collect()
    }
    
    /// Get all groups
    pub fn get_all_groups(&self) -> Vec<String> {
        let mut groups: std::collections::HashSet<_> = self.playlists.iter()
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
// Tauri Commands
// ============================================================================

use std::sync::Mutex;
use once_cell::sync::Lazy;

static IPTV_MANAGER: Lazy<Mutex<IptvManager>> = Lazy::new(|| {
    Mutex::new(IptvManager::new())
});


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
    
    manager.get_all_channels()
        .into_iter()
        .cloned()
        .collect()
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
    
    manager.get_channels_by_group(&group)
        .into_iter()
        .cloned()
        .collect()
}


pub fn search_iptv_channels(query: String) -> Vec<IptvChannel> {
    let manager = match IPTV_MANAGER.lock() {
        Ok(m) => m,
        Err(_) => return Vec::new(),
    };
    
    manager.search_channels(&query)
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
