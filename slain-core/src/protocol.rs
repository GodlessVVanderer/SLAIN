//! Protocol Handler for slain:// URLs
//!
//! Handles URLs like:
//!   slain://play?url=https://cdn.discordapp.com/...
//!   slain://open?file=C:\Videos\movie.mp4
//!   slain://test

use std::env;
use url::Url;

#[derive(Debug, Clone)]
pub enum ProtocolAction {
    /// Play a video from URL
    PlayUrl(String),
    /// Open a local file
    OpenFile(String),
    /// Test connection (just show the app)
    Test,
    /// No action / normal launch
    None,
}

/// Parse command line arguments for protocol URL
pub fn parse_protocol_args() -> ProtocolAction {
    let args: Vec<String> = env::args().collect();
    
    // Check if launched with a slain:// URL
    for arg in args.iter().skip(1) {
        if arg.starts_with("slain://") {
            return parse_slain_url(arg);
        }
        
        // Also handle direct file paths
        if is_video_file(arg) {
            return ProtocolAction::OpenFile(arg.clone());
        }
    }
    
    ProtocolAction::None
}

/// Parse a slain:// URL into an action
fn parse_slain_url(url_str: &str) -> ProtocolAction {
    // Handle simple commands
    let path = url_str.strip_prefix("slain://").unwrap_or("");
    
    match path.split('?').next().unwrap_or("") {
        "test" | "ping" => return ProtocolAction::Test,
        "" => return ProtocolAction::None,
        _ => {}
    }
    
    // Parse as URL to extract query parameters
    if let Ok(url) = Url::parse(url_str) {
        let command = url.host_str().unwrap_or("");
        
        match command {
            "play" => {
                // Get the video URL from query params
                for (key, value) in url.query_pairs() {
                    if key == "url" {
                        return ProtocolAction::PlayUrl(value.to_string());
                    }
                }
            }
            "open" => {
                for (key, value) in url.query_pairs() {
                    if key == "file" || key == "path" {
                        return ProtocolAction::OpenFile(value.to_string());
                    }
                }
            }
            _ => {
                // Treat unknown command as a URL to play
                if command.starts_with("http") || command.contains('.') {
                    // Reconstruct the URL (it was parsed as host)
                    let video_url = path.to_string();
                    return ProtocolAction::PlayUrl(video_url);
                }
            }
        }
    }
    
    ProtocolAction::None
}

/// Check if a path is a video file
fn is_video_file(path: &str) -> bool {
    let extensions = [
        ".mp4", ".mkv", ".avi", ".webm", ".mov", ".wmv", ".flv",
        ".m4v", ".ts", ".mts", ".m2ts", ".vob", ".ogv", ".3gp",
    ];
    
    let lower = path.to_lowercase();
    extensions.iter().any(|ext| lower.ends_with(ext))
}

// ============================================================================
// Tauri Commands
// ============================================================================

use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct LaunchInfo {
    pub action: String,
    pub url: Option<String>,
    pub file: Option<String>,
}


pub fn get_launch_action() -> LaunchInfo {
    match parse_protocol_args() {
        ProtocolAction::PlayUrl(url) => LaunchInfo {
            action: "play_url".to_string(),
            url: Some(url),
            file: None,
        },
        ProtocolAction::OpenFile(path) => LaunchInfo {
            action: "open_file".to_string(),
            url: None,
            file: Some(path),
        },
        ProtocolAction::Test => LaunchInfo {
            action: "test".to_string(),
            url: None,
            file: None,
        },
        ProtocolAction::None => LaunchInfo {
            action: "none".to_string(),
            url: None,
            file: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_play_url() {
        let url = "slain://play?url=https://cdn.discordapp.com/attachments/123/456/video.mp4";
        match parse_slain_url(url) {
            ProtocolAction::PlayUrl(u) => {
                assert!(u.contains("cdn.discordapp.com"));
            }
            _ => panic!("Expected PlayUrl"),
        }
    }
    
    #[test]
    fn test_parse_test() {
        assert!(matches!(parse_slain_url("slain://test"), ProtocolAction::Test));
    }
}
