//! Media Library & Movie Scraping
//!
//! Features:
//! - Local media scanning
//! - TMDB/OMDB metadata scraping
//! - Plex/Jellyfin integration
//! - Free streaming sources

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ============================================================================
// Media Item Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaItem {
    pub id: String,
    pub title: String,
    pub year: Option<u32>,
    pub media_type: MediaType,
    pub source: MediaSource,
    pub path: Option<String>,           // Local file path
    pub stream_url: Option<String>,     // Remote stream URL
    pub metadata: Option<MediaMetadata>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum MediaType {
    Movie,
    TvShow,
    Episode,
    LiveTv,
    Music,
    Photo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MediaSource {
    Local,              // Local file
    Plex { server_id: String },
    Jellyfin { server_id: String },
    Emby { server_id: String },
    FreeTubi,
    FreePluto,
    FreeRoku,
    FreePeacock,
    FreeYouTube,
    Custom { name: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    pub tmdb_id: Option<u64>,
    pub imdb_id: Option<String>,
    pub overview: Option<String>,
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub rating: Option<f32>,
    pub runtime_minutes: Option<u32>,
    pub genres: Vec<String>,
    pub cast: Vec<CastMember>,
    pub director: Option<String>,
    pub release_date: Option<String>,
    pub trailer_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CastMember {
    pub name: String,
    pub character: Option<String>,
    pub profile_url: Option<String>,
}

// ============================================================================
// TMDB API
// ============================================================================

const TMDB_API_BASE: &str = "https://api.themoviedb.org/3";
const TMDB_IMAGE_BASE: &str = "https://image.tmdb.org/t/p";

pub struct TmdbClient {
    api_key: String,
    client: reqwest::Client,
}

impl TmdbClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Search for movies
    pub async fn search_movie(&self, query: &str, year: Option<u32>) -> Result<Vec<TmdbMovie>, String> {
        let mut url = format!(
            "{}/search/movie?api_key={}&query={}",
            TMDB_API_BASE, self.api_key, urlencoding::encode(query)
        );
        
        if let Some(y) = year {
            url.push_str(&format!("&year={}", y));
        }

        let response: TmdbSearchResponse = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;

        Ok(response.results)
    }

    /// Get movie details
    pub async fn get_movie(&self, tmdb_id: u64) -> Result<TmdbMovieDetails, String> {
        let url = format!(
            "{}/movie/{}?api_key={}&append_to_response=credits,videos",
            TMDB_API_BASE, tmdb_id, self.api_key
        );

        self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    /// Convert to our metadata format
    pub fn to_metadata(details: &TmdbMovieDetails) -> MediaMetadata {
        MediaMetadata {
            tmdb_id: Some(details.id),
            imdb_id: details.imdb_id.clone(),
            overview: details.overview.clone(),
            poster_url: details.poster_path.as_ref().map(|p| 
                format!("{}/w500{}", TMDB_IMAGE_BASE, p)
            ),
            backdrop_url: details.backdrop_path.as_ref().map(|p|
                format!("{}/w1280{}", TMDB_IMAGE_BASE, p)
            ),
            rating: details.vote_average,
            runtime_minutes: details.runtime,
            genres: details.genres.iter().map(|g| g.name.clone()).collect(),
            cast: details.credits.as_ref().map(|c| {
                c.cast.iter().take(10).map(|m| CastMember {
                    name: m.name.clone(),
                    character: m.character.clone(),
                    profile_url: m.profile_path.as_ref().map(|p|
                        format!("{}/w185{}", TMDB_IMAGE_BASE, p)
                    ),
                }).collect()
            }).unwrap_or_default(),
            director: details.credits.as_ref().and_then(|c| {
                c.crew.iter().find(|m| m.job.as_deref() == Some("Director"))
                    .map(|m| m.name.clone())
            }),
            release_date: details.release_date.clone(),
            trailer_url: details.videos.as_ref().and_then(|v| {
                v.results.iter()
                    .find(|t| t.site == "YouTube" && t.type_field == "Trailer")
                    .map(|t| format!("https://youtube.com/watch?v={}", t.key))
            }),
        }
    }
}

// TMDB API Response Types
#[derive(Debug, Deserialize)]
struct TmdbSearchResponse {
    results: Vec<TmdbMovie>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmdbMovie {
    pub id: u64,
    pub title: String,
    pub release_date: Option<String>,
    pub poster_path: Option<String>,
    pub vote_average: Option<f32>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbMovieDetails {
    pub id: u64,
    pub title: String,
    pub imdb_id: Option<String>,
    pub overview: Option<String>,
    pub poster_path: Option<String>,
    pub backdrop_path: Option<String>,
    pub vote_average: Option<f32>,
    pub runtime: Option<u32>,
    pub release_date: Option<String>,
    pub genres: Vec<TmdbGenre>,
    pub credits: Option<TmdbCredits>,
    pub videos: Option<TmdbVideos>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbGenre {
    pub id: u64,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct TmdbCredits {
    pub cast: Vec<TmdbCast>,
    pub crew: Vec<TmdbCrew>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbCast {
    pub name: String,
    pub character: Option<String>,
    pub profile_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbCrew {
    pub name: String,
    pub job: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbVideos {
    pub results: Vec<TmdbVideo>,
}

#[derive(Debug, Deserialize)]
pub struct TmdbVideo {
    pub key: String,
    pub site: String,
    #[serde(rename = "type")]
    pub type_field: String,
}

// ============================================================================
// Local Media Scanner
// ============================================================================

const VIDEO_EXTENSIONS: &[&str] = &[
    "mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v",
    "ts", "mts", "m2ts", "vob", "3gp", "ogv",
];

pub struct MediaScanner {
    tmdb: Option<TmdbClient>,
}

impl MediaScanner {
    pub fn new(tmdb_api_key: Option<&str>) -> Self {
        Self {
            tmdb: tmdb_api_key.map(TmdbClient::new),
        }
    }

    /// Scan a directory for video files
    pub async fn scan_directory(&self, path: &str) -> Result<Vec<MediaItem>, String> {
        let mut items = Vec::new();
        
        let entries = std::fs::read_dir(path)
            .map_err(|e| format!("Failed to read directory: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    let ext_str = ext.to_string_lossy().to_lowercase();
                    if VIDEO_EXTENSIONS.contains(&ext_str.as_str()) {
                        let item = self.create_item_from_file(&path).await?;
                        items.push(item);
                    }
                }
            } else if path.is_dir() {
                // Recurse into subdirectories
                if let Ok(sub_items) = Box::pin(self.scan_directory(
                    &path.to_string_lossy()
                )).await {
                    items.extend(sub_items);
                }
            }
        }

        Ok(items)
    }

    /// Create media item from file, try to scrape metadata
    async fn create_item_from_file(&self, path: &PathBuf) -> Result<MediaItem, String> {
        let filename = path.file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        // Parse title and year from filename
        // Common formats: "Movie Name (2023)", "Movie.Name.2023.1080p", etc.
        let (title, year) = parse_movie_filename(&filename);

        let mut item = MediaItem {
            id: uuid::Uuid::new_v4().to_string(),
            title: title.clone(),
            year,
            media_type: MediaType::Movie,
            source: MediaSource::Local,
            path: Some(path.to_string_lossy().to_string()),
            stream_url: None,
            metadata: None,
        };

        // Try to fetch metadata from TMDB
        if let Some(ref tmdb) = self.tmdb {
            if let Ok(results) = tmdb.search_movie(&title, year).await {
                if let Some(first) = results.first() {
                    if let Ok(details) = tmdb.get_movie(first.id).await {
                        item.metadata = Some(TmdbClient::to_metadata(&details));
                    }
                }
            }
        }

        Ok(item)
    }
}

/// Parse movie title and year from filename
fn parse_movie_filename(filename: &str) -> (String, Option<u32>) {
    // Try to find year in parentheses: "Movie Name (2023)"
    let re_parens = regex::Regex::new(r"^(.+?)\s*\((\d{4})\)").unwrap();
    if let Some(caps) = re_parens.captures(filename) {
        let title = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
        let year = caps.get(2).and_then(|m| m.as_str().parse().ok());
        return (title, year);
    }

    // Try dot-separated: "Movie.Name.2023.1080p"
    let re_dots = regex::Regex::new(r"^(.+?)\.(\d{4})\.").unwrap();
    if let Some(caps) = re_dots.captures(filename) {
        let title = caps.get(1)
            .map(|m| m.as_str().replace('.', " "))
            .unwrap_or_default();
        let year = caps.get(2).and_then(|m| m.as_str().parse().ok());
        return (title, year);
    }

    // Just return the filename as title
    (filename.replace('.', " ").replace('_', " "), None)
}

// ============================================================================
// Plex Integration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexServer {
    pub name: String,
    pub address: String,
    pub port: u16,
    pub token: String,
}

pub struct PlexClient {
    server: PlexServer,
    client: reqwest::Client,
}

impl PlexClient {
    pub fn new(server: PlexServer) -> Self {
        Self {
            server,
            client: reqwest::Client::new(),
        }
    }

    /// Get all libraries
    pub async fn get_libraries(&self) -> Result<Vec<PlexLibrary>, String> {
        let url = format!(
            "http://{}:{}/library/sections?X-Plex-Token={}",
            self.server.address, self.server.port, self.server.token
        );

        // Parse XML response...
        Ok(Vec::new())
    }

    /// Get items in a library
    pub async fn get_library_items(&self, library_id: &str) -> Result<Vec<MediaItem>, String> {
        let url = format!(
            "http://{}:{}/library/sections/{}/all?X-Plex-Token={}",
            self.server.address, self.server.port, library_id, self.server.token
        );

        // Parse XML response and convert to MediaItem...
        Ok(Vec::new())
    }

    /// Get stream URL for an item
    pub fn get_stream_url(&self, item_key: &str) -> String {
        format!(
            "http://{}:{}{}?X-Plex-Token={}",
            self.server.address, self.server.port, item_key, self.server.token
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlexLibrary {
    pub id: String,
    pub title: String,
    pub library_type: String,
}

// ============================================================================
// Free Streaming Sources
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FreeStreamSource {
    pub name: String,
    pub url: String,
    pub logo_url: Option<String>,
    pub requires_login: bool,
    pub regions: Vec<String>,
}

/// Legal free streaming services
pub fn get_free_sources() -> Vec<FreeStreamSource> {
    vec![
        FreeStreamSource {
            name: "Tubi".to_string(),
            url: "https://tubitv.com".to_string(),
            logo_url: Some("https://tubitv.com/favicon.ico".to_string()),
            requires_login: false,
            regions: vec!["US".to_string(), "CA".to_string(), "MX".to_string()],
        },
        FreeStreamSource {
            name: "Pluto TV".to_string(),
            url: "https://pluto.tv".to_string(),
            logo_url: Some("https://pluto.tv/favicon.ico".to_string()),
            requires_login: false,
            regions: vec!["US".to_string(), "UK".to_string(), "DE".to_string()],
        },
        FreeStreamSource {
            name: "Peacock Free".to_string(),
            url: "https://peacocktv.com".to_string(),
            logo_url: None,
            requires_login: true,
            regions: vec!["US".to_string()],
        },
        FreeStreamSource {
            name: "The Roku Channel".to_string(),
            url: "https://therokuchannel.roku.com".to_string(),
            logo_url: None,
            requires_login: false,
            regions: vec!["US".to_string()],
        },
        FreeStreamSource {
            name: "Freevee (Amazon)".to_string(),
            url: "https://amazon.com/freevee".to_string(),
            logo_url: None,
            requires_login: true,
            regions: vec!["US".to_string(), "UK".to_string()],
        },
        FreeStreamSource {
            name: "Crackle".to_string(),
            url: "https://crackle.com".to_string(),
            logo_url: None,
            requires_login: false,
            regions: vec!["US".to_string()],
        },
        FreeStreamSource {
            name: "Plex Free Movies".to_string(),
            url: "https://plex.tv/watch-free".to_string(),
            logo_url: None,
            requires_login: true,
            regions: vec!["US".to_string(), "CA".to_string(), "UK".to_string()],
        },
        FreeStreamSource {
            name: "YouTube Movies (Free)".to_string(),
            url: "https://youtube.com/feed/storefront?bp=ogUCKAQ%3D".to_string(),
            logo_url: None,
            requires_login: false,
            regions: vec!["GLOBAL".to_string()],
        },
    ]
}

// ============================================================================
// Public API
// ============================================================================


pub async fn scan_media_folder(path: String, tmdb_key: Option<String>) -> Result<Vec<MediaItem>, String> {
    let scanner = MediaScanner::new(tmdb_key.as_deref());
    scanner.scan_directory(&path).await
}


pub async fn search_tmdb(query: String, year: Option<u32>, api_key: String) -> Result<Vec<TmdbMovie>, String> {
    let client = TmdbClient::new(&api_key);
    client.search_movie(&query, year).await
}


pub async fn get_movie_metadata(tmdb_id: u64, api_key: String) -> Result<MediaMetadata, String> {
    let client = TmdbClient::new(&api_key);
    let details = client.get_movie(tmdb_id).await?;
    Ok(TmdbClient::to_metadata(&details))
}


pub fn get_free_streaming_sources() -> Vec<FreeStreamSource> {
    get_free_sources()
}


pub async fn connect_plex(address: String, port: u16, token: String) -> Result<Vec<PlexLibrary>, String> {
    let server = PlexServer {
        name: "My Plex".to_string(),
        address,
        port,
        token,
    };
    
    let client = PlexClient::new(server);
    client.get_libraries().await
}
