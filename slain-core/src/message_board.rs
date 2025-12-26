// SLAIN MESSAGE BOARD - Persistent Discussion Forum
// Local SQLite storage for video-related discussions
// Syncs with central server when online (optional)

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use parking_lot::RwLock;
use chrono::{DateTime, Utc};

// ============================================================================
// DATA TYPES
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Board {
    pub id: String,
    pub video_id: String,
    pub video_title: String,
    pub created_at: String,
    pub thread_count: u32,
    pub post_count: u32,
    pub last_activity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub board_id: String,
    pub title: String,
    pub author: Author,
    pub created_at: String,
    pub updated_at: String,
    pub post_count: u32,
    pub view_count: u32,
    pub pinned: bool,
    pub locked: bool,
    pub tags: Vec<String>,
    /// First post content (preview)
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Post {
    pub id: String,
    pub thread_id: String,
    pub author: Author,
    pub content: String,
    pub created_at: String,
    pub edited_at: Option<String>,
    pub reply_to: Option<String>,
    pub reactions: Reactions,
    /// Referenced YouTube comment ID (if from FORUMYZE)
    pub source_comment_id: Option<String>,
    /// Attachments (images, evidence links)
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    pub id: String,
    pub display_name: String,
    pub avatar_url: Option<String>,
    /// Google account ID if authenticated
    pub google_id: Option<String>,
    /// Is this a verified account?
    pub verified: bool,
    /// Role (user, moderator, admin)
    pub role: UserRole,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum UserRole {
    Guest,
    User,
    Moderator,
    Admin,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Reactions {
    pub upvotes: u32,
    pub downvotes: u32,
    pub hearts: u32,
    pub eyes: u32,  // "witnessed" reaction
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    pub url: String,
    pub mime_type: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub id: String,
    pub google_id: Option<String>,
    pub display_name: String,
    pub email: Option<String>,
    pub avatar_url: Option<String>,
    pub created_at: String,
    pub post_count: u32,
    pub reputation: i32,
    pub role: UserRole,
    pub preferences: UserPreferences,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserPreferences {
    pub theme: String,
    pub notifications_enabled: bool,
    pub email_notifications: bool,
    pub show_timestamps: bool,
}

// ============================================================================
// DATABASE SCHEMA (SQLite)
// ============================================================================

const SCHEMA: &str = r#"
-- Boards (one per video)
CREATE TABLE IF NOT EXISTS boards (
    id TEXT PRIMARY KEY,
    video_id TEXT UNIQUE NOT NULL,
    video_title TEXT NOT NULL,
    created_at TEXT NOT NULL,
    thread_count INTEGER DEFAULT 0,
    post_count INTEGER DEFAULT 0,
    last_activity TEXT
);

CREATE INDEX IF NOT EXISTS idx_boards_video ON boards(video_id);

-- Users
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    google_id TEXT UNIQUE,
    display_name TEXT NOT NULL,
    email TEXT,
    avatar_url TEXT,
    created_at TEXT NOT NULL,
    post_count INTEGER DEFAULT 0,
    reputation INTEGER DEFAULT 0,
    role TEXT DEFAULT 'User',
    preferences TEXT DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_users_google ON users(google_id);

-- Threads
CREATE TABLE IF NOT EXISTS threads (
    id TEXT PRIMARY KEY,
    board_id TEXT NOT NULL REFERENCES boards(id),
    title TEXT NOT NULL,
    author_id TEXT NOT NULL REFERENCES users(id),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    post_count INTEGER DEFAULT 1,
    view_count INTEGER DEFAULT 0,
    pinned INTEGER DEFAULT 0,
    locked INTEGER DEFAULT 0,
    tags TEXT DEFAULT '[]'
);

CREATE INDEX IF NOT EXISTS idx_threads_board ON threads(board_id);
CREATE INDEX IF NOT EXISTS idx_threads_author ON threads(author_id);
CREATE INDEX IF NOT EXISTS idx_threads_updated ON threads(updated_at DESC);

-- Posts
CREATE TABLE IF NOT EXISTS posts (
    id TEXT PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id),
    author_id TEXT NOT NULL REFERENCES users(id),
    content TEXT NOT NULL,
    created_at TEXT NOT NULL,
    edited_at TEXT,
    reply_to TEXT,
    upvotes INTEGER DEFAULT 0,
    downvotes INTEGER DEFAULT 0,
    hearts INTEGER DEFAULT 0,
    eyes INTEGER DEFAULT 0,
    source_comment_id TEXT,
    attachments TEXT DEFAULT '[]'
);

CREATE INDEX IF NOT EXISTS idx_posts_thread ON posts(thread_id);
CREATE INDEX IF NOT EXISTS idx_posts_author ON posts(author_id);
CREATE INDEX IF NOT EXISTS idx_posts_source ON posts(source_comment_id);

-- Reactions (track who reacted)
CREATE TABLE IF NOT EXISTS reactions (
    id TEXT PRIMARY KEY,
    post_id TEXT NOT NULL REFERENCES posts(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    reaction_type TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(post_id, user_id, reaction_type)
);

CREATE INDEX IF NOT EXISTS idx_reactions_post ON reactions(post_id);

-- Sync queue (for offline changes)
CREATE TABLE IF NOT EXISTS sync_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    action TEXT NOT NULL,
    table_name TEXT NOT NULL,
    record_id TEXT NOT NULL,
    data TEXT NOT NULL,
    created_at TEXT NOT NULL,
    synced INTEGER DEFAULT 0
);
"#;

// ============================================================================
// MESSAGE BOARD SERVICE
// ============================================================================

pub struct MessageBoard {
    db_path: PathBuf,
    conn: Arc<RwLock<rusqlite::Connection>>,
    current_user: Arc<RwLock<Option<UserProfile>>>,
}

impl MessageBoard {
    /// Open or create message board database
    pub fn open(db_path: PathBuf) -> Result<Self, String> {
        let conn = rusqlite::Connection::open(&db_path)
            .map_err(|e| format!("Failed to open database: {}", e))?;
        
        // Initialize schema
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("Failed to create schema: {}", e))?;
        
        // Enable foreign keys
        conn.execute("PRAGMA foreign_keys = ON", [])
            .map_err(|e| format!("Failed to enable foreign keys: {}", e))?;
        
        Ok(Self {
            db_path,
            conn: Arc::new(RwLock::new(conn)),
            current_user: Arc::new(RwLock::new(None)),
        })
    }
    
    /// Get default database path
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("SLAIN")
            .join("messageboard.db")
    }
    
    // ========================================================================
    // USER MANAGEMENT
    // ========================================================================
    
    /// Login with Google OAuth
    pub fn login_google(&self, google_id: &str, name: &str, email: &str, avatar: Option<&str>) -> Result<UserProfile, String> {
        let conn = self.conn.read();
        
        // Check if user exists
        let existing: Option<String> = conn.query_row(
            "SELECT id FROM users WHERE google_id = ?",
            [google_id],
            |row| row.get(0)
        ).ok();
        
        let user_id = if let Some(id) = existing {
            // Update existing user
            conn.execute(
                "UPDATE users SET display_name = ?, email = ?, avatar_url = ? WHERE id = ?",
                rusqlite::params![name, email, avatar, id]
            ).map_err(|e| format!("Failed to update user: {}", e))?;
            id
        } else {
            // Create new user
            let id = uuid::Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO users (id, google_id, display_name, email, avatar_url, created_at, role) VALUES (?, ?, ?, ?, ?, ?, ?)",
                rusqlite::params![id, google_id, name, email, avatar, Utc::now().to_rfc3339(), "User"]
            ).map_err(|e| format!("Failed to create user: {}", e))?;
            id
        };
        
        drop(conn);
        
        let profile = self.get_user(&user_id)?;
        *self.current_user.write() = Some(profile.clone());
        
        Ok(profile)
    }
    
    /// Login as guest
    pub fn login_guest(&self, name: &str) -> Result<UserProfile, String> {
        let id = uuid::Uuid::new_v4().to_string();
        let conn = self.conn.read();
        
        conn.execute(
            "INSERT INTO users (id, display_name, created_at, role) VALUES (?, ?, ?, ?)",
            rusqlite::params![id, name, Utc::now().to_rfc3339(), "Guest"]
        ).map_err(|e| format!("Failed to create guest: {}", e))?;
        
        drop(conn);
        
        let profile = self.get_user(&id)?;
        *self.current_user.write() = Some(profile.clone());
        
        Ok(profile)
    }
    
    /// Get current logged in user
    pub fn current_user(&self) -> Option<UserProfile> {
        self.current_user.read().clone()
    }
    
    /// Get user by ID
    pub fn get_user(&self, user_id: &str) -> Result<UserProfile, String> {
        let conn = self.conn.read();
        
        conn.query_row(
            "SELECT id, google_id, display_name, email, avatar_url, created_at, post_count, reputation, role, preferences FROM users WHERE id = ?",
            [user_id],
            |row| {
                let role_str: String = row.get(8)?;
                let prefs_str: String = row.get(9)?;
                
                Ok(UserProfile {
                    id: row.get(0)?,
                    google_id: row.get(1)?,
                    display_name: row.get(2)?,
                    email: row.get(3)?,
                    avatar_url: row.get(4)?,
                    created_at: row.get(5)?,
                    post_count: row.get(6)?,
                    reputation: row.get(7)?,
                    role: match role_str.as_str() {
                        "Guest" => UserRole::Guest,
                        "Moderator" => UserRole::Moderator,
                        "Admin" => UserRole::Admin,
                        _ => UserRole::User,
                    },
                    preferences: serde_json::from_str(&prefs_str).unwrap_or_default(),
                })
            }
        ).map_err(|e| format!("User not found: {}", e))
    }
    
    // ========================================================================
    // BOARDS
    // ========================================================================
    
    /// Get or create board for a video
    pub fn get_or_create_board(&self, video_id: &str, video_title: &str) -> Result<Board, String> {
        let conn = self.conn.read();
        
        // Try to get existing
        let existing = conn.query_row(
            "SELECT id, video_id, video_title, created_at, thread_count, post_count, last_activity FROM boards WHERE video_id = ?",
            [video_id],
            |row| Ok(Board {
                id: row.get(0)?,
                video_id: row.get(1)?,
                video_title: row.get(2)?,
                created_at: row.get(3)?,
                thread_count: row.get(4)?,
                post_count: row.get(5)?,
                last_activity: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        );
        
        if let Ok(board) = existing {
            return Ok(board);
        }
        
        // Create new board
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        
        conn.execute(
            "INSERT INTO boards (id, video_id, video_title, created_at, last_activity) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![id, video_id, video_title, now, now]
        ).map_err(|e| format!("Failed to create board: {}", e))?;
        
        Ok(Board {
            id,
            video_id: video_id.to_string(),
            video_title: video_title.to_string(),
            created_at: now.clone(),
            thread_count: 0,
            post_count: 0,
            last_activity: now,
        })
    }
    
    /// List all boards
    pub fn list_boards(&self, limit: usize, offset: usize) -> Result<Vec<Board>, String> {
        let conn = self.conn.read();
        
        let mut stmt = conn.prepare(
            "SELECT id, video_id, video_title, created_at, thread_count, post_count, last_activity 
             FROM boards ORDER BY last_activity DESC LIMIT ? OFFSET ?"
        ).map_err(|e| format!("Query failed: {}", e))?;
        
        let boards = stmt.query_map([limit as i64, offset as i64], |row| {
            Ok(Board {
                id: row.get(0)?,
                video_id: row.get(1)?,
                video_title: row.get(2)?,
                created_at: row.get(3)?,
                thread_count: row.get(4)?,
                post_count: row.get(5)?,
                last_activity: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        }).map_err(|e| format!("Query failed: {}", e))?;
        
        boards.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect boards: {}", e))
    }
    
    // ========================================================================
    // THREADS
    // ========================================================================
    
    /// Create a new thread
    pub fn create_thread(&self, board_id: &str, title: &str, content: &str, tags: Vec<String>) -> Result<Thread, String> {
        let user = self.current_user()
            .ok_or_else(|| "Must be logged in to create thread".to_string())?;
        
        let conn = self.conn.read();
        
        let thread_id = uuid::Uuid::new_v4().to_string();
        let post_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        let tags_json = serde_json::to_string(&tags).unwrap_or_default();
        
        // Create thread
        conn.execute(
            "INSERT INTO threads (id, board_id, title, author_id, created_at, updated_at, tags) VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![thread_id, board_id, title, user.id, now, now, tags_json]
        ).map_err(|e| format!("Failed to create thread: {}", e))?;
        
        // Create first post
        conn.execute(
            "INSERT INTO posts (id, thread_id, author_id, content, created_at) VALUES (?, ?, ?, ?, ?)",
            rusqlite::params![post_id, thread_id, user.id, content, now]
        ).map_err(|e| format!("Failed to create post: {}", e))?;
        
        // Update board counters
        conn.execute(
            "UPDATE boards SET thread_count = thread_count + 1, post_count = post_count + 1, last_activity = ? WHERE id = ?",
            rusqlite::params![now, board_id]
        ).ok();
        
        // Update user post count
        conn.execute(
            "UPDATE users SET post_count = post_count + 1 WHERE id = ?",
            [&user.id]
        ).ok();
        
        Ok(Thread {
            id: thread_id,
            board_id: board_id.to_string(),
            title: title.to_string(),
            author: Author {
                id: user.id,
                display_name: user.display_name,
                avatar_url: user.avatar_url,
                verified: user.google_id.is_some(),
                google_id: user.google_id,
                role: user.role,
            },
            created_at: now.clone(),
            updated_at: now,
            post_count: 1,
            view_count: 0,
            pinned: false,
            locked: false,
            tags,
            preview: content.chars().take(200).collect(),
        })
    }
    
    /// List threads in a board
    pub fn list_threads(&self, board_id: &str, limit: usize, offset: usize) -> Result<Vec<Thread>, String> {
        let conn = self.conn.read();
        
        let mut stmt = conn.prepare(
            "SELECT t.id, t.board_id, t.title, t.author_id, t.created_at, t.updated_at, 
                    t.post_count, t.view_count, t.pinned, t.locked, t.tags,
                    u.display_name, u.avatar_url, u.google_id, u.role,
                    (SELECT content FROM posts WHERE thread_id = t.id ORDER BY created_at LIMIT 1) as preview
             FROM threads t
             JOIN users u ON t.author_id = u.id
             WHERE t.board_id = ?
             ORDER BY t.pinned DESC, t.updated_at DESC
             LIMIT ? OFFSET ?"
        ).map_err(|e| format!("Query failed: {}", e))?;
        
        let threads = stmt.query_map(rusqlite::params![board_id, limit as i64, offset as i64], |row| {
            let tags_str: String = row.get(10)?;
            let role_str: String = row.get(14)?;
            
            Ok(Thread {
                id: row.get(0)?,
                board_id: row.get(1)?,
                title: row.get(2)?,
                author: Author {
                    id: row.get(3)?,
                    display_name: row.get(11)?,
                    avatar_url: row.get(12)?,
                    google_id: row.get(13)?,
                    verified: row.get::<_, Option<String>>(13)?.is_some(),
                    role: match role_str.as_str() {
                        "Guest" => UserRole::Guest,
                        "Moderator" => UserRole::Moderator,
                        "Admin" => UserRole::Admin,
                        _ => UserRole::User,
                    },
                },
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                post_count: row.get(6)?,
                view_count: row.get(7)?,
                pinned: row.get::<_, i32>(8)? != 0,
                locked: row.get::<_, i32>(9)? != 0,
                tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                preview: row.get::<_, Option<String>>(15)?.unwrap_or_default().chars().take(200).collect(),
            })
        }).map_err(|e| format!("Query failed: {}", e))?;
        
        threads.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect threads: {}", e))
    }
    
    /// Get thread by ID
    pub fn get_thread(&self, thread_id: &str) -> Result<Thread, String> {
        let conn = self.conn.read();
        
        // Increment view count
        conn.execute(
            "UPDATE threads SET view_count = view_count + 1 WHERE id = ?",
            [thread_id]
        ).ok();
        
        conn.query_row(
            "SELECT t.id, t.board_id, t.title, t.author_id, t.created_at, t.updated_at, 
                    t.post_count, t.view_count, t.pinned, t.locked, t.tags,
                    u.display_name, u.avatar_url, u.google_id, u.role
             FROM threads t
             JOIN users u ON t.author_id = u.id
             WHERE t.id = ?",
            [thread_id],
            |row| {
                let tags_str: String = row.get(10)?;
                let role_str: String = row.get(14)?;
                
                Ok(Thread {
                    id: row.get(0)?,
                    board_id: row.get(1)?,
                    title: row.get(2)?,
                    author: Author {
                        id: row.get(3)?,
                        display_name: row.get(11)?,
                        avatar_url: row.get(12)?,
                        google_id: row.get(13)?,
                        verified: row.get::<_, Option<String>>(13)?.is_some(),
                        role: match role_str.as_str() {
                            "Guest" => UserRole::Guest,
                            "Moderator" => UserRole::Moderator,
                            "Admin" => UserRole::Admin,
                            _ => UserRole::User,
                        },
                    },
                    created_at: row.get(4)?,
                    updated_at: row.get(5)?,
                    post_count: row.get(6)?,
                    view_count: row.get(7)?,
                    pinned: row.get::<_, i32>(8)? != 0,
                    locked: row.get::<_, i32>(9)? != 0,
                    tags: serde_json::from_str(&tags_str).unwrap_or_default(),
                    preview: String::new(),
                })
            }
        ).map_err(|e| format!("Thread not found: {}", e))
    }
    
    // ========================================================================
    // POSTS
    // ========================================================================
    
    /// Create a reply post
    pub fn create_post(&self, thread_id: &str, content: &str, reply_to: Option<&str>, source_comment_id: Option<&str>) -> Result<Post, String> {
        let user = self.current_user()
            .ok_or_else(|| "Must be logged in to post".to_string())?;
        
        // Check if thread is locked
        let thread = self.get_thread(thread_id)?;
        if thread.locked {
            return Err("Thread is locked".to_string());
        }
        
        let conn = self.conn.read();
        
        let post_id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();
        
        conn.execute(
            "INSERT INTO posts (id, thread_id, author_id, content, created_at, reply_to, source_comment_id) VALUES (?, ?, ?, ?, ?, ?, ?)",
            rusqlite::params![post_id, thread_id, user.id, content, now, reply_to, source_comment_id]
        ).map_err(|e| format!("Failed to create post: {}", e))?;
        
        // Update thread
        conn.execute(
            "UPDATE threads SET post_count = post_count + 1, updated_at = ? WHERE id = ?",
            rusqlite::params![now, thread_id]
        ).ok();
        
        // Update board
        conn.execute(
            "UPDATE boards SET post_count = post_count + 1, last_activity = ? WHERE id = ?",
            rusqlite::params![now, thread.board_id]
        ).ok();
        
        // Update user post count
        conn.execute(
            "UPDATE users SET post_count = post_count + 1 WHERE id = ?",
            [&user.id]
        ).ok();
        
        Ok(Post {
            id: post_id,
            thread_id: thread_id.to_string(),
            author: Author {
                id: user.id,
                display_name: user.display_name,
                avatar_url: user.avatar_url,
                verified: user.google_id.is_some(),
                google_id: user.google_id,
                role: user.role,
            },
            content: content.to_string(),
            created_at: now,
            edited_at: None,
            reply_to: reply_to.map(|s| s.to_string()),
            reactions: Reactions::default(),
            source_comment_id: source_comment_id.map(|s| s.to_string()),
            attachments: vec![],
        })
    }
    
    /// List posts in a thread
    pub fn list_posts(&self, thread_id: &str, limit: usize, offset: usize) -> Result<Vec<Post>, String> {
        let conn = self.conn.read();
        
        let mut stmt = conn.prepare(
            "SELECT p.id, p.thread_id, p.author_id, p.content, p.created_at, p.edited_at,
                    p.reply_to, p.upvotes, p.downvotes, p.hearts, p.eyes, 
                    p.source_comment_id, p.attachments,
                    u.display_name, u.avatar_url, u.google_id, u.role
             FROM posts p
             JOIN users u ON p.author_id = u.id
             WHERE p.thread_id = ?
             ORDER BY p.created_at ASC
             LIMIT ? OFFSET ?"
        ).map_err(|e| format!("Query failed: {}", e))?;
        
        let posts = stmt.query_map(rusqlite::params![thread_id, limit as i64, offset as i64], |row| {
            let attachments_str: String = row.get(12)?;
            let role_str: String = row.get(16)?;
            
            Ok(Post {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                author: Author {
                    id: row.get(2)?,
                    display_name: row.get(13)?,
                    avatar_url: row.get(14)?,
                    google_id: row.get(15)?,
                    verified: row.get::<_, Option<String>>(15)?.is_some(),
                    role: match role_str.as_str() {
                        "Guest" => UserRole::Guest,
                        "Moderator" => UserRole::Moderator,
                        "Admin" => UserRole::Admin,
                        _ => UserRole::User,
                    },
                },
                content: row.get(3)?,
                created_at: row.get(4)?,
                edited_at: row.get(5)?,
                reply_to: row.get(6)?,
                reactions: Reactions {
                    upvotes: row.get(7)?,
                    downvotes: row.get(8)?,
                    hearts: row.get(9)?,
                    eyes: row.get(10)?,
                },
                source_comment_id: row.get(11)?,
                attachments: serde_json::from_str(&attachments_str).unwrap_or_default(),
            })
        }).map_err(|e| format!("Query failed: {}", e))?;
        
        posts.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect posts: {}", e))
    }
    
    /// React to a post
    pub fn react(&self, post_id: &str, reaction: &str) -> Result<(), String> {
        let user = self.current_user()
            .ok_or_else(|| "Must be logged in to react".to_string())?;
        
        let conn = self.conn.read();
        
        // Check if already reacted
        let existing: Option<String> = conn.query_row(
            "SELECT id FROM reactions WHERE post_id = ? AND user_id = ? AND reaction_type = ?",
            rusqlite::params![post_id, user.id, reaction],
            |row| row.get(0)
        ).ok();
        
        if existing.is_some() {
            // Remove reaction
            conn.execute(
                "DELETE FROM reactions WHERE post_id = ? AND user_id = ? AND reaction_type = ?",
                rusqlite::params![post_id, user.id, reaction]
            ).ok();
            
            let column = match reaction {
                "upvote" => "upvotes",
                "downvote" => "downvotes",
                "heart" => "hearts",
                "eyes" => "eyes",
                _ => return Err("Invalid reaction".to_string()),
            };
            
            conn.execute(
                &format!("UPDATE posts SET {} = {} - 1 WHERE id = ?", column, column),
                [post_id]
            ).ok();
        } else {
            // Add reaction
            let reaction_id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            
            conn.execute(
                "INSERT INTO reactions (id, post_id, user_id, reaction_type, created_at) VALUES (?, ?, ?, ?, ?)",
                rusqlite::params![reaction_id, post_id, user.id, reaction, now]
            ).map_err(|e| format!("Failed to add reaction: {}", e))?;
            
            let column = match reaction {
                "upvote" => "upvotes",
                "downvote" => "downvotes",
                "heart" => "hearts",
                "eyes" => "eyes",
                _ => return Err("Invalid reaction".to_string()),
            };
            
            conn.execute(
                &format!("UPDATE posts SET {} = {} + 1 WHERE id = ?", column, column),
                [post_id]
            ).ok();
        }
        
        Ok(())
    }
    
    // ========================================================================
    // IMPORT FROM FORUMYZE
    // ========================================================================
    
    /// Import FORUMYZE comments as thread starter posts
    pub fn import_forumyze_discussion(
        &self, 
        board_id: &str, 
        discussion_title: &str,
        comments: &[crate::forumyze::YouTubeComment]
    ) -> Result<Thread, String> {
        // Create thread with first comment
        if comments.is_empty() {
            return Err("No comments to import".to_string());
        }
        
        let first = &comments[0];
        let thread = self.create_thread(
            board_id,
            discussion_title,
            &format!("**{}** said:\n\n> {}\n\n---\n*Imported from YouTube comments*", 
                first.author, first.text),
            vec!["imported".to_string(), "youtube".to_string()]
        )?;
        
        // Import remaining comments as posts
        for comment in comments.iter().skip(1).take(50) {
            self.create_post(
                &thread.id,
                &format!("**{}** said:\n\n> {}", comment.author, comment.text),
                None,
                Some(&comment.id)
            ).ok();
        }
        
        Ok(thread)
    }
    
    // ========================================================================
    // SEARCH
    // ========================================================================
    
    /// Search posts
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Post>, String> {
        let conn = self.conn.read();
        let search_term = format!("%{}%", query);
        
        let mut stmt = conn.prepare(
            "SELECT p.id, p.thread_id, p.author_id, p.content, p.created_at, p.edited_at,
                    p.reply_to, p.upvotes, p.downvotes, p.hearts, p.eyes, 
                    p.source_comment_id, p.attachments,
                    u.display_name, u.avatar_url, u.google_id, u.role
             FROM posts p
             JOIN users u ON p.author_id = u.id
             WHERE p.content LIKE ?
             ORDER BY p.created_at DESC
             LIMIT ?"
        ).map_err(|e| format!("Query failed: {}", e))?;
        
        let posts = stmt.query_map(rusqlite::params![search_term, limit as i64], |row| {
            let attachments_str: String = row.get(12)?;
            let role_str: String = row.get(16)?;
            
            Ok(Post {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                author: Author {
                    id: row.get(2)?,
                    display_name: row.get(13)?,
                    avatar_url: row.get(14)?,
                    google_id: row.get(15)?,
                    verified: row.get::<_, Option<String>>(15)?.is_some(),
                    role: match role_str.as_str() {
                        "Guest" => UserRole::Guest,
                        "Moderator" => UserRole::Moderator,
                        "Admin" => UserRole::Admin,
                        _ => UserRole::User,
                    },
                },
                content: row.get(3)?,
                created_at: row.get(4)?,
                edited_at: row.get(5)?,
                reply_to: row.get(6)?,
                reactions: Reactions {
                    upvotes: row.get(7)?,
                    downvotes: row.get(8)?,
                    hearts: row.get(9)?,
                    eyes: row.get(10)?,
                },
                source_comment_id: row.get(11)?,
                attachments: serde_json::from_str(&attachments_str).unwrap_or_default(),
            })
        }).map_err(|e| format!("Query failed: {}", e))?;
        
        posts.collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("Failed to collect posts: {}", e))
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    
    #[test]
    fn test_create_board() {
        let temp = NamedTempFile::new().unwrap();
        let board = MessageBoard::open(temp.path().to_path_buf()).unwrap();
        
        // Login as guest
        board.login_guest("TestUser").unwrap();
        
        // Create board
        let b = board.get_or_create_board("dQw4w9WgXcQ", "Test Video").unwrap();
        assert_eq!(b.video_id, "dQw4w9WgXcQ");
        
        // Get same board again
        let b2 = board.get_or_create_board("dQw4w9WgXcQ", "Test Video").unwrap();
        assert_eq!(b.id, b2.id);
    }
    
    #[test]
    fn test_create_thread_and_posts() {
        let temp = NamedTempFile::new().unwrap();
        let board = MessageBoard::open(temp.path().to_path_buf()).unwrap();
        
        board.login_guest("TestUser").unwrap();
        let b = board.get_or_create_board("test123", "Test").unwrap();
        
        // Create thread
        let thread = board.create_thread(&b.id, "Test Thread", "Hello world!", vec![]).unwrap();
        assert_eq!(thread.title, "Test Thread");
        assert_eq!(thread.post_count, 1);
        
        // Create reply
        let post = board.create_post(&thread.id, "This is a reply", None, None).unwrap();
        assert_eq!(post.thread_id, thread.id);
        
        // List posts
        let posts = board.list_posts(&thread.id, 10, 0).unwrap();
        assert_eq!(posts.len(), 2);
    }
}
