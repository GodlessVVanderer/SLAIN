//! Watch History & Resume Playback
//!
//! Features:
//! - Track watch progress for all videos
//! - Resume from last position
//! - Watch history browser
//! - Bookmarks within videos
//! - "Continue Watching" list

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

// ============================================================================
// Watch Progress
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchProgress {
    pub video_id: String, // Hash or path
    pub title: String,
    pub path: Option<String>,
    pub stream_url: Option<String>,
    pub position_seconds: f64,
    pub duration_seconds: f64,
    pub percent_complete: f32,
    pub completed: bool,   // Watched > 90%
    pub last_watched: i64, // Unix timestamp
    pub watch_count: u32,
    pub audio_track: Option<u32>,
    pub subtitle_track: Option<u32>,
    pub thumbnail: Option<String>,
}

impl WatchProgress {
    pub fn new(video_id: &str, title: &str, duration: f64) -> Self {
        Self {
            video_id: video_id.to_string(),
            title: title.to_string(),
            path: None,
            stream_url: None,
            position_seconds: 0.0,
            duration_seconds: duration,
            percent_complete: 0.0,
            completed: false,
            last_watched: chrono::Utc::now().timestamp(),
            watch_count: 1,
            audio_track: None,
            subtitle_track: None,
            thumbnail: None,
        }
    }

    pub fn update_position(&mut self, position: f64) {
        self.position_seconds = position;
        self.percent_complete = if self.duration_seconds > 0.0 {
            (position / self.duration_seconds * 100.0) as f32
        } else {
            0.0
        };
        self.completed = self.percent_complete > 90.0;
        self.last_watched = chrono::Utc::now().timestamp();
    }

    pub fn should_resume(&self) -> bool {
        // Resume if between 1% and 90%
        self.percent_complete > 1.0 && self.percent_complete < 90.0
    }
}

// ============================================================================
// Bookmarks
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub video_id: String,
    pub position_seconds: f64,
    pub label: String,
    pub created_at: i64,
    pub thumbnail: Option<String>,
}

// ============================================================================
// Watch History Database
// ============================================================================

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct WatchHistory {
    pub progress: HashMap<String, WatchProgress>,
    pub bookmarks: HashMap<String, Vec<Bookmark>>,
}

impl WatchHistory {
    pub fn load() -> Result<Self, String> {
        let path = history_file_path();

        if !path.exists() {
            return Ok(Self::default());
        }

        let content =
            std::fs::read_to_string(&path).map_err(|e| format!("Failed to read history: {}", e))?;

        serde_json::from_str(&content).map_err(|e| format!("Failed to parse history: {}", e))
    }

    pub fn save(&self) -> Result<(), String> {
        let path = history_file_path();

        // Ensure directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory: {}", e))?;
        }

        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize history: {}", e))?;

        std::fs::write(&path, content).map_err(|e| format!("Failed to write history: {}", e))
    }

    /// Update progress for a video
    pub fn update_progress(&mut self, video_id: &str, position: f64, duration: f64, title: &str) {
        let progress = self
            .progress
            .entry(video_id.to_string())
            .or_insert_with(|| WatchProgress::new(video_id, title, duration));

        progress.update_position(position);
        progress.duration_seconds = duration;
    }

    /// Get resume position for a video
    pub fn get_resume_position(&self, video_id: &str) -> Option<f64> {
        self.progress
            .get(video_id)
            .filter(|p| p.should_resume())
            .map(|p| p.position_seconds)
    }

    /// Get "Continue Watching" list (recently watched, not completed)
    pub fn get_continue_watching(&self, limit: usize) -> Vec<&WatchProgress> {
        let mut items: Vec<_> = self
            .progress
            .values()
            .filter(|p| p.should_resume())
            .collect();

        items.sort_by(|a, b| b.last_watched.cmp(&a.last_watched));
        items.truncate(limit);
        items
    }

    /// Get recently watched (all, including completed)
    pub fn get_recently_watched(&self, limit: usize) -> Vec<&WatchProgress> {
        let mut items: Vec<_> = self.progress.values().collect();
        items.sort_by(|a, b| b.last_watched.cmp(&a.last_watched));
        items.truncate(limit);
        items
    }

    /// Add bookmark
    pub fn add_bookmark(&mut self, video_id: &str, position: f64, label: &str) -> Bookmark {
        let bookmark = Bookmark {
            id: uuid::Uuid::new_v4().to_string(),
            video_id: video_id.to_string(),
            position_seconds: position,
            label: label.to_string(),
            created_at: chrono::Utc::now().timestamp(),
            thumbnail: None,
        };

        self.bookmarks
            .entry(video_id.to_string())
            .or_insert_with(Vec::new)
            .push(bookmark.clone());

        bookmark
    }

    /// Get bookmarks for a video
    pub fn get_bookmarks(&self, video_id: &str) -> Vec<&Bookmark> {
        self.bookmarks
            .get(video_id)
            .map(|b| b.iter().collect())
            .unwrap_or_default()
    }

    /// Remove bookmark
    pub fn remove_bookmark(&mut self, video_id: &str, bookmark_id: &str) {
        if let Some(bookmarks) = self.bookmarks.get_mut(video_id) {
            bookmarks.retain(|b| b.id != bookmark_id);
        }
    }

    /// Clear watch history for a video
    pub fn clear_video(&mut self, video_id: &str) {
        self.progress.remove(video_id);
        self.bookmarks.remove(video_id);
    }

    /// Clear all history
    pub fn clear_all(&mut self) {
        self.progress.clear();
        self.bookmarks.clear();
    }

    /// Mark as completed
    pub fn mark_completed(&mut self, video_id: &str) {
        if let Some(progress) = self.progress.get_mut(video_id) {
            progress.completed = true;
            progress.position_seconds = progress.duration_seconds;
            progress.percent_complete = 100.0;
        }
    }
}

fn history_file_path() -> PathBuf {
    let mut path = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push("SLAIN");
    path.push("watch_history.json");
    path
}

// ============================================================================
// Video ID Generation
// ============================================================================

/// Generate unique ID for a video file
pub fn generate_video_id(path: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Use file path + size for uniqueness
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);

    if let Ok(metadata) = std::fs::metadata(path) {
        metadata.len().hash(&mut hasher);
    }

    format!("{:016x}", hasher.finish())
}

/// Generate ID for streaming URL
pub fn generate_stream_id(url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

// ============================================================================
// Public Rust API
// ============================================================================

use once_cell::sync::Lazy;
use std::sync::Mutex;

static HISTORY: Lazy<Mutex<WatchHistory>> =
    Lazy::new(|| Mutex::new(WatchHistory::load().unwrap_or_default()));

pub fn update_watch_progress(
    video_id: String,
    position: f64,
    duration: f64,
    title: String,
) -> Result<(), String> {
    let mut history = HISTORY.lock().map_err(|e| e.to_string())?;
    history.update_progress(&video_id, position, duration, &title);
    history.save()
}

pub fn get_resume_position(video_id: String) -> Option<f64> {
    let history = HISTORY.lock().ok()?;
    history.get_resume_position(&video_id)
}

pub fn get_continue_watching() -> Vec<WatchProgress> {
    let history = match HISTORY.lock() {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };

    history
        .get_continue_watching(20)
        .into_iter()
        .cloned()
        .collect()
}

pub fn get_recently_watched() -> Vec<WatchProgress> {
    let history = match HISTORY.lock() {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };

    history
        .get_recently_watched(50)
        .into_iter()
        .cloned()
        .collect()
}

pub fn add_video_bookmark(
    video_id: String,
    position: f64,
    label: String,
) -> Result<Bookmark, String> {
    let mut history = HISTORY.lock().map_err(|e| e.to_string())?;
    let bookmark = history.add_bookmark(&video_id, position, &label);
    history.save()?;
    Ok(bookmark)
}

pub fn get_video_bookmarks(video_id: String) -> Vec<Bookmark> {
    let history = match HISTORY.lock() {
        Ok(h) => h,
        Err(_) => return Vec::new(),
    };

    history
        .get_bookmarks(&video_id)
        .into_iter()
        .cloned()
        .collect()
}

pub fn remove_video_bookmark(video_id: String, bookmark_id: String) -> Result<(), String> {
    let mut history = HISTORY.lock().map_err(|e| e.to_string())?;
    history.remove_bookmark(&video_id, &bookmark_id);
    history.save()
}

pub fn clear_watch_history() -> Result<(), String> {
    let mut history = HISTORY.lock().map_err(|e| e.to_string())?;
    history.clear_all();
    history.save()
}

pub fn get_video_id_for_path(path: String) -> String {
    generate_video_id(&path)
}

pub fn get_video_id_for_url(url: String) -> String {
    generate_stream_id(&url)
}
