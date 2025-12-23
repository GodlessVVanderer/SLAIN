// FORUMYZE - YouTube Comment Intelligence
// 
// "What's this?"
// ═══════════════════════════════════════════════════════════════════════════
// 
// FORUMYZE filters the garbage so you can find real comments.
// 
// YouTube comment sections are often 50-75% spam and bots - especially on
// videos about civil rights, politics, or anything controversial. Coordinated
// attacks flood comments with noise to drown out real discussion.
// 
// FORUMYZE analyzes comments and shows you:
// ✓ Real human discussions
// ✓ Genuine questions and feedback  
// ✓ Creator replies (highlighted)
// 
// And hides:
// ✗ Bot-generated spam
// ✗ Coordinated attack comments
// ✗ Duplicate/copy-paste spam
// ✗ Promotional garbage
// 
// You provide your own YouTube API key (free from Google Cloud Console).
// Your key, your quota, your privacy.
// 
// ═══════════════════════════════════════════════════════════════════════════

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use once_cell::sync::Lazy;
use std::sync::RwLock;

// ============================================================================
// User-Facing Description
// ============================================================================

pub const FORUMYZE_DESCRIPTION: &str = r#"
FORUMYZE filters YouTube comments so you can find real discussions.

Problem: Up to 75% of comments on controversial videos are spam, bots, 
or coordinated attacks designed to drown out real conversation.

Solution: FORUMYZE analyzes comments and separates:
• Real discussions from bot spam
• Genuine feedback from promotional garbage  
• Creator replies (highlighted)

You use your own YouTube API key - free from Google Cloud Console.
Your key stays on your device. We never see it.

How to get an API key:
1. Go to console.cloud.google.com
2. Create a project
3. Enable "YouTube Data API v3"
4. Create credentials → API Key
5. Paste it here

Free tier: 10,000 units/day (enough for ~100 videos)
"#;

// ============================================================================
// Settings
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumyzeSettings {
    // Master toggle
    pub enabled: bool,
    
    // API Keys (user provides their own)
    pub youtube_api_key: Option<String>,
    pub gemini_api_key: Option<String>,  // Optional: for AI analysis
    
    // What to filter
    pub hide_spam: bool,
    pub hide_bots: bool,
    pub hide_duplicates: bool,
    pub hide_coordinated: bool,      // Coordinated attack detection
    
    // What to highlight
    pub highlight_creator: bool,
    pub highlight_verified: bool,
    pub highlight_top_fans: bool,
    
    // Thresholds
    pub spam_threshold: f32,         // 0.0-1.0, default 0.6
    pub bot_threshold: f32,          // 0.0-1.0, default 0.7
    
    // Display
    pub sort_by: SortOrder,
    pub show_analysis_badges: bool,  // Show spam/bot badges
    pub compact_mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    Relevance,      // YouTube default
    Newest,
    MostLiked,
    MostReplies,
    RealFirst,      // FORUMYZE: real comments first
}

impl Default for ForumyzeSettings {
    fn default() -> Self {
        Self {
            enabled: false,  // Off by default, user opts in
            youtube_api_key: None,
            gemini_api_key: None,
            hide_spam: true,
            hide_bots: true,
            hide_duplicates: true,
            hide_coordinated: true,
            highlight_creator: true,
            highlight_verified: true,
            highlight_top_fans: true,
            spam_threshold: 0.6,
            bot_threshold: 0.7,
            sort_by: SortOrder::RealFirst,
            show_analysis_badges: true,
            compact_mode: false,
        }
    }
}

// ============================================================================
// Comment Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub author: String,
    pub author_id: String,
    pub text: String,
    pub likes: u32,
    pub replies: u32,
    pub timestamp: String,
    pub is_reply: bool,
    pub parent_id: Option<String>,
    
    // FORUMYZE analysis
    pub classification: CommentClass,
    pub spam_score: f32,
    pub bot_score: f32,
    pub is_duplicate: bool,
    pub duplicate_count: u32,
    pub is_creator: bool,
    pub is_verified: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommentClass {
    Real,           // Genuine human comment
    Spam,           // Promotional/scam
    Bot,            // Automated/bot
    Coordinated,    // Part of attack campaign
    Duplicate,      // Copy-paste spam
    Unknown,        // Not yet analyzed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoComments {
    pub video_id: String,
    pub video_title: String,
    pub total_fetched: u32,
    pub comments: Vec<Comment>,
    
    // Stats
    pub real_count: u32,
    pub spam_count: u32,
    pub bot_count: u32,
    pub duplicate_count: u32,
    pub real_percentage: f32,
}

// ============================================================================
// Analysis Engine
// ============================================================================

/// Analyze comments using heuristics (no API needed)
pub fn analyze_comments(comments: &mut [Comment]) {
    // Step 1: Build frequency maps
    let mut text_freq: HashMap<String, u32> = HashMap::new();
    let mut author_freq: HashMap<String, u32> = HashMap::new();
    
    for c in comments.iter() {
        let normalized = normalize_text(&c.text);
        *text_freq.entry(normalized).or_insert(0) += 1;
        *author_freq.entry(c.author_id.clone()).or_insert(0) += 1;
    }
    
    // Step 2: Analyze each comment
    for comment in comments.iter_mut() {
        let normalized = normalize_text(&comment.text);
        
        // Duplicate detection
        let dup_count = *text_freq.get(&normalized).unwrap_or(&1);
        comment.is_duplicate = dup_count > 2;
        comment.duplicate_count = dup_count;
        
        // Spam indicators
        let mut spam_score = 0.0f32;
        
        // Check for spam patterns
        let text_lower = comment.text.to_lowercase();
        
        // Promotional spam
        if text_lower.contains("check out my") || 
           text_lower.contains("sub to my") ||
           text_lower.contains("subscribe to my") {
            spam_score += 0.4;
        }
        
        // Scam patterns
        if text_lower.contains("bitcoin") && text_lower.contains("profit") {
            spam_score += 0.5;
        }
        if text_lower.contains("whatsapp") || text_lower.contains("telegram") {
            spam_score += 0.3;
        }
        
        // Excessive caps
        let caps_ratio = comment.text.chars()
            .filter(|c| c.is_uppercase())
            .count() as f32 / comment.text.len().max(1) as f32;
        if caps_ratio > 0.5 && comment.text.len() > 20 {
            spam_score += 0.2;
        }
        
        // Excessive emojis
        let emoji_count = comment.text.chars()
            .filter(|c| *c as u32 > 0x1F300)
            .count();
        if emoji_count > 10 {
            spam_score += 0.2;
        }
        
        // Duplicate penalty
        if comment.is_duplicate {
            spam_score += 0.3 * (dup_count as f32 / 10.0).min(1.0);
        }
        
        // Bot indicators
        let mut bot_score = 0.0f32;
        
        // Generic bot phrases
        if text_lower.contains("who else is watching in") ||
           text_lower.contains("like if you") ||
           text_lower.contains("who's here after") {
            bot_score += 0.3;
        }
        
        // High frequency poster
        let post_count = *author_freq.get(&comment.author_id).unwrap_or(&1);
        if post_count > 5 {
            bot_score += 0.2 * (post_count as f32 / 20.0).min(1.0);
        }
        
        // Very short generic comments
        if comment.text.len() < 10 && (
            text_lower == "first" ||
            text_lower == "nice" ||
            text_lower == "cool" ||
            text_lower == "lol" ||
            text_lower.starts_with("🔥")
        ) {
            bot_score += 0.2;
        }
        
        // Set scores
        comment.spam_score = spam_score.min(1.0);
        comment.bot_score = bot_score.min(1.0);
        
        // Classify
        comment.classification = if spam_score > 0.6 {
            CommentClass::Spam
        } else if bot_score > 0.7 {
            CommentClass::Bot
        } else if comment.is_duplicate && dup_count > 5 {
            CommentClass::Coordinated
        } else if comment.is_duplicate {
            CommentClass::Duplicate
        } else {
            CommentClass::Real
        };
    }
}

fn normalize_text(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Filter comments based on settings
pub fn filter_comments(comments: &[Comment], settings: &ForumyzeSettings) -> Vec<Comment> {
    comments.iter()
        .filter(|c| {
            if settings.hide_spam && c.classification == CommentClass::Spam {
                return false;
            }
            if settings.hide_bots && c.classification == CommentClass::Bot {
                return false;
            }
            if settings.hide_duplicates && c.classification == CommentClass::Duplicate {
                return false;
            }
            if settings.hide_coordinated && c.classification == CommentClass::Coordinated {
                return false;
            }
            if c.spam_score > settings.spam_threshold {
                return false;
            }
            if c.bot_score > settings.bot_threshold {
                return false;
            }
            true
        })
        .cloned()
        .collect()
}

/// Sort comments based on settings
pub fn sort_comments(comments: &mut [Comment], order: SortOrder) {
    match order {
        SortOrder::Relevance => {
            // Keep original order
        }
        SortOrder::Newest => {
            comments.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        }
        SortOrder::MostLiked => {
            comments.sort_by(|a, b| b.likes.cmp(&a.likes));
        }
        SortOrder::MostReplies => {
            comments.sort_by(|a, b| b.replies.cmp(&a.replies));
        }
        SortOrder::RealFirst => {
            // Real comments first, sorted by engagement
            comments.sort_by(|a, b| {
                let a_real = a.classification == CommentClass::Real;
                let b_real = b.classification == CommentClass::Real;
                
                match (a_real, b_real) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => {
                        // Within same class, sort by engagement
                        let a_score = a.likes + a.replies * 2;
                        let b_score = b.likes + b.replies * 2;
                        b_score.cmp(&a_score)
                    }
                }
            });
        }
    }
}

/// Calculate stats for video
pub fn calculate_stats(comments: &[Comment]) -> (u32, u32, u32, u32, f32) {
    let total = comments.len() as u32;
    let real = comments.iter().filter(|c| c.classification == CommentClass::Real).count() as u32;
    let spam = comments.iter().filter(|c| c.classification == CommentClass::Spam).count() as u32;
    let bot = comments.iter().filter(|c| c.classification == CommentClass::Bot).count() as u32;
    let dup = comments.iter().filter(|c| c.is_duplicate).count() as u32;
    let pct = if total > 0 { real as f32 / total as f32 * 100.0 } else { 0.0 };
    
    (real, spam, bot, dup, pct)
}

// ============================================================================
// YouTube API
// ============================================================================

pub async fn fetch_youtube_comments(
    video_id: &str,
    api_key: &str,
    max_results: u32,
) -> Result<Vec<Comment>, String> {
    let client = reqwest::Client::new();
    
    let url = format!(
        "https://www.googleapis.com/youtube/v3/commentThreads?\
         part=snippet,replies&videoId={}&maxResults={}&key={}",
        video_id, max_results.min(100), api_key
    );
    
    let response = client.get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("YouTube API error {}: {}", status, body));
    }
    
    let data: serde_json::Value = response.json()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;
    
    let mut comments = Vec::new();
    
    if let Some(items) = data["items"].as_array() {
        for item in items {
            let snippet = &item["snippet"]["topLevelComment"]["snippet"];
            
            comments.push(Comment {
                id: item["id"].as_str().unwrap_or("").to_string(),
                author: snippet["authorDisplayName"].as_str().unwrap_or("").to_string(),
                author_id: snippet["authorChannelId"]["value"].as_str().unwrap_or("").to_string(),
                text: snippet["textDisplay"].as_str().unwrap_or("").to_string(),
                likes: snippet["likeCount"].as_u64().unwrap_or(0) as u32,
                replies: item["snippet"]["totalReplyCount"].as_u64().unwrap_or(0) as u32,
                timestamp: snippet["publishedAt"].as_str().unwrap_or("").to_string(),
                is_reply: false,
                parent_id: None,
                classification: CommentClass::Unknown,
                spam_score: 0.0,
                bot_score: 0.0,
                is_duplicate: false,
                duplicate_count: 1,
                is_creator: false,
                is_verified: false,
            });
        }
    }
    
    Ok(comments)
}

/// Extract video ID from YouTube URL
pub fn extract_video_id(url: &str) -> Option<String> {
    // Handle various YouTube URL formats
    if url.contains("youtu.be/") {
        url.split("youtu.be/")
            .nth(1)
            .map(|s| s.split(&['?', '&'][..]).next().unwrap_or(s).to_string())
    } else if url.contains("youtube.com/watch") {
        url.split("v=")
            .nth(1)
            .map(|s| s.split(&['?', '&'][..]).next().unwrap_or(s).to_string())
    } else if url.contains("youtube.com/embed/") {
        url.split("embed/")
            .nth(1)
            .map(|s| s.split(&['?', '&'][..]).next().unwrap_or(s).to_string())
    } else if url.len() == 11 && url.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
        // Bare video ID
        Some(url.to_string())
    } else {
        None
    }
}

// ============================================================================
// Global State
// ============================================================================

static FORUMYZE_SETTINGS: Lazy<RwLock<ForumyzeSettings>> = Lazy::new(|| {
    RwLock::new(ForumyzeSettings::default())
});

// ============================================================================
// Tauri Commands
// ============================================================================


pub fn forumyze_get_description() -> &'static str {
    FORUMYZE_DESCRIPTION
}


pub fn forumyze_get_settings() -> ForumyzeSettings {
    FORUMYZE_SETTINGS.read().unwrap().clone()
}


pub fn forumyze_set_settings(settings: ForumyzeSettings) {
    *FORUMYZE_SETTINGS.write().unwrap() = settings;
}


pub fn forumyze_set_api_key(key: String) {
    FORUMYZE_SETTINGS.write().unwrap().youtube_api_key = Some(key);
}


pub fn forumyze_is_enabled() -> bool {
    FORUMYZE_SETTINGS.read().unwrap().enabled
}


pub fn forumyze_toggle(enabled: bool) {
    FORUMYZE_SETTINGS.write().unwrap().enabled = enabled;
}


pub async fn forumyze_analyze_video(video_url: String) -> Result<VideoComments, String> {
    let settings = FORUMYZE_SETTINGS.read().unwrap().clone();
    
    if !settings.enabled {
        return Err("FORUMYZE is disabled. Enable it in settings.".to_string());
    }
    
    let api_key = settings.youtube_api_key.clone()
        .ok_or("YouTube API key not configured. Add it in settings.")?;
    
    let video_id = extract_video_id(&video_url)
        .ok_or("Invalid YouTube URL")?;
    
    // Fetch comments
    let mut comments = fetch_youtube_comments(&video_id, &api_key, 100).await?;
    
    // Analyze
    analyze_comments(&mut comments);
    
    // Calculate stats before filtering
    let (real, spam, bot, dup, pct) = calculate_stats(&comments);
    
    // Filter based on settings
    let mut filtered = filter_comments(&comments, &settings);
    
    // Sort
    sort_comments(&mut filtered, settings.sort_by);
    
    Ok(VideoComments {
        video_id,
        video_title: String::new(), // Would need another API call
        total_fetched: comments.len() as u32,
        comments: filtered,
        real_count: real,
        spam_count: spam,
        bot_count: bot,
        duplicate_count: dup,
        real_percentage: pct,
    })
}


pub fn forumyze_extract_video_id(url: String) -> Option<String> {
    extract_video_id(&url)
}
