// VOICE - Native Rust Voice Control
//
// Wake word detection and command recognition
// No Python, no external services, runs locally
//
// Wake words: "SLAIN", "Hey SLAIN", "Computer"
// Public Rust API parsed locally using pattern matching

use serde::{Deserialize, Serialize};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    pub enabled: bool,
    pub wake_word: WakeWord,
    pub custom_wake_word: Option<String>,
    pub sensitivity: f32,      // 0.0-1.0
    pub timeout_secs: u32,     // How long to listen after wake word
    pub feedback_sounds: bool, // Play confirmation sounds
    pub push_to_talk: bool,    // Require key hold instead of wake word
    pub push_to_talk_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WakeWord {
    Slain,    // "SLAIN"
    HeySlain, // "Hey SLAIN"
    Computer, // "Computer"
    Custom,   // User-defined
    None,     // Push-to-talk only
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            wake_word: WakeWord::Slain,
            custom_wake_word: None,
            sensitivity: 0.5,
            timeout_secs: 5,
            feedback_sounds: true,
            push_to_talk: false,
            push_to_talk_key: "F2".to_string(),
        }
    }
}

// ============================================================================
// Voice Commands
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VoiceCommand {
    // Playback
    Play,
    Pause,
    Stop,
    Resume,

    // Navigation
    NextChapter,
    PreviousChapter,
    SkipForward(u32), // seconds
    SkipBackward(u32),
    GoToTime(u32, u32, u32), // hours, minutes, seconds

    // Volume
    VolumeUp,
    VolumeDown,
    Mute,
    Unmute,
    SetVolume(u32), // 0-100

    // Display
    Fullscreen,
    ExitFullscreen,
    ToggleFullscreen,

    // Subtitles
    SubtitlesOn,
    SubtitlesOff,
    NextSubtitle,

    // Channels (IPTV)
    ChannelUp,
    ChannelDown,
    GoToChannel(String),

    // Search
    Search(String),

    // System
    ShowHelp,
    WhatTimeIsIt,
    HowMuchLeft,

    // Meta
    CancelCommand,
    Unknown(String),
}

impl VoiceCommand {
    /// Parse text into a command
    pub fn parse(text: &str) -> Self {
        let text = text.to_lowercase().trim().to_string();

        // Playback commands
        if text == "play" || text == "start" || text == "begin" {
            return Self::Play;
        }
        if text == "pause" || text == "wait" || text == "hold" {
            return Self::Pause;
        }
        if text == "stop" || text == "end" {
            return Self::Stop;
        }
        if text == "resume" || text == "continue" {
            return Self::Resume;
        }

        // Skip commands
        if text.contains("skip") || text.contains("forward") {
            if let Some(secs) = extract_seconds(&text) {
                return Self::SkipForward(secs);
            }
            return Self::SkipForward(10); // default
        }
        if text.contains("back") || text.contains("rewind") {
            if let Some(secs) = extract_seconds(&text) {
                return Self::SkipBackward(secs);
            }
            return Self::SkipBackward(10);
        }

        // Chapter navigation
        if text.contains("next chapter") {
            return Self::NextChapter;
        }
        if text.contains("previous chapter") || text.contains("last chapter") {
            return Self::PreviousChapter;
        }

        // Volume commands
        if text.contains("volume up") || text.contains("louder") {
            return Self::VolumeUp;
        }
        if text.contains("volume down") || text.contains("quieter") || text.contains("softer") {
            return Self::VolumeDown;
        }
        if text == "mute" || text == "silence" {
            return Self::Mute;
        }
        if text == "unmute" {
            return Self::Unmute;
        }
        if text.starts_with("set volume to") || text.starts_with("volume") {
            if let Some(level) = extract_number(&text) {
                return Self::SetVolume(level.min(100));
            }
        }

        // Fullscreen
        if text == "fullscreen" || text == "full screen" {
            return Self::Fullscreen;
        }
        if text == "exit fullscreen" || text == "windowed" {
            return Self::ExitFullscreen;
        }
        if text == "toggle fullscreen" {
            return Self::ToggleFullscreen;
        }

        // Subtitles
        if text.contains("subtitle") && (text.contains("on") || text.contains("enable")) {
            return Self::SubtitlesOn;
        }
        if text.contains("subtitle") && (text.contains("off") || text.contains("disable")) {
            return Self::SubtitlesOff;
        }
        if text.contains("next subtitle") {
            return Self::NextSubtitle;
        }

        // Channel commands
        if text == "channel up" || text == "next channel" {
            return Self::ChannelUp;
        }
        if text == "channel down" || text == "previous channel" {
            return Self::ChannelDown;
        }
        if text.starts_with("go to channel") || text.starts_with("switch to") {
            let channel = text
                .replace("go to channel", "")
                .replace("switch to", "")
                .trim()
                .to_string();
            if !channel.is_empty() {
                return Self::GoToChannel(channel);
            }
        }

        // Search
        if text.starts_with("search for") || text.starts_with("find") {
            let query = text
                .replace("search for", "")
                .replace("find", "")
                .trim()
                .to_string();
            if !query.is_empty() {
                return Self::Search(query);
            }
        }

        // Time queries
        if text.contains("what time") || text.contains("current time") {
            return Self::WhatTimeIsIt;
        }
        if text.contains("how much") && text.contains("left") {
            return Self::HowMuchLeft;
        }

        // Help
        if text == "help" || text.contains("what can you do") {
            return Self::ShowHelp;
        }

        // Cancel
        if text == "cancel" || text == "never mind" || text == "forget it" {
            return Self::CancelCommand;
        }

        // Time navigation
        if text.contains("go to") || text.contains("jump to") {
            if let Some((h, m, s)) = parse_time(&text) {
                return Self::GoToTime(h, m, s);
            }
        }

        Self::Unknown(text)
    }

    /// Get a description of the command
    pub fn description(&self) -> String {
        match self {
            Self::Play => "▶ Play".to_string(),
            Self::Pause => "⏸ Pause".to_string(),
            Self::Stop => "⏹ Stop".to_string(),
            Self::Resume => "▶ Resume".to_string(),
            Self::SkipForward(s) => format!("⏩ Skip forward {}s", s),
            Self::SkipBackward(s) => format!("⏪ Skip backward {}s", s),
            Self::VolumeUp => "🔊 Volume up".to_string(),
            Self::VolumeDown => "🔉 Volume down".to_string(),
            Self::Mute => "🔇 Mute".to_string(),
            Self::Unmute => "🔊 Unmute".to_string(),
            Self::SetVolume(v) => format!("🔊 Volume {}%", v),
            Self::Fullscreen => "⛶ Fullscreen".to_string(),
            Self::NextChapter => "⏭ Next chapter".to_string(),
            Self::Search(q) => format!("🔍 Search: {}", q),
            Self::Unknown(t) => format!("❓ Unknown: {}", t),
            _ => format!("{:?}", self),
        }
    }
}

// Helper functions
fn extract_seconds(text: &str) -> Option<u32> {
    // "skip 30 seconds" or "skip 30"
    for word in text.split_whitespace() {
        if let Ok(n) = word.parse::<u32>() {
            return Some(n);
        }
    }

    // Word numbers
    if text.contains("ten") {
        return Some(10);
    }
    if text.contains("thirty") {
        return Some(30);
    }
    if text.contains("five") && !text.contains("fifteen") {
        return Some(5);
    }
    if text.contains("fifteen") {
        return Some(15);
    }

    None
}

fn extract_number(text: &str) -> Option<u32> {
    for word in text.split_whitespace() {
        if let Ok(n) = word.parse::<u32>() {
            return Some(n);
        }
    }
    None
}

fn parse_time(text: &str) -> Option<(u32, u32, u32)> {
    // "go to 1:30:00" or "jump to 5 minutes"

    // Try HH:MM:SS or MM:SS format
    let parts: Vec<&str> = text.split(&[':', ' '][..]).collect();
    let numbers: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();

    match numbers.len() {
        1 => Some((0, numbers[0], 0)),                   // Just minutes
        2 => Some((0, numbers[0], numbers[1])),          // MM:SS
        3 => Some((numbers[0], numbers[1], numbers[2])), // HH:MM:SS
        _ => None,
    }
}

// ============================================================================
// Audio Processing (Basic - uses cpal for cross-platform audio)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioLevel {
    pub rms: f32,
    pub peak: f32,
    pub is_speech: bool,
}

/// Simple voice activity detection based on energy
pub fn detect_voice_activity(samples: &[f32], threshold: f32) -> bool {
    if samples.is_empty() {
        return false;
    }

    // Calculate RMS energy
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    let rms = (sum_squares / samples.len() as f32).sqrt();

    rms > threshold
}

/// Basic wake word detection using energy + simple pattern
/// In production, you'd use a proper model like Porcupine or Snowboy
pub fn check_wake_word(samples: &[f32], wake_word: WakeWord) -> bool {
    // This is a placeholder - real wake word detection needs ML
    // For now, just detect sustained speech above threshold

    let threshold = match wake_word {
        WakeWord::None => return false,
        _ => 0.1,
    };

    // Check for speech activity
    let chunk_size = samples.len() / 10;
    let mut speech_chunks = 0;

    for chunk in samples.chunks(chunk_size) {
        if detect_voice_activity(chunk, threshold) {
            speech_chunks += 1;
        }
    }

    // Wake word detected if several consecutive chunks have speech
    speech_chunks >= 3
}

// ============================================================================
// Voice Engine State
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ListeningState {
    Idle,           // Not listening
    WaitingForWake, // Listening for wake word
    Listening,      // Active listening for command
    Processing,     // Processing command
}

pub struct VoiceEngine {
    config: Arc<RwLock<VoiceConfig>>,
    state: Arc<RwLock<ListeningState>>,
    running: Arc<AtomicBool>,
    last_command: Arc<RwLock<Option<VoiceCommand>>>,
}

impl VoiceEngine {
    pub fn new(config: VoiceConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            state: Arc::new(RwLock::new(ListeningState::Idle)),
            running: Arc::new(AtomicBool::new(false)),
            last_command: Arc::new(RwLock::new(None)),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn get_state(&self) -> ListeningState {
        *self.state.read().unwrap()
    }

    pub fn start(&self) {
        if self.running.load(Ordering::Relaxed) {
            return;
        }

        self.running.store(true, Ordering::Relaxed);
        *self.state.write().unwrap() = ListeningState::WaitingForWake;

        // In a real implementation, this would start audio capture
        // For now, we just set the state
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
        *self.state.write().unwrap() = ListeningState::Idle;
    }

    /// Simulate receiving transcribed text (from external STT)
    pub fn process_text(&self, text: &str) -> Option<VoiceCommand> {
        let config = self.config.read().unwrap();

        // Check for wake word in text
        let wake_detected = match config.wake_word {
            WakeWord::Slain => text.to_lowercase().contains("slain"),
            WakeWord::HeySlain => text.to_lowercase().contains("hey slain"),
            WakeWord::Computer => text.to_lowercase().contains("computer"),
            WakeWord::Custom => {
                if let Some(ref word) = config.custom_wake_word {
                    text.to_lowercase().contains(&word.to_lowercase())
                } else {
                    false
                }
            }
            WakeWord::None => true, // Always active (push-to-talk mode)
        };

        if !wake_detected {
            return None;
        }

        // Remove wake word from text
        let command_text = text
            .to_lowercase()
            .replace("slain", "")
            .replace("hey", "")
            .replace("computer", "")
            .trim()
            .to_string();

        if command_text.is_empty() {
            // Wake word detected but no command yet
            *self.state.write().unwrap() = ListeningState::Listening;
            return None;
        }

        // Parse command
        let command = VoiceCommand::parse(&command_text);
        *self.last_command.write().unwrap() = Some(command.clone());

        Some(command)
    }

    pub fn get_last_command(&self) -> Option<VoiceCommand> {
        self.last_command.read().unwrap().clone()
    }
}

// ============================================================================
// Global State
// ============================================================================

static VOICE_CONFIG: Lazy<RwLock<VoiceConfig>> = Lazy::new(|| RwLock::new(VoiceConfig::default()));

static VOICE_ENGINE: Lazy<VoiceEngine> = Lazy::new(|| VoiceEngine::new(VoiceConfig::default()));

// ============================================================================
// Public Rust API
// ============================================================================

pub fn voice_get_config() -> VoiceConfig {
    VOICE_CONFIG.read().unwrap().clone()
}

pub fn voice_set_config(config: VoiceConfig) {
    *VOICE_CONFIG.write().unwrap() = config;
}

pub fn voice_is_enabled() -> bool {
    VOICE_CONFIG.read().unwrap().enabled
}

pub fn voice_toggle(enabled: bool) {
    VOICE_CONFIG.write().unwrap().enabled = enabled;
    if enabled {
        VOICE_ENGINE.start();
    } else {
        VOICE_ENGINE.stop();
    }
}

pub fn voice_get_state() -> String {
    format!("{:?}", VOICE_ENGINE.get_state())
}

pub fn voice_process_text(text: String) -> Option<serde_json::Value> {
    VOICE_ENGINE.process_text(&text).map(|cmd| {
        serde_json::json!({
            "command": format!("{:?}", cmd),
            "description": cmd.description(),
        })
    })
}

pub fn voice_parse_command(text: String) -> serde_json::Value {
    let cmd = VoiceCommand::parse(&text);
    serde_json::json!({
        "command": format!("{:?}", cmd),
        "description": cmd.description(),
    })
}

pub fn voice_get_help() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({ "category": "Playback", "commands": ["play", "pause", "stop", "resume"] }),
        serde_json::json!({ "category": "Navigation", "commands": ["skip 30 seconds", "back 10 seconds", "next chapter", "go to 1:30:00"] }),
        serde_json::json!({ "category": "Volume", "commands": ["volume up", "volume down", "mute", "unmute", "set volume to 50"] }),
        serde_json::json!({ "category": "Display", "commands": ["fullscreen", "exit fullscreen"] }),
        serde_json::json!({ "category": "Subtitles", "commands": ["subtitles on", "subtitles off", "next subtitle"] }),
        serde_json::json!({ "category": "Channels", "commands": ["channel up", "channel down", "go to channel CNN"] }),
        serde_json::json!({ "category": "Search", "commands": ["search for action movies", "find comedy"] }),
    ]
}
