// DEBRID - Premium Link Resolver Integration
// 
// Supports: Real-Debrid, AllDebrid, Premiumize, Put.io
// 
// LEGAL: These are legitimate premium download services.
// Users provide their own API keys and subscriptions.
// What content users access is their responsibility.
// 
// How it works:
// 1. User has subscription to debrid service
// 2. User provides their API key in settings
// 3. SLAIN uses API to resolve links to fast premium servers
// 4. Better speeds, less buffering, more sources

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;
use once_cell::sync::Lazy;


// ============================================================================
// Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebridConfig {
    // Service API keys (user provides their own)
    pub real_debrid_key: Option<String>,
    pub all_debrid_key: Option<String>,
    pub premiumize_key: Option<String>,
    pub putio_key: Option<String>,
    
    // Preferences
    pub preferred_service: DebridService,
    pub auto_resolve: bool,           // Auto-resolve links when detected
    pub prefer_cached: bool,          // Prefer already-cached content
    pub max_filesize_gb: u32,         // Skip files larger than this
    pub preferred_quality: Quality,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DebridService {
    RealDebrid,
    AllDebrid,
    Premiumize,
    PutIO,
    Auto,  // Use whichever has the link cached
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    Highest,    // 4K if available
    High,       // 1080p
    Medium,     // 720p
    Low,        // 480p
    Lowest,     // Whatever works
}

impl Default for DebridConfig {
    fn default() -> Self {
        Self {
            real_debrid_key: None,
            all_debrid_key: None,
            premiumize_key: None,
            putio_key: None,
            preferred_service: DebridService::Auto,
            auto_resolve: true,
            prefer_cached: true,
            max_filesize_gb: 50,
            preferred_quality: Quality::High,
        }
    }
}

// ============================================================================
// Link Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnresolvedLink {
    pub url: String,
    pub host: String,           // mega, rapidgator, etc
    pub filename: Option<String>,
    pub filesize: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedLink {
    pub original_url: String,
    pub download_url: String,   // Direct download link
    pub stream_url: Option<String>,  // If streamable
    pub filename: String,
    pub filesize: u64,
    pub host: String,
    pub service: DebridService,
    pub is_cached: bool,
    pub quality: Option<String>,
    pub mime_type: Option<String>,
    
    // Streaming info
    pub streamable: bool,
    pub transcode_available: bool,
    pub available_qualities: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheStatus {
    pub url: String,
    pub is_cached: bool,
    pub service: DebridService,
    pub instant_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountInfo {
    pub service: DebridService,
    pub username: String,
    pub email: Option<String>,
    pub premium: bool,
    pub premium_until: Option<String>,
    pub points: Option<u32>,
    pub quota_used: Option<u64>,
    pub quota_total: Option<u64>,
}

// ============================================================================
// API Clients
// ============================================================================

pub struct RealDebridClient {
    api_key: String,
    base_url: String,
}

impl RealDebridClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.real-debrid.com/rest/1.0".to_string(),
        }
    }
    
    /// Check if user has valid premium account
    pub async fn get_user(&self) -> Result<AccountInfo, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/user", self.base_url);
        
        let response = client.get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            return Err(format!("API error: {}", response.status()));
        }
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        Ok(AccountInfo {
            service: DebridService::RealDebrid,
            username: data["username"].as_str().unwrap_or("").to_string(),
            email: data["email"].as_str().map(String::from),
            premium: data["type"].as_str() == Some("premium"),
            premium_until: data["expiration"].as_str().map(String::from),
            points: data["points"].as_u64().map(|p| p as u32),
            quota_used: None,
            quota_total: None,
        })
    }
    
    /// Unrestrict a link (resolve to direct download)
    pub async fn unrestrict(&self, url: &str) -> Result<ResolvedLink, String> {
        let client = reqwest::Client::new();
        let api_url = format!("{}/unrestrict/link", self.base_url);
        
        let mut params = HashMap::new();
        params.insert("link", url);
        
        let response = client.post(&api_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(format!("API error {}: {}", status, body));
        }
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        Ok(ResolvedLink {
            original_url: url.to_string(),
            download_url: data["download"].as_str().unwrap_or("").to_string(),
            stream_url: None,
            filename: data["filename"].as_str().unwrap_or("").to_string(),
            filesize: data["filesize"].as_u64().unwrap_or(0),
            host: data["host"].as_str().unwrap_or("").to_string(),
            service: DebridService::RealDebrid,
            is_cached: true,
            quality: data["type"].as_str().map(String::from),
            mime_type: data["mimeType"].as_str().map(String::from),
            streamable: data["streamable"].as_i64() == Some(1),
            transcode_available: false,
            available_qualities: Vec::new(),
        })
    }
    
    /// Check if content is cached (instant availability)
    pub async fn check_cache(&self, url: &str) -> Result<CacheStatus, String> {
        let client = reqwest::Client::new();
        let api_url = format!("{}/unrestrict/check", self.base_url);
        
        let mut params = HashMap::new();
        params.insert("link", url);
        
        let response = client.post(&api_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        Ok(CacheStatus {
            url: url.to_string(),
            is_cached: data["supported"].as_i64() == Some(1),
            service: DebridService::RealDebrid,
            instant_available: data["supported"].as_i64() == Some(1),
        })
    }
    
    /// Get streaming info for a file
    pub async fn get_streaming_info(&self, file_id: &str) -> Result<serde_json::Value, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/streaming/transcode/{}", self.base_url, file_id);
        
        let response = client.get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))
    }
    
    /// Get list of supported hosts
    pub async fn get_hosts(&self) -> Result<Vec<String>, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/hosts", self.base_url);
        
        let response = client.get(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        // Extract host domains
        let mut hosts = Vec::new();
        if let Some(obj) = data.as_object() {
            for (key, _) in obj {
                hosts.push(key.clone());
            }
        }
        
        Ok(hosts)
    }
}

pub struct AllDebridClient {
    api_key: String,
    base_url: String,
}

impl AllDebridClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.alldebrid.com/v4".to_string(),
        }
    }
    
    pub async fn get_user(&self) -> Result<AccountInfo, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/user?agent=SLAIN&apikey={}", self.base_url, self.api_key);
        
        let response = client.get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        let user = &data["data"]["user"];
        
        Ok(AccountInfo {
            service: DebridService::AllDebrid,
            username: user["username"].as_str().unwrap_or("").to_string(),
            email: user["email"].as_str().map(String::from),
            premium: user["isPremium"].as_bool().unwrap_or(false),
            premium_until: user["premiumUntil"].as_str().map(String::from),
            points: None,
            quota_used: None,
            quota_total: None,
        })
    }
    
    pub async fn unrestrict(&self, url: &str) -> Result<ResolvedLink, String> {
        let client = reqwest::Client::new();
        let api_url = format!(
            "{}/link/unlock?agent=SLAIN&apikey={}&link={}",
            self.base_url, self.api_key, urlencoding::encode(url)
        );
        
        let response = client.get(&api_url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        if data["status"].as_str() != Some("success") {
            return Err(format!("API error: {:?}", data["error"]));
        }
        
        let link_data = &data["data"];
        
        Ok(ResolvedLink {
            original_url: url.to_string(),
            download_url: link_data["link"].as_str().unwrap_or("").to_string(),
            stream_url: link_data["streaming"].as_str().map(String::from),
            filename: link_data["filename"].as_str().unwrap_or("").to_string(),
            filesize: link_data["filesize"].as_u64().unwrap_or(0),
            host: link_data["host"].as_str().unwrap_or("").to_string(),
            service: DebridService::AllDebrid,
            is_cached: true,
            quality: None,
            mime_type: None,
            streamable: link_data["streaming"].is_string(),
            transcode_available: false,
            available_qualities: Vec::new(),
        })
    }
}

pub struct PremiumizeClient {
    api_key: String,
    base_url: String,
}

impl PremiumizeClient {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://www.premiumize.me/api".to_string(),
        }
    }
    
    pub async fn get_user(&self) -> Result<AccountInfo, String> {
        let client = reqwest::Client::new();
        let url = format!("{}/account/info?apikey={}", self.base_url, self.api_key);
        
        let response = client.get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        Ok(AccountInfo {
            service: DebridService::Premiumize,
            username: data["customer_id"].as_str().unwrap_or("").to_string(),
            email: None,
            premium: data["status"].as_str() == Some("active"),
            premium_until: data["premium_until"].as_i64()
                .map(|ts| chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default()),
            points: None,
            quota_used: data["space_used"].as_f64().map(|f| f as u64),
            quota_total: Some(1024 * 1024 * 1024 * 1024), // 1TB
        })
    }
    
    pub async fn unrestrict(&self, url: &str) -> Result<ResolvedLink, String> {
        let client = reqwest::Client::new();
        let api_url = format!("{}/transfer/directdl?apikey={}", self.base_url, self.api_key);
        
        let mut params = HashMap::new();
        params.insert("src", url);
        
        let response = client.post(&api_url)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        if data["status"].as_str() != Some("success") {
            return Err(format!("API error: {:?}", data["message"]));
        }
        
        // Premiumize returns content array
        let content = data["content"].as_array()
            .and_then(|arr| arr.first())
            .ok_or("No content returned")?;
        
        Ok(ResolvedLink {
            original_url: url.to_string(),
            download_url: content["link"].as_str().unwrap_or("").to_string(),
            stream_url: content["stream_link"].as_str().map(String::from),
            filename: data["filename"].as_str().unwrap_or("").to_string(),
            filesize: data["filesize"].as_u64().unwrap_or(0),
            host: "premiumize".to_string(),
            service: DebridService::Premiumize,
            is_cached: true,
            quality: None,
            mime_type: content["mime_type"].as_str().map(String::from),
            streamable: content["stream_link"].is_string(),
            transcode_available: content["transcode_status"].as_str() == Some("finished"),
            available_qualities: Vec::new(),
        })
    }
    
    pub async fn check_cache(&self, hashes: &[&str]) -> Result<Vec<CacheStatus>, String> {
        let client = reqwest::Client::new();
        let items = hashes.join(",");
        let url = format!(
            "{}/cache/check?apikey={}&items[]={}",
            self.base_url, self.api_key, items
        );
        
        let response = client.get(&url)
            .send()
            .await
            .map_err(|e| format!("Request failed: {}", e))?;
        
        let data: serde_json::Value = response.json()
            .await
            .map_err(|e| format!("Parse error: {}", e))?;
        
        let mut results = Vec::new();
        if let Some(response_arr) = data["response"].as_array() {
            for (i, is_cached) in response_arr.iter().enumerate() {
                if i < hashes.len() {
                    results.push(CacheStatus {
                        url: hashes[i].to_string(),
                        is_cached: is_cached.as_bool().unwrap_or(false),
                        service: DebridService::Premiumize,
                        instant_available: is_cached.as_bool().unwrap_or(false),
                    });
                }
            }
        }
        
        Ok(results)
    }
}

// ============================================================================
// Unified Debrid Manager
// ============================================================================

pub struct DebridManager {
    config: DebridConfig,
}

impl DebridManager {
    pub fn new(config: DebridConfig) -> Self {
        Self { config }
    }
    
    /// Get available (configured) services
    pub fn available_services(&self) -> Vec<DebridService> {
        let mut services = Vec::new();
        if self.config.real_debrid_key.is_some() {
            services.push(DebridService::RealDebrid);
        }
        if self.config.all_debrid_key.is_some() {
            services.push(DebridService::AllDebrid);
        }
        if self.config.premiumize_key.is_some() {
            services.push(DebridService::Premiumize);
        }
        if self.config.putio_key.is_some() {
            services.push(DebridService::PutIO);
        }
        services
    }
    
    /// Resolve a link using the best available service
    pub async fn resolve(&self, url: &str) -> Result<ResolvedLink, String> {
        let service = match self.config.preferred_service {
            DebridService::Auto => {
                // Try in order of preference
                if self.config.real_debrid_key.is_some() {
                    DebridService::RealDebrid
                } else if self.config.all_debrid_key.is_some() {
                    DebridService::AllDebrid
                } else if self.config.premiumize_key.is_some() {
                    DebridService::Premiumize
                } else {
                    return Err("No debrid service configured".to_string());
                }
            }
            other => other,
        };
        
        match service {
            DebridService::RealDebrid => {
                let key = self.config.real_debrid_key.as_ref()
                    .ok_or("Real-Debrid not configured")?;
                RealDebridClient::new(key.clone()).unrestrict(url).await
            }
            DebridService::AllDebrid => {
                let key = self.config.all_debrid_key.as_ref()
                    .ok_or("AllDebrid not configured")?;
                AllDebridClient::new(key.clone()).unrestrict(url).await
            }
            DebridService::Premiumize => {
                let key = self.config.premiumize_key.as_ref()
                    .ok_or("Premiumize not configured")?;
                PremiumizeClient::new(key.clone()).unrestrict(url).await
            }
            _ => Err("Service not implemented".to_string()),
        }
    }
    
    /// Check account status for all configured services
    pub async fn check_accounts(&self) -> Vec<Result<AccountInfo, String>> {
        let mut results = Vec::new();
        
        if let Some(ref key) = self.config.real_debrid_key {
            results.push(RealDebridClient::new(key.clone()).get_user().await);
        }
        if let Some(ref key) = self.config.all_debrid_key {
            results.push(AllDebridClient::new(key.clone()).get_user().await);
        }
        if let Some(ref key) = self.config.premiumize_key {
            results.push(PremiumizeClient::new(key.clone()).get_user().await);
        }
        
        results
    }
}

// ============================================================================
// Global State
// ============================================================================

static DEBRID_CONFIG: Lazy<RwLock<DebridConfig>> = Lazy::new(|| {
    RwLock::new(DebridConfig::default())
});

// ============================================================================
// Public API
// ============================================================================


pub fn debrid_get_config() -> DebridConfig {
    DEBRID_CONFIG.read().unwrap().clone()
}


pub fn debrid_set_config(config: DebridConfig) {
    *DEBRID_CONFIG.write().unwrap() = config;
}


pub fn debrid_set_key(service: DebridService, key: String) {
    let mut config = DEBRID_CONFIG.write().unwrap();
    match service {
        DebridService::RealDebrid => config.real_debrid_key = Some(key),
        DebridService::AllDebrid => config.all_debrid_key = Some(key),
        DebridService::Premiumize => config.premiumize_key = Some(key),
        DebridService::PutIO => config.putio_key = Some(key),
        _ => {}
    }
}


pub fn debrid_available_services() -> Vec<String> {
    let config = DEBRID_CONFIG.read().unwrap();
    let manager = DebridManager::new(config.clone());
    manager.available_services()
        .into_iter()
        .map(|s| format!("{:?}", s))
        .collect()
}


pub async fn debrid_resolve(url: String) -> Result<ResolvedLink, String> {
    let config = DEBRID_CONFIG.read().unwrap().clone();
    let manager = DebridManager::new(config);
    manager.resolve(&url).await
}


pub async fn debrid_check_accounts() -> Vec<serde_json::Value> {
    let config = DEBRID_CONFIG.read().unwrap().clone();
    let manager = DebridManager::new(config);
    
    manager.check_accounts().await
        .into_iter()
        .map(|r| match r {
            Ok(info) => serde_json::to_value(info).unwrap_or_default(),
            Err(e) => serde_json::json!({ "error": e }),
        })
        .collect()
}


pub async fn debrid_get_hosts(service: DebridService) -> Result<Vec<String>, String> {
    let config = DEBRID_CONFIG.read().unwrap();
    
    match service {
        DebridService::RealDebrid => {
            let key = config.real_debrid_key.as_ref()
                .ok_or("Real-Debrid not configured")?;
            RealDebridClient::new(key.clone()).get_hosts().await
        }
        _ => Err("Not implemented for this service".to_string()),
    }
}
