// FORUMYZE - YouTube Comment Analysis for SLAIN
// Users provide their own YouTube API key and optionally Gemini API key
// Integrated into SLAIN video player for YouTube playback

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

// ============================================================================
// DATA TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeComment {
    pub id: String,
    pub author: String,
    pub author_image: String,
    pub author_channel: String,
    pub text: String,
    pub published_at: String,
    pub like_count: u64,
    pub reply_count: u64,
    pub replies: Vec<YouTubeComment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_video_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub video_id: String,
    pub title: String,
    pub channel_name: String,
    pub channel_id: String,
    pub description: String,
    pub published_at: String,
    pub view_count: u64,
    pub like_count: u64,
    pub comment_count: u64,
    pub duration_seconds: u64,
    pub thumbnail_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Discussion {
    pub id: String,
    pub title: String,
    pub description: String,
    pub comment_ids: Vec<String>,
    pub total_comments: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumyzeResult {
    pub video: VideoMetadata,
    pub total_comments: usize,
    pub filtered_comments: usize,
    pub discussions: Vec<Discussion>,
    pub core_dialogue: Vec<YouTubeComment>,
    pub questions: Vec<YouTubeComment>,
    pub signal_noise: Vec<YouTubeComment>,
    pub anomalies: Vec<YouTubeComment>,
    pub topics: Vec<String>,
    pub stats: FilterStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilterStats {
    pub total: usize,
    pub kept: usize,
    pub too_short: usize,
    pub spam_phrases: usize,
    pub emotion_only: usize,
    pub similar: usize,
    pub replies_removed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserApiKeys {
    pub youtube_api_key: String,
    pub gemini_api_key: Option<String>,
}

// ============================================================================
// GOOGLE OAUTH
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleUser {
    pub google_id: String,
    pub email: String,
    pub name: String,
    pub picture: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: u64,
}

/// Google OAuth configuration
pub struct GoogleOAuth {
    client_id: String,
    client_secret: String,
    redirect_uri: String,
}

impl GoogleOAuth {
    pub fn new(client_id: String, client_secret: String, redirect_uri: String) -> Self {
        Self { client_id, client_secret, redirect_uri }
    }
    
    /// Generate OAuth URL for user to visit
    pub fn get_auth_url(&self, state: &str) -> String {
        let scopes = [
            "openid",
            "email", 
            "profile",
            "https://www.googleapis.com/auth/youtube.readonly",
        ].join(" ");
        
        format!(
            "https://accounts.google.com/o/oauth2/v2/auth?\
            client_id={}&\
            redirect_uri={}&\
            response_type=code&\
            scope={}&\
            access_type=offline&\
            prompt=consent&\
            state={}",
            urlencoding::encode(&self.client_id),
            urlencoding::encode(&self.redirect_uri),
            urlencoding::encode(&scopes),
            urlencoding::encode(state)
        )
    }
    
    /// Exchange authorization code for tokens
    pub async fn exchange_code(&self, code: &str) -> Result<OAuthTokens, String> {
        let client = reqwest::Client::new();
        
        let params = [
            ("code", code),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("redirect_uri", &self.redirect_uri),
            ("grant_type", "authorization_code"),
        ];
        
        let response = client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Token exchange failed: {}", e))?;
        
        if !response.status().is_success() {
            let error = response.text().await.unwrap_or_default();
            return Err(format!("Token exchange failed: {}", error));
        }
        
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            refresh_token: Option<String>,
            expires_in: u64,
        }
        
        let tokens: TokenResponse = response.json().await
            .map_err(|e| format!("Failed to parse tokens: {}", e))?;
        
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() + tokens.expires_in;
        
        Ok(OAuthTokens {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at,
        })
    }
    
    /// Get user info from access token
    pub async fn get_user_info(&self, access_token: &str) -> Result<GoogleUser, String> {
        let client = reqwest::Client::new();
        
        let response = client
            .get("https://www.googleapis.com/oauth2/v2/userinfo")
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|e| format!("Failed to get user info: {}", e))?;
        
        if !response.status().is_success() {
            return Err("Failed to get user info".to_string());
        }
        
        #[derive(Deserialize)]
        struct UserInfo {
            id: String,
            email: String,
            name: String,
            picture: String,
        }
        
        let info: UserInfo = response.json().await
            .map_err(|e| format!("Failed to parse user info: {}", e))?;
        
        Ok(GoogleUser {
            google_id: info.id,
            email: info.email,
            name: info.name,
            picture: info.picture,
        })
    }
    
    /// Refresh access token
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<OAuthTokens, String> {
        let client = reqwest::Client::new();
        
        let params = [
            ("refresh_token", refresh_token),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("grant_type", "refresh_token"),
        ];
        
        let response = client
            .post("https://oauth2.googleapis.com/token")
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Token refresh failed: {}", e))?;
        
        if !response.status().is_success() {
            return Err("Token refresh failed".to_string());
        }
        
        #[derive(Deserialize)]
        struct TokenResponse {
            access_token: String,
            expires_in: u64,
        }
        
        let tokens: TokenResponse = response.json().await
            .map_err(|e| format!("Failed to parse tokens: {}", e))?;
        
        let expires_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() + tokens.expires_in;
        
        Ok(OAuthTokens {
            access_token: tokens.access_token,
            refresh_token: Some(refresh_token.to_string()),
            expires_at,
        })
    }
}

// ============================================================================
// YOUTUBE API CLIENT
// ============================================================================

pub struct YouTubeClient {
    api_key: String,
    client: reqwest::Client,
}

impl YouTubeClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }
    
    /// Extract video ID from various YouTube URL formats
    pub fn extract_video_id(url: &str) -> Option<String> {
        let patterns = [
            r"(?:youtube\.com/watch\?v=|youtu\.be/)([^&\n?#]+)",
            r"youtube\.com/embed/([^&\n?#]+)",
            r"youtube\.com/v/([^&\n?#]+)",
            r"youtube\.com/shorts/([^&\n?#]+)",
        ];
        
        for pattern in patterns {
            if let Ok(re) = regex::Regex::new(pattern) {
                if let Some(caps) = re.captures(url) {
                    if let Some(id) = caps.get(1) {
                        return Some(id.as_str().to_string());
                    }
                }
            }
        }
        
        // Maybe it's already just a video ID
        if url.len() == 11 && url.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Some(url.to_string());
        }
        
        None
    }
    
    /// Get video metadata
    pub async fn get_video_metadata(&self, video_id: &str) -> Result<VideoMetadata, String> {
        let url = format!(
            "https://www.googleapis.com/youtube/v3/videos?\
            part=snippet,statistics,contentDetails&\
            id={}&key={}",
            video_id, self.api_key
        );
        
        let response = self.client.get(&url).send().await
            .map_err(|e| format!("API request failed: {}", e))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await.unwrap_or_default();
            return Err(format!("YouTube API error {}: {}", status, error));
        }
        
        #[derive(Deserialize)]
        struct VideoResponse {
            items: Vec<VideoItem>,
        }
        
        #[derive(Deserialize)]
        struct VideoItem {
            snippet: Snippet,
            statistics: Statistics,
            #[serde(rename = "contentDetails")]
            content_details: ContentDetails,
        }
        
        #[derive(Deserialize)]
        struct Snippet {
            title: String,
            #[serde(rename = "channelTitle")]
            channel_title: String,
            #[serde(rename = "channelId")]
            channel_id: String,
            description: String,
            #[serde(rename = "publishedAt")]
            published_at: String,
        }
        
        #[derive(Deserialize)]
        struct Statistics {
            #[serde(rename = "viewCount", default)]
            view_count: String,
            #[serde(rename = "likeCount", default)]
            like_count: String,
            #[serde(rename = "commentCount", default)]
            comment_count: String,
        }
        
        #[derive(Deserialize)]
        struct ContentDetails {
            duration: String,
        }
        
        let data: VideoResponse = response.json().await
            .map_err(|e| format!("Failed to parse response: {}", e))?;
        
        let video = data.items.into_iter().next()
            .ok_or_else(|| "Video not found".to_string())?;
        
        // Parse ISO 8601 duration (PT1H2M3S)
        let duration_seconds = parse_iso_duration(&video.content_details.duration);
        
        Ok(VideoMetadata {
            video_id: video_id.to_string(),
            title: video.snippet.title,
            channel_name: video.snippet.channel_title,
            channel_id: video.snippet.channel_id,
            description: video.snippet.description,
            published_at: video.snippet.published_at,
            view_count: video.statistics.view_count.parse().unwrap_or(0),
            like_count: video.statistics.like_count.parse().unwrap_or(0),
            comment_count: video.statistics.comment_count.parse().unwrap_or(0),
            duration_seconds,
            thumbnail_url: format!("https://img.youtube.com/vi/{}/maxresdefault.jpg", video_id),
        })
    }
    
    /// Fetch all comments for a video
    pub async fn fetch_comments(&self, video_id: &str, max_comments: usize) -> Result<Vec<YouTubeComment>, String> {
        let mut comments = Vec::new();
        let mut page_token: Option<String> = None;
        
        while comments.len() < max_comments {
            let mut url = format!(
                "https://www.googleapis.com/youtube/v3/commentThreads?\
                part=snippet,replies&\
                videoId={}&\
                maxResults=100&\
                textFormat=plainText&\
                order=relevance&\
                key={}",
                video_id, self.api_key
            );
            
            if let Some(token) = &page_token {
                url.push_str(&format!("&pageToken={}", token));
            }
            
            let response = self.client.get(&url).send().await
                .map_err(|e| format!("API request failed: {}", e))?;
            
            if !response.status().is_success() {
                let status = response.status();
                let error = response.text().await.unwrap_or_default();
                
                // Check for comments disabled
                if error.contains("commentsDisabled") {
                    return Err("Comments are disabled on this video".to_string());
                }
                
                return Err(format!("YouTube API error {}: {}", status, error));
            }
            
            #[derive(Deserialize)]
            struct CommentsResponse {
                items: Vec<CommentThread>,
                #[serde(rename = "nextPageToken")]
                next_page_token: Option<String>,
            }
            
            #[derive(Deserialize)]
            struct CommentThread {
                id: String,
                snippet: ThreadSnippet,
                replies: Option<Replies>,
            }
            
            #[derive(Deserialize)]
            struct ThreadSnippet {
                #[serde(rename = "topLevelComment")]
                top_level_comment: Comment,
                #[serde(rename = "totalReplyCount")]
                total_reply_count: u64,
            }
            
            #[derive(Deserialize)]
            struct Comment {
                snippet: CommentSnippet,
            }
            
            #[derive(Deserialize)]
            struct CommentSnippet {
                #[serde(rename = "authorDisplayName")]
                author_display_name: String,
                #[serde(rename = "authorProfileImageUrl", default)]
                author_profile_image_url: String,
                #[serde(rename = "authorChannelUrl", default)]
                author_channel_url: String,
                #[serde(rename = "textOriginal", default)]
                text_original: String,
                #[serde(rename = "textDisplay", default)]
                text_display: String,
                #[serde(rename = "publishedAt")]
                published_at: String,
                #[serde(rename = "likeCount", default)]
                like_count: u64,
            }
            
            #[derive(Deserialize)]
            struct Replies {
                comments: Vec<ReplyComment>,
            }
            
            #[derive(Deserialize)]
            struct ReplyComment {
                id: String,
                snippet: CommentSnippet,
            }
            
            let data: CommentsResponse = response.json().await
                .map_err(|e| format!("Failed to parse comments: {}", e))?;
            
            for item in data.items {
                let snippet = &item.snippet.top_level_comment.snippet;
                
                // Parse replies
                let replies: Vec<YouTubeComment> = item.replies
                    .map(|r| r.comments.into_iter().map(|reply| {
                        YouTubeComment {
                            id: reply.id,
                            author: reply.snippet.author_display_name,
                            author_image: reply.snippet.author_profile_image_url,
                            author_channel: reply.snippet.author_channel_url,
                            text: if reply.snippet.text_original.is_empty() {
                                reply.snippet.text_display
                            } else {
                                reply.snippet.text_original
                            },
                            published_at: reply.snippet.published_at,
                            like_count: reply.snippet.like_count,
                            reply_count: 0,
                            replies: vec![],
                            source_video_id: Some(video_id.to_string()),
                            source_type: Some("main".to_string()),
                        }
                    }).collect())
                    .unwrap_or_default();
                
                comments.push(YouTubeComment {
                    id: item.id,
                    author: snippet.author_display_name.clone(),
                    author_image: snippet.author_profile_image_url.clone(),
                    author_channel: snippet.author_channel_url.clone(),
                    text: if snippet.text_original.is_empty() {
                        snippet.text_display.clone()
                    } else {
                        snippet.text_original.clone()
                    },
                    published_at: snippet.published_at.clone(),
                    like_count: snippet.like_count,
                    reply_count: item.snippet.total_reply_count,
                    replies,
                    source_video_id: Some(video_id.to_string()),
                    source_type: Some("main".to_string()),
                });
                
                if comments.len() >= max_comments {
                    break;
                }
            }
            
            page_token = data.next_page_token;
            if page_token.is_none() {
                break;
            }
            
            // Rate limiting
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        
        Ok(comments)
    }
}

/// Parse ISO 8601 duration (PT1H2M3S) to seconds
fn parse_iso_duration(duration: &str) -> u64 {
    let mut seconds = 0u64;
    let mut num = String::new();
    
    for c in duration.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: u64 = num.parse().unwrap_or(0);
            num.clear();
            
            match c {
                'H' => seconds += n * 3600,
                'M' => seconds += n * 60,
                'S' => seconds += n,
                _ => {}
            }
        }
    }
    
    seconds
}

// ============================================================================
// COMMENT FILTER
// ============================================================================

pub struct CommentFilter {
    spam_phrases: HashSet<String>,
    min_length: usize,
}

impl Default for CommentFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl CommentFilter {
    pub fn new() -> Self {
        let spam_phrases: HashSet<String> = [
            "sub to me",
            "subscribe to my channel",
            "check out my channel",
            "who's watching in",
            "like if you",
            "first comment",
            "first!",
            "notification squad",
            "early gang",
            "who else is watching",
            "edit: omg thanks",
            "edit: thanks for the likes",
            "nobody:",
            "no one:",
            "literally no one:",
        ].iter().map(|s| s.to_lowercase()).collect();
        
        Self {
            spam_phrases,
            min_length: 15,
        }
    }
    
    /// Filter comments, returning (kept, stats)
    pub fn filter(&self, comments: Vec<YouTubeComment>) -> (Vec<YouTubeComment>, FilterStats) {
        let mut stats = FilterStats {
            total: comments.len(),
            ..Default::default()
        };
        
        let mut kept = Vec::new();
        let mut seen_texts: HashSet<String> = HashSet::new();
        
        for comment in comments {
            let text_lower = comment.text.to_lowercase();
            let text_normalized = normalize_text(&text_lower);
            
            // Too short
            if comment.text.len() < self.min_length {
                stats.too_short += 1;
                continue;
            }
            
            // Spam phrases
            if self.spam_phrases.iter().any(|phrase| text_lower.contains(phrase)) {
                stats.spam_phrases += 1;
                continue;
            }
            
            // Emotion only (just emojis/punctuation)
            let alpha_count = comment.text.chars().filter(|c| c.is_alphabetic()).count();
            if alpha_count < 5 {
                stats.emotion_only += 1;
                continue;
            }
            
            // Similar/duplicate
            if seen_texts.contains(&text_normalized) {
                stats.similar += 1;
                continue;
            }
            
            seen_texts.insert(text_normalized);
            stats.kept += 1;
            kept.push(comment);
        }
        
        (kept, stats)
    }
}

/// Normalize text for similarity comparison
fn normalize_text(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

// ============================================================================
// AI ANALYZER (Local or Gemini)
// ============================================================================

pub struct CommentAnalyzer {
    gemini_key: Option<String>,
    client: reqwest::Client,
}

impl CommentAnalyzer {
    pub fn new(gemini_key: Option<String>) -> Self {
        Self {
            gemini_key,
            client: reqwest::Client::new(),
        }
    }
    
    /// Analyze comments and categorize them
    pub async fn analyze(&self, comments: &[YouTubeComment], video_title: &str) -> AnalysisResult {
        // If we have Gemini API key, use it
        if let Some(key) = &self.gemini_key {
            if let Ok(result) = self.analyze_with_gemini(comments, video_title, key).await {
                return result;
            }
        }
        
        // Fallback to local heuristic analysis
        self.analyze_local(comments, video_title)
    }
    
    /// Local heuristic-based analysis
    fn analyze_local(&self, comments: &[YouTubeComment], _video_title: &str) -> AnalysisResult {
        let mut core_dialogue = Vec::new();
        let mut questions = Vec::new();
        let mut signal_noise = Vec::new();
        let mut anomalies = Vec::new();
        let mut topics: HashMap<String, usize> = HashMap::new();
        
        for comment in comments {
            let text = &comment.text;
            
            // Questions
            if text.contains('?') && text.len() > 20 {
                questions.push(comment.clone());
                continue;
            }
            
            // Anomalies (controversial - high engagement relative to likes)
            if comment.reply_count > 10 && comment.like_count < comment.reply_count * 2 {
                anomalies.push(comment.clone());
                continue;
            }
            
            // Core dialogue (substantive, high engagement)
            if text.len() > 50 && comment.like_count > 5 {
                core_dialogue.push(comment.clone());
            } else {
                signal_noise.push(comment.clone());
            }
            
            // Extract topics (simple word frequency)
            for word in text.split_whitespace() {
                let word_clean = word.to_lowercase()
                    .chars()
                    .filter(|c| c.is_alphabetic())
                    .collect::<String>();
                
                if word_clean.len() > 4 {
                    *topics.entry(word_clean).or_insert(0) += 1;
                }
            }
        }
        
        // Sort core dialogue by likes
        core_dialogue.sort_by(|a, b| b.like_count.cmp(&a.like_count));
        questions.sort_by(|a, b| b.like_count.cmp(&a.like_count));
        
        // Get top topics
        let mut topic_vec: Vec<_> = topics.into_iter().collect();
        topic_vec.sort_by(|a, b| b.1.cmp(&a.1));
        let top_topics: Vec<String> = topic_vec.into_iter()
            .take(10)
            .map(|(word, _)| word)
            .collect();
        
        AnalysisResult {
            discussions: vec![],
            core_dialogue,
            questions,
            signal_noise,
            anomalies,
            topics: top_topics,
        }
    }
    
    /// Analyze using Gemini API
    async fn analyze_with_gemini(
        &self,
        comments: &[YouTubeComment],
        video_title: &str,
        api_key: &str,
    ) -> Result<AnalysisResult, String> {
        // Prepare comment text (limit to avoid token limits)
        let comment_texts: Vec<String> = comments.iter()
            .take(500)
            .map(|c| format!("[{}] {}: {}", c.like_count, c.author, c.text))
            .collect();
        
        let prompt = format!(
            r#"Analyze these YouTube comments for the video "{}" and categorize them.

Comments:
{}

Respond in JSON format:
{{
  "discussions": [
    {{"id": "1", "title": "Topic Name", "description": "Brief description", "comment_indices": [0, 3, 7]}}
  ],
  "core_dialogue_indices": [1, 4, 8],
  "question_indices": [2, 5],
  "anomaly_indices": [6],
  "topics": ["topic1", "topic2"]
}}

Only include indices of comments that fit each category. Core dialogue = substantive discussion. Anomalies = controversial/outlier views."#,
            video_title,
            comment_texts.join("\n")
        );
        
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-pro:generateContent?key={}",
            api_key
        );
        
        let body = serde_json::json!({
            "contents": [{
                "parts": [{"text": prompt}]
            }],
            "generationConfig": {
                "temperature": 0.3,
                "maxOutputTokens": 4096
            }
        });
        
        let response = self.client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("Gemini request failed: {}", e))?;
        
        if !response.status().is_success() {
            return Err("Gemini API error".to_string());
        }
        
        #[derive(Deserialize)]
        struct GeminiResponse {
            candidates: Vec<Candidate>,
        }
        
        #[derive(Deserialize)]
        struct Candidate {
            content: Content,
        }
        
        #[derive(Deserialize)]
        struct Content {
            parts: Vec<Part>,
        }
        
        #[derive(Deserialize)]
        struct Part {
            text: String,
        }
        
        let gemini_response: GeminiResponse = response.json().await
            .map_err(|e| format!("Failed to parse Gemini response: {}", e))?;
        
        let text = gemini_response.candidates
            .first()
            .and_then(|c| c.content.parts.first())
            .map(|p| p.text.clone())
            .ok_or_else(|| "Empty Gemini response".to_string())?;
        
        // Extract JSON from response
        let json_start = text.find('{').ok_or("No JSON in response")?;
        let json_end = text.rfind('}').ok_or("No JSON end in response")? + 1;
        let json_str = &text[json_start..json_end];
        
        #[derive(Deserialize)]
        struct GeminiAnalysis {
            discussions: Vec<GeminiDiscussion>,
            core_dialogue_indices: Vec<usize>,
            question_indices: Vec<usize>,
            anomaly_indices: Vec<usize>,
            topics: Vec<String>,
        }
        
        #[derive(Deserialize)]
        struct GeminiDiscussion {
            id: String,
            title: String,
            description: String,
            comment_indices: Vec<usize>,
        }
        
        let analysis: GeminiAnalysis = serde_json::from_str(json_str)
            .map_err(|e| format!("Failed to parse analysis: {}", e))?;
        
        // Map indices back to comments
        let get_comments = |indices: &[usize]| -> Vec<YouTubeComment> {
            indices.iter()
                .filter_map(|&i| comments.get(i).cloned())
                .collect()
        };
        
        let discussions: Vec<Discussion> = analysis.discussions.into_iter()
            .map(|d| Discussion {
                id: d.id,
                title: d.title,
                description: d.description,
                comment_ids: d.comment_indices.iter()
                    .filter_map(|&i| comments.get(i).map(|c| c.id.clone()))
                    .collect(),
                total_comments: d.comment_indices.len(),
            })
            .collect();
        
        // Collect all categorized indices
        let categorized: HashSet<usize> = analysis.core_dialogue_indices.iter()
            .chain(&analysis.question_indices)
            .chain(&analysis.anomaly_indices)
            .copied()
            .collect();
        
        // Signal noise = everything not categorized
        let signal_noise: Vec<YouTubeComment> = comments.iter()
            .enumerate()
            .filter(|(i, _)| !categorized.contains(i))
            .map(|(_, c)| c.clone())
            .collect();
        
        Ok(AnalysisResult {
            discussions,
            core_dialogue: get_comments(&analysis.core_dialogue_indices),
            questions: get_comments(&analysis.question_indices),
            signal_noise,
            anomalies: get_comments(&analysis.anomaly_indices),
            topics: analysis.topics,
        })
    }
}

#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub discussions: Vec<Discussion>,
    pub core_dialogue: Vec<YouTubeComment>,
    pub questions: Vec<YouTubeComment>,
    pub signal_noise: Vec<YouTubeComment>,
    pub anomalies: Vec<YouTubeComment>,
    pub topics: Vec<String>,
}

// ============================================================================
// FORUMYZE MAIN API
// ============================================================================

pub struct Forumyze {
    youtube: YouTubeClient,
    filter: CommentFilter,
    analyzer: CommentAnalyzer,
}

impl Forumyze {
    /// Create new Forumyze instance with user's API keys
    pub fn new(keys: UserApiKeys) -> Self {
        Self {
            youtube: YouTubeClient::new(keys.youtube_api_key),
            filter: CommentFilter::new(),
            analyzer: CommentAnalyzer::new(keys.gemini_api_key),
        }
    }
    
    /// Analyze a YouTube video's comments
    pub async fn analyze_video(&self, video_id_or_url: &str, max_comments: usize) -> Result<ForumyzeResult, String> {
        // Extract video ID
        let video_id = YouTubeClient::extract_video_id(video_id_or_url)
            .ok_or_else(|| "Invalid YouTube URL or video ID".to_string())?;
        
        // Fetch metadata
        let metadata = self.youtube.get_video_metadata(&video_id).await?;
        
        // Fetch comments
        let raw_comments = self.youtube.fetch_comments(&video_id, max_comments).await?;
        let total = raw_comments.len();
        
        if raw_comments.is_empty() {
            return Err("No comments found".to_string());
        }
        
        // Filter
        let (filtered_comments, stats) = self.filter.filter(raw_comments);
        let filtered_count = filtered_comments.len();
        
        // Analyze
        let analysis = self.analyzer.analyze(&filtered_comments, &metadata.title).await;
        
        Ok(ForumyzeResult {
            video: metadata,
            total_comments: total,
            filtered_comments: filtered_count,
            discussions: analysis.discussions,
            core_dialogue: analysis.core_dialogue,
            questions: analysis.questions,
            signal_noise: analysis.signal_noise,
            anomalies: analysis.anomalies,
            topics: analysis.topics,
            stats,
        })
    }
    
    /// Quick preview with limited comments
    pub async fn preview(&self, video_id_or_url: &str) -> Result<ForumyzeResult, String> {
        self.analyze_video(video_id_or_url, 500).await
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_extract_video_id() {
        assert_eq!(
            YouTubeClient::extract_video_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            YouTubeClient::extract_video_id("https://youtu.be/dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
        assert_eq!(
            YouTubeClient::extract_video_id("https://youtube.com/shorts/abc123xyz99"),
            Some("abc123xyz99".to_string())
        );
        assert_eq!(
            YouTubeClient::extract_video_id("dQw4w9WgXcQ"),
            Some("dQw4w9WgXcQ".to_string())
        );
    }
    
    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_iso_duration("PT1H2M3S"), 3723);
        assert_eq!(parse_iso_duration("PT10M"), 600);
        assert_eq!(parse_iso_duration("PT45S"), 45);
    }
    
    #[test]
    fn test_filter() {
        let filter = CommentFilter::new();
        let comments = vec![
            YouTubeComment {
                id: "1".to_string(),
                author: "User".to_string(),
                author_image: String::new(),
                author_channel: String::new(),
                text: "This is a great video about programming!".to_string(),
                published_at: String::new(),
                like_count: 10,
                reply_count: 0,
                replies: vec![],
                source_video_id: None,
                source_type: None,
            },
            YouTubeComment {
                id: "2".to_string(),
                author: "Spammer".to_string(),
                author_image: String::new(),
                author_channel: String::new(),
                text: "Sub to me please check out my channel".to_string(),
                published_at: String::new(),
                like_count: 0,
                reply_count: 0,
                replies: vec![],
                source_video_id: None,
                source_type: None,
            },
            YouTubeComment {
                id: "3".to_string(),
                author: "Short".to_string(),
                author_image: String::new(),
                author_channel: String::new(),
                text: "Nice!".to_string(),
                published_at: String::new(),
                like_count: 5,
                reply_count: 0,
                replies: vec![],
                source_video_id: None,
                source_type: None,
            },
        ];
        
        let (kept, stats) = filter.filter(comments);
        assert_eq!(kept.len(), 1);
        assert_eq!(stats.spam_phrases, 1);
        assert_eq!(stats.too_short, 1);
    }
}
