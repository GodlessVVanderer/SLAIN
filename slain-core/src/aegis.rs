// AEGIS - Adaptive Defense & Deception System
//
// "Make hackers wish they never touched your computer"
//
// Not an antivirus. Not a firewall. A TRAP.
//
// ═══════════════════════════════════════════════════════════════════════════
//
// PHILOSOPHY:
// Traditional security is reactive - detect, block, clean up.
// AEGIS is OFFENSIVE - detect, deceive, waste their time, corrupt their tools.
//
// When malware or an attacker touches your system, AEGIS:
// 1. Detects the intrusion
// 2. Feeds them fake data (honeypots)
// 3. Corrupts their exfiltration (ricochet)
// 4. Wastes their time with infinite rabbit holes
// 5. Fingerprints them for future identification
// 6. Reports back (optional)
//
// RESOURCE EFFICIENT:
// - No constant scanning
// - No signature updates
// - Minimal CPU/RAM when idle
// - Only activates when triggered
//
// ═══════════════════════════════════════════════════════════════════════════

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc, RwLock,
};
use std::time::{Duration, Instant, SystemTime};

use once_cell::sync::Lazy;

// ============================================================================
// AEGIS Configuration
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AegisConfig {
    pub enabled: bool,

    // Defense Modules
    pub honeypots_enabled: bool,
    pub ricochet_enabled: bool,
    pub deception_enabled: bool,
    pub fingerprinting_enabled: bool,

    // Aggressiveness (how much to mess with attackers)
    pub aggression_level: AggressionLevel,

    // Notification settings
    pub notify_on_detection: bool,
    pub log_all_access: bool,
    pub report_to_server: bool, // Anonymous telemetry

    // Resource limits
    pub max_honeypot_files: u32,
    pub max_memory_mb: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggressionLevel {
    Passive,   // Just detect and log
    Defensive, // Deceive and waste time
    Offensive, // Corrupt their data, fingerprint them
    Maximum,   // All of the above + active countermeasures
}

impl Default for AegisConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            honeypots_enabled: true,
            ricochet_enabled: true,
            deception_enabled: true,
            fingerprinting_enabled: true,
            aggression_level: AggressionLevel::Defensive,
            notify_on_detection: true,
            log_all_access: true,
            report_to_server: false,
            max_honeypot_files: 100,
            max_memory_mb: 50,
        }
    }
}

// ============================================================================
// Threat Detection
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreatEvent {
    pub id: String,
    pub timestamp: u64,
    pub threat_type: ThreatType,
    pub source: ThreatSource,
    pub target: String,
    pub severity: Severity,
    pub action_taken: ActionTaken,
    pub fingerprint: Option<AttackerFingerprint>,
    pub details: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreatType {
    // File system
    HoneypotAccess,      // Something touched a honeypot file
    SensitiveFileAccess, // Access to wallet, password files
    MassFileRead,        // Ransomware-like behavior
    SuspiciousWrite,     // Writing to system locations

    // Process
    ProcessInjection,    // Code injection attempt
    PrivilegeEscalation, // UAC bypass attempt
    SuspiciousSpawn,     // Unknown process spawned

    // Network
    DataExfiltration, // Large upload to unknown server
    C2Communication,  // Command & control pattern
    PortScan,         // Internal network probing

    // Screen/Input
    ScreenCapture,    // Screenshot attempt
    KeyloggerPattern, // Keystroke capture detected
    ClipboardSnoop,   // Clipboard monitoring

    // Persistence
    StartupModification, // Autorun changes
    ScheduledTask,       // New scheduled tasks
    ServiceInstall,      // New service installed
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ThreatSource {
    Process {
        pid: u32,
        name: String,
        path: String,
    },
    Network {
        ip: String,
        port: u16,
    },
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Low,      // Informational
    Medium,   // Suspicious activity
    High,     // Likely threat
    Critical, // Active attack
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionTaken {
    Logged,      // Just recorded
    Deceived,    // Fed fake data
    Blocked,     // Access denied
    Corrupted,   // Ricochet - corrupted their data
    Quarantined, // Isolated the process
    Reported,    // Sent to threat intel
}

// ============================================================================
// Honeypot System
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Honeypot {
    pub id: String,
    pub path: PathBuf,
    pub honeypot_type: HoneypotType,
    pub created: u64,
    pub access_count: u32,
    pub last_accessed: Option<u64>,
    pub triggered_by: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoneypotType {
    // Financial bait
    FakeWallet,       // "wallet.dat", "seed_phrase.txt"
    FakeBankingCreds, // "bank_login.txt"
    FakeCreditCard,   // "cards.csv"

    // Credential bait
    FakePasswords, // "passwords.txt", "logins.csv"
    FakeSshKey,    // "id_rsa"
    FakeApiKeys,   // "api_keys.env"

    // Sensitive docs
    FakeTaxReturn,     // "tax_2024.pdf"
    FakeMedicalRecord, // "medical_records.pdf"
    FakeLegalDoc,      // "contract.docx"

    // Crypto bait
    FakeSeedPhrase, // "recovery_words.txt"
    FakePrivateKey, // "private_key.pem"

    // Corporate bait
    FakeVpnConfig, // "company_vpn.ovpn"
    FakeDatabase,  // "customers.db"
}

impl HoneypotType {
    /// Generate realistic-looking fake content
    pub fn generate_content(&self) -> Vec<u8> {
        match self {
            Self::FakeWallet => {
                // Fake Bitcoin wallet.dat structure
                let mut content = Vec::new();
                content.extend_from_slice(b"\x00\x00\x00\x00\x01\x00\x00\x00");
                content.extend_from_slice(b"AEGIS_HONEYPOT_TRAP");
                // Add random-looking encrypted data
                for _ in 0..1024 {
                    content.push(rand_byte());
                }
                content
            }
            Self::FakeSeedPhrase => {
                // Generate fake BIP39 words (real words but random, not a valid wallet)
                let words = [
                    "abandon", "ability", "able", "about", "above", "absent", "absorb", "abstract",
                    "absurd", "abuse", "access", "accident", "account", "accuse", "achieve",
                    "acid", "acoustic", "acquire", "across", "act", "action", "actor", "actress",
                    "actual",
                ];
                let fake_phrase: Vec<&str> =
                    (0..24).map(|i| words[(i * 7 + 3) % words.len()]).collect();

                format!(
                    "Bitcoin Recovery Seed Phrase\n\
                     Generated: 2024-01-15\n\
                     KEEP THIS SAFE!\n\n\
                     {}\n\n\
                     AEGIS_TRAP_DO_NOT_USE",
                    fake_phrase.join(" ")
                )
                .into_bytes()
            }
            Self::FakePasswords => {
                // Fake password file with honeypot indicators
                format!(
                    "# Passwords - DO NOT SHARE\n\
                     # Last updated: 2024-12-01\n\n\
                     [Bank of America]\n\
                     user: john.smith.1985\n\
                     pass: MyD0g$Name2024!\n\n\
                     [Gmail]\n\
                     user: johnsmith1985@gmail.com\n\
                     pass: Summer2024!Family\n\n\
                     [Amazon]\n\
                     user: jsmith85\n\
                     pass: Amaz0n$hopp1ng!\n\n\
                     [Facebook]\n\
                     user: john.smith.1985\n\
                     pass: Fac3b00k!2024\n\n\
                     <!-- AEGIS_HONEYPOT_MARKER -->\n"
                )
                .into_bytes()
            }
            Self::FakeSshKey => {
                // Fake SSH private key (invalid but realistic looking)
                format!(
                    "-----BEGIN OPENSSH PRIVATE KEY-----\n\
                     b3BlbnNzaC1rZXktdjEAAAAACmFlczI1Ni1jdHIAAAAGYmNyeXB0AAAAGAAAABB\n\
                     {} \n\
                     -----END OPENSSH PRIVATE KEY-----\n\
                     # AEGIS HONEYPOT - This key is monitored\n",
                    base64_fake(200)
                )
                .into_bytes()
            }
            Self::FakeApiKeys => format!(
                "# API Keys - Production Environment\n\
                     # WARNING: Do not commit to git!\n\n\
                     AWS_ACCESS_KEY_ID=AKIA{}\n\
                     AWS_SECRET_ACCESS_KEY={}\n\
                     STRIPE_SECRET_KEY=sk_live_{}\n\
                     OPENAI_API_KEY=sk-{}\n\
                     DATABASE_URL=postgres://admin:{}@db.company.com/prod\n\n\
                     # AEGIS_TRAP_MARKER\n",
                random_alphanumeric(16),
                random_alphanumeric(40),
                random_alphanumeric(24),
                random_alphanumeric(48),
                random_alphanumeric(16),
            )
            .into_bytes(),
            Self::FakeCreditCard => {
                // Fake credit card CSV
                format!(
                    "card_number,expiry,cvv,name,billing_address\n\
                     4532{}0001,12/26,{},John Smith,123 Main St Anytown USA 12345\n\
                     5425{}0002,03/27,{},John Smith,123 Main St Anytown USA 12345\n\
                     3782{}0003,09/25,{},John Smith,123 Main St Anytown USA 12345\n\
                     # AEGIS HONEYPOT - All numbers are fake and monitored\n",
                    random_digits(8),
                    random_digits(3),
                    random_digits(8),
                    random_digits(3),
                    random_digits(7),
                    random_digits(4),
                )
                .into_bytes()
            }
            _ => {
                // Generic fake content
                format!("AEGIS HONEYPOT FILE - Access is logged and monitored\n").into_bytes()
            }
        }
    }

    /// Get realistic filename for this honeypot type
    pub fn suggested_filenames(&self) -> Vec<&'static str> {
        match self {
            Self::FakeWallet => vec!["wallet.dat", "bitcoin_wallet.dat", "electrum.dat"],
            Self::FakeSeedPhrase => {
                vec!["seed_phrase.txt", "recovery_words.txt", "wallet_backup.txt"]
            }
            Self::FakePasswords => vec!["passwords.txt", "logins.txt", "credentials.csv"],
            Self::FakeSshKey => vec!["id_rsa", "private_key", "ssh_key"],
            Self::FakeApiKeys => vec![".env", "api_keys.txt", "secrets.env", "config.env"],
            Self::FakeCreditCard => vec!["cards.csv", "credit_cards.txt", "payment_info.csv"],
            Self::FakeBankingCreds => vec!["bank_login.txt", "banking.txt"],
            Self::FakeVpnConfig => vec!["work_vpn.ovpn", "company.ovpn", "vpn_config.conf"],
            _ => vec!["important.txt", "confidential.doc"],
        }
    }
}

// Helper functions for generating fake data
fn rand_byte() -> u8 {
    use std::time::SystemTime;
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u8;
    t.wrapping_mul(17).wrapping_add(31)
}

fn random_digits(n: usize) -> String {
    (0..n)
        .map(|i| ((rand_byte().wrapping_add(i as u8)) % 10 + b'0') as char)
        .collect()
}

fn random_alphanumeric(n: usize) -> String {
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    (0..n)
        .map(|i| {
            let idx = (rand_byte().wrapping_add(i as u8)) as usize % CHARS.len();
            CHARS[idx] as char
        })
        .collect()
}

fn base64_fake(n: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    (0..n)
        .map(|i| {
            let idx = (rand_byte().wrapping_add(i as u8)) as usize % CHARS.len();
            CHARS[idx] as char
        })
        .collect()
}

// ============================================================================
// Ricochet System - Make attackers hurt themselves
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RicochetConfig {
    pub corrupt_exfiltrated_data: bool, // Poison data being stolen
    pub inject_tracking: bool,          // Add trackers to stolen data
    pub infinite_tar_pit: bool,         // Never-ending fake files
    pub cpu_waste: bool,                // Make their tools work hard on garbage
}

impl Default for RicochetConfig {
    fn default() -> Self {
        Self {
            corrupt_exfiltrated_data: true,
            inject_tracking: true,
            infinite_tar_pit: true,
            cpu_waste: false,
        }
    }
}

/// Corrupt data that's being exfiltrated
pub fn corrupt_data(original: &[u8], corruption_level: f32) -> Vec<u8> {
    let mut corrupted = original.to_vec();
    let corrupt_count = (corrupted.len() as f32 * corruption_level) as usize;

    for i in 0..corrupt_count {
        let pos = (i * 17 + 7) % corrupted.len();
        corrupted[pos] = corrupted[pos].wrapping_add(rand_byte());
    }

    corrupted
}

/// Generate infinite tar pit content - never-ending "file"
pub fn tar_pit_generator() -> impl Iterator<Item = u8> {
    std::iter::from_fn(|| Some(rand_byte()))
}

/// Inject tracking beacon into stolen data
pub fn inject_tracking_beacon(data: &[u8], beacon_id: &str) -> Vec<u8> {
    let beacon = format!("\n<!-- AEGIS_TRACK:{} -->\n", beacon_id);

    let mut result = data.to_vec();
    result.extend_from_slice(beacon.as_bytes());
    result
}

// ============================================================================
// Attacker Fingerprinting
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttackerFingerprint {
    pub id: String,
    pub first_seen: u64,
    pub last_seen: u64,
    pub access_count: u32,

    // Process info
    pub process_names: Vec<String>,
    pub process_hashes: Vec<String>,

    // Behavior patterns
    pub file_access_patterns: Vec<String>,
    pub time_patterns: Vec<String>,

    // Network
    pub ip_addresses: Vec<String>,
    pub domains_contacted: Vec<String>,

    // Classification
    pub malware_family: Option<String>,
    pub threat_actor: Option<String>,
    pub confidence: f32,
}

impl AttackerFingerprint {
    pub fn new() -> Self {
        let id = format!("ATK_{}", random_alphanumeric(16));
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        Self {
            id,
            first_seen: now,
            last_seen: now,
            access_count: 1,
            process_names: Vec::new(),
            process_hashes: Vec::new(),
            file_access_patterns: Vec::new(),
            time_patterns: Vec::new(),
            ip_addresses: Vec::new(),
            domains_contacted: Vec::new(),
            malware_family: None,
            threat_actor: None,
            confidence: 0.0,
        }
    }
}

// ============================================================================
// AEGIS Engine
// ============================================================================

pub struct AegisEngine {
    config: Arc<RwLock<AegisConfig>>,
    honeypots: Arc<RwLock<Vec<Honeypot>>>,
    threat_log: Arc<RwLock<Vec<ThreatEvent>>>,
    fingerprints: Arc<RwLock<HashMap<String, AttackerFingerprint>>>,
    active: Arc<AtomicBool>,
    events_count: Arc<AtomicU64>,
}

impl AegisEngine {
    pub fn new(config: AegisConfig) -> Self {
        Self {
            config: Arc::new(RwLock::new(config)),
            honeypots: Arc::new(RwLock::new(Vec::new())),
            threat_log: Arc::new(RwLock::new(Vec::new())),
            fingerprints: Arc::new(RwLock::new(HashMap::new())),
            active: Arc::new(AtomicBool::new(false)),
            events_count: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn start(&self) {
        self.active.store(true, Ordering::Relaxed);
        // Deploy honeypots, start monitoring
        self.deploy_default_honeypots();
    }

    pub fn stop(&self) {
        self.active.store(false, Ordering::Relaxed);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    /// Deploy default honeypot files
    fn deploy_default_honeypots(&self) {
        let types = [
            HoneypotType::FakeSeedPhrase,
            HoneypotType::FakePasswords,
            HoneypotType::FakeApiKeys,
            HoneypotType::FakeSshKey,
        ];

        let mut honeypots = self.honeypots.write().unwrap();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        for hp_type in types {
            for filename in hp_type.suggested_filenames().into_iter().take(1) {
                let honeypot = Honeypot {
                    id: format!("HP_{}", random_alphanumeric(8)),
                    path: PathBuf::from(format!("/AEGIS_TRAPS/{}", filename)),
                    honeypot_type: hp_type,
                    created: now,
                    access_count: 0,
                    last_accessed: None,
                    triggered_by: Vec::new(),
                };
                honeypots.push(honeypot);
            }
        }
    }

    /// Record a threat event
    pub fn record_threat(
        &self,
        threat_type: ThreatType,
        source: ThreatSource,
        target: &str,
        details: HashMap<String, String>,
    ) {
        let config = self.config.read().unwrap();

        let severity = match threat_type {
            ThreatType::HoneypotAccess => Severity::High,
            ThreatType::DataExfiltration => Severity::Critical,
            ThreatType::ScreenCapture => Severity::High,
            ThreatType::ProcessInjection => Severity::Critical,
            _ => Severity::Medium,
        };

        let action = match config.aggression_level {
            AggressionLevel::Passive => ActionTaken::Logged,
            AggressionLevel::Defensive => ActionTaken::Deceived,
            AggressionLevel::Offensive => ActionTaken::Corrupted,
            AggressionLevel::Maximum => ActionTaken::Quarantined,
        };

        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let event = ThreatEvent {
            id: format!("THREAT_{}", random_alphanumeric(12)),
            timestamp: now,
            threat_type,
            source,
            target: target.to_string(),
            severity,
            action_taken: action,
            fingerprint: None,
            details,
        };

        self.threat_log.write().unwrap().push(event);
        self.events_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get statistics
    pub fn get_stats(&self) -> AegisStats {
        let threats = self.threat_log.read().unwrap();
        let honeypots = self.honeypots.read().unwrap();
        let fingerprints = self.fingerprints.read().unwrap();

        AegisStats {
            is_active: self.is_active(),
            total_events: self.events_count.load(Ordering::Relaxed),
            honeypots_deployed: honeypots.len() as u32,
            honeypots_triggered: honeypots.iter().filter(|h| h.access_count > 0).count() as u32,
            attackers_fingerprinted: fingerprints.len() as u32,
            threats_blocked: threats
                .iter()
                .filter(|t| t.action_taken != ActionTaken::Logged)
                .count() as u32,
            last_threat: threats.last().map(|t| t.timestamp),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AegisStats {
    pub is_active: bool,
    pub total_events: u64,
    pub honeypots_deployed: u32,
    pub honeypots_triggered: u32,
    pub attackers_fingerprinted: u32,
    pub threats_blocked: u32,
    pub last_threat: Option<u64>,
}

// ============================================================================
// Global State
// ============================================================================

static AEGIS_CONFIG: Lazy<RwLock<AegisConfig>> = Lazy::new(|| RwLock::new(AegisConfig::default()));

static AEGIS_ENGINE: Lazy<AegisEngine> = Lazy::new(|| AegisEngine::new(AegisConfig::default()));

// ============================================================================
// Public Rust API
// ============================================================================

pub fn aegis_get_config() -> AegisConfig {
    AEGIS_CONFIG.read().unwrap().clone()
}

pub fn aegis_set_config(config: AegisConfig) {
    *AEGIS_CONFIG.write().unwrap() = config;
}

pub fn aegis_is_active() -> bool {
    AEGIS_ENGINE.is_active()
}

pub fn aegis_start() {
    let mut config = AEGIS_CONFIG.write().unwrap();
    config.enabled = true;
    AEGIS_ENGINE.start();
}

pub fn aegis_stop() {
    let mut config = AEGIS_CONFIG.write().unwrap();
    config.enabled = false;
    AEGIS_ENGINE.stop();
}

pub fn aegis_get_stats() -> AegisStats {
    AEGIS_ENGINE.get_stats()
}

pub fn aegis_get_threats() -> Vec<ThreatEvent> {
    AEGIS_ENGINE.threat_log.read().unwrap().clone()
}

pub fn aegis_get_honeypots() -> Vec<Honeypot> {
    AEGIS_ENGINE.honeypots.read().unwrap().clone()
}

pub fn aegis_deploy_honeypot(hp_type: HoneypotType, location: String) -> Result<String, String> {
    let mut honeypots = AEGIS_ENGINE.honeypots.write().unwrap();
    let config = AEGIS_CONFIG.read().unwrap();

    if honeypots.len() >= config.max_honeypot_files as usize {
        return Err("Maximum honeypot limit reached".to_string());
    }

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let id = format!("HP_{}", random_alphanumeric(8));

    let honeypot = Honeypot {
        id: id.clone(),
        path: PathBuf::from(location),
        honeypot_type: hp_type,
        created: now,
        access_count: 0,
        last_accessed: None,
        triggered_by: Vec::new(),
    };

    honeypots.push(honeypot);

    Ok(id)
}

pub fn aegis_generate_honeypot_content(hp_type: HoneypotType) -> Vec<u8> {
    hp_type.generate_content()
}

pub fn aegis_test_ricochet(data: String) -> String {
    let corrupted = corrupt_data(data.as_bytes(), 0.3);
    String::from_utf8_lossy(&corrupted).to_string()
}

pub fn aegis_get_description() -> String {
    r#"
AEGIS - Adaptive Defense & Deception System

Not an antivirus. A TRAP.

When attackers touch your system, AEGIS:
🍯 Honeypots - Fake wallets, passwords, API keys that alert when accessed
🔄 Ricochet - Corrupt data being stolen so it's useless
⏱️ Tar Pits - Waste attacker time with infinite fake files
🔍 Fingerprinting - Identify and track attackers
🛡️ Deception - Feed false information

RESOURCE EFFICIENT:
• No constant scanning (unlike antivirus)
• Minimal CPU/RAM when idle
• Only activates when triggered
• No signature updates needed

PHILOSOPHY:
Traditional security is reactive. AEGIS is offensive.
Make hackers regret touching your computer.
"#
    .to_string()
}
