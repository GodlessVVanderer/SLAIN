//! # SLAIN Security Audit Report
//!
//! Audit Date: 2024-12-24
//! Auditor: Claude Code Security Review
//! Scope: Full codebase security analysis
//!
//! ## Executive Summary
//!
//! SLAIN is a GPU-accelerated video player written in pure Rust. The codebase
//! demonstrates good Rust practices overall, but several security concerns
//! were identified that should be addressed.

use serde::{Deserialize, Serialize};

// ============================================================================
// Audit Finding Structures
// ============================================================================

/// Severity level of a security finding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Critical - Immediate exploitation possible
    Critical,
    /// High - Significant security risk
    High,
    /// Medium - Moderate security concern
    Medium,
    /// Low - Minor issue or informational
    Low,
    /// Info - Best practice recommendation
    Info,
}

/// Category of security finding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Category {
    /// DLL/Library hijacking vulnerabilities
    DllHijacking,
    /// Server-Side Request Forgery
    Ssrf,
    /// Memory safety issues
    MemorySafety,
    /// Input validation issues
    InputValidation,
    /// Authentication/Authorization
    AuthZ,
    /// Cryptography issues
    Crypto,
    /// Path traversal
    PathTraversal,
    /// Information disclosure
    InfoDisclosure,
    /// Denial of Service
    DoS,
    /// Unsafe code review
    UnsafeCode,
}

/// A security audit finding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: &'static str,
    pub title: &'static str,
    pub severity: Severity,
    pub category: Category,
    pub file: &'static str,
    pub lines: &'static str,
    pub description: &'static str,
    pub impact: &'static str,
    pub recommendation: &'static str,
    pub status: FindingStatus,
}

/// Status of a finding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindingStatus {
    /// Not yet addressed
    Open,
    /// Fix in progress
    InProgress,
    /// Fixed and verified
    Resolved,
    /// Accepted risk
    AcceptedRisk,
    /// Not applicable
    NotApplicable,
}

// ============================================================================
// Audit Findings
// ============================================================================

/// All security findings from this audit
pub const FINDINGS: &[Finding] = &[
    // -------------------------------------------------------------------------
    // HIGH SEVERITY
    // -------------------------------------------------------------------------
    Finding {
        id: "SLAIN-001",
        title: "DLL Hijacking via Dynamic Library Loading",
        severity: Severity::High,
        category: Category::DllHijacking,
        file: "gpu.rs",
        lines: "382-383, 774-775",
        description: "GPU libraries (nvapi64.dll, atiadlxx.dll) are loaded using \
            relative paths via libloading::Library::new(). On Windows, DLLs are \
            searched in the current working directory before system paths.",
        impact: "An attacker who can place a malicious DLL in the application's \
            working directory can achieve arbitrary code execution with the \
            privileges of the user running SLAIN.",
        recommendation: "Use absolute paths for library loading. On Windows, use \
            GetSystemDirectory() or similar to construct full paths. Consider \
            using SetDllDirectory(\"\") to remove CWD from search path.",
        status: FindingStatus::Open,
    },
    Finding {
        id: "SLAIN-002",
        title: "DLL Hijacking in NVDEC Decoder",
        severity: Severity::High,
        category: Category::DllHijacking,
        file: "nvdec.rs",
        lines: "347-356",
        description: "CUDA libraries (nvcuda.dll, nvcuvid.dll) loaded without \
            full paths. Same vulnerability pattern as SLAIN-001.",
        impact: "Code execution via malicious DLL placement.",
        recommendation: "Load from System32 or CUDA installation directory only.",
        status: FindingStatus::Open,
    },
    Finding {
        id: "SLAIN-003",
        title: "DLL Hijacking in Pipeline System",
        severity: Severity::High,
        category: Category::DllHijacking,
        file: "pipeline.rs",
        lines: "165, 232, 381",
        description: "AviSynth.dll, vsscript.dll, and nvcuda.dll loaded via \
            relative paths in the pipeline initialization code.",
        impact: "Code execution if attacker controls working directory.",
        recommendation: "Resolve library paths from known installation locations.",
        status: FindingStatus::Open,
    },
    Finding {
        id: "SLAIN-004",
        title: "SSRF via UPnP/DLNA Discovery",
        severity: Severity::High,
        category: Category::Ssrf,
        file: "streaming.rs",
        lines: "212",
        description: "The DLNA discovery code fetches device descriptions from \
            URLs provided in SSDP LOCATION headers without validation. An attacker \
            on the local network can inject malicious SSDP responses.",
        impact: "Attacker can force SLAIN to make HTTP requests to arbitrary \
            internal services, potentially accessing sensitive endpoints or \
            triggering actions on internal systems.",
        recommendation: "Validate LOCATION URLs: check scheme (http/https only), \
            validate host is on local network, implement timeout and size limits.",
        status: FindingStatus::Open,
    },
    Finding {
        id: "SLAIN-005",
        title: "SSRF via IPTV Playlist URLs",
        severity: Severity::High,
        category: Category::Ssrf,
        file: "iptv.rs",
        lines: "246",
        description: "M3U/M3U8 playlists can contain arbitrary URLs that are \
            fetched without validation. A malicious playlist can trigger \
            requests to internal services.",
        impact: "Internal service access, potential data exfiltration via \
            DNS or timing side channels.",
        recommendation: "Implement URL allowlist, block private IP ranges \
            (10.x, 172.16-31.x, 192.168.x, 127.x, ::1), add request timeouts.",
        status: FindingStatus::Open,
    },

    // -------------------------------------------------------------------------
    // MEDIUM SEVERITY
    // -------------------------------------------------------------------------
    Finding {
        id: "SLAIN-006",
        title: "Memory Exhaustion via Malformed Container Files",
        severity: Severity::Medium,
        category: Category::DoS,
        file: "mkv.rs",
        lines: "1267",
        description: "Attachment extraction allocates a Vec based on size read \
            from the file: `vec![0u8; attachment.size as usize]`. A malformed \
            MKV claiming a multi-gigabyte attachment could exhaust memory.",
        impact: "Denial of service via memory exhaustion when opening \
            maliciously crafted media files.",
        recommendation: "Implement maximum size limits for allocations. Validate \
            claimed sizes against actual file size. Use streaming reads for \
            large data.",
        status: FindingStatus::Open,
    },
    Finding {
        id: "SLAIN-007",
        title: "Unsafe FFI Code in GPU Interfaces",
        severity: Severity::Medium,
        category: Category::UnsafeCode,
        file: "gpu.rs",
        lines: "401-434, 762",
        description: "Extensive unsafe code for GPU API FFI. Function pointer \
            transmutes could cause undefined behavior if API versions mismatch. \
            Memory allocation callbacks use raw allocation.",
        impact: "Potential crashes or undefined behavior with mismatched \
            GPU driver versions.",
        recommendation: "Add version checks before loading GPU APIs. Consider \
            using safer FFI wrappers. Document minimum driver versions.",
        status: FindingStatus::Open,
    },
    Finding {
        id: "SLAIN-008",
        title: "No Authentication on MCP Server",
        severity: Severity::Medium,
        category: Category::AuthZ,
        file: "slain-mcp/src/main.rs",
        lines: "entire file",
        description: "The MCP server exposes powerful capabilities (GPU control, \
            file access, system queries) without any authentication mechanism.",
        impact: "Any process with access to the MCP server's stdin/stdout can \
            control GPU settings, open files, and query system information.",
        recommendation: "By design for MCP, but document security implications. \
            Consider optional authentication for sensitive operations.",
        status: FindingStatus::AcceptedRisk,
    },
    Finding {
        id: "SLAIN-009",
        title: "Protocol Handler Allows Arbitrary File Access",
        severity: Severity::Medium,
        category: Category::InputValidation,
        file: "protocol.rs",
        lines: "66-69",
        description: "The slain:// protocol handler can open arbitrary local \
            files via slain://open?file=/path/to/file URLs.",
        impact: "If an attacker can get a user to click a crafted slain:// \
            link, they could open any file the user has access to.",
        recommendation: "Implement file type validation, consider prompting \
            user for confirmation on external protocol invocations.",
        status: FindingStatus::Open,
    },

    // -------------------------------------------------------------------------
    // LOW SEVERITY / INFORMATIONAL
    // -------------------------------------------------------------------------
    Finding {
        id: "SLAIN-010",
        title: "API Keys Stored in Memory",
        severity: Severity::Low,
        category: Category::InfoDisclosure,
        file: "debrid.rs",
        lines: "global state",
        description: "Debrid service API keys are stored in global RwLock state. \
            Keys are transmitted over HTTPS which is appropriate.",
        impact: "Memory dumps could expose API keys. This is standard practice \
            for API key handling.",
        recommendation: "Consider using secure memory (mlock) for sensitive data. \
            Document that users should use app-specific API keys.",
        status: FindingStatus::AcceptedRisk,
    },
    Finding {
        id: "SLAIN-011",
        title: "Path Traversal Protection Present",
        severity: Severity::Info,
        category: Category::PathTraversal,
        file: "archive.rs",
        lines: "209-212, 447-450",
        description: "Archive extraction properly validates output paths using \
            starts_with() to prevent path traversal attacks.",
        impact: "None - this is a POSITIVE finding.",
        recommendation: "No action needed. Good implementation.",
        status: FindingStatus::Resolved,
    },
    Finding {
        id: "SLAIN-012",
        title: "AEGIS Module Security Review",
        severity: Severity::Info,
        category: Category::InfoDisclosure,
        file: "aegis.rs",
        lines: "entire file",
        description: "AEGIS is a defensive security module implementing honeypots \
            and deception. It generates fake credentials and can corrupt \
            exfiltrated data. This is INTENTIONAL security functionality.",
        impact: "None - this is defensive security code, not malware.",
        recommendation: "Document AEGIS capabilities clearly. Ensure users \
            understand the deception features.",
        status: FindingStatus::NotApplicable,
    },
];

// ============================================================================
// Audit Statistics
// ============================================================================

/// Get count of findings by severity
pub fn count_by_severity(severity: Severity) -> usize {
    FINDINGS.iter().filter(|f| f.severity == severity).count()
}

/// Get count of open findings
pub fn count_open() -> usize {
    FINDINGS.iter().filter(|f| f.status == FindingStatus::Open).count()
}

/// Get all findings for a specific file
pub fn findings_for_file(file: &str) -> Vec<&Finding> {
    FINDINGS.iter().filter(|f| f.file == file).collect()
}

/// Generate audit summary
pub fn audit_summary() -> AuditSummary {
    AuditSummary {
        total_findings: FINDINGS.len(),
        critical: count_by_severity(Severity::Critical),
        high: count_by_severity(Severity::High),
        medium: count_by_severity(Severity::Medium),
        low: count_by_severity(Severity::Low),
        info: count_by_severity(Severity::Info),
        open: count_open(),
    }
}

/// Audit summary statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditSummary {
    pub total_findings: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
    pub open: usize,
}

// ============================================================================
// Positive Security Observations
// ============================================================================

/// Security strengths identified in the codebase
pub const POSITIVE_OBSERVATIONS: &[&str] = &[
    "Written in Rust - memory safety by default",
    "Uses HTTPS for all external API communications (debrid.rs)",
    "Path traversal protection in archive extraction (archive.rs)",
    "No SQL - eliminates SQL injection risk",
    "No shell command execution with user input - no command injection",
    "Good error handling with thiserror and anyhow",
    "Uses parking_lot for efficient, safe synchronization",
    "Streaming/buffered file processing reduces memory attack surface",
    "Clear separation of concerns in module structure",
    "No hardcoded credentials in source code",
];

// ============================================================================
// Recommended Fixes
// ============================================================================

/// Helper to get system library path (for DLL hijacking fix)
#[cfg(windows)]
pub fn get_system_library_path(lib_name: &str) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;

    // Try System32 first
    if let Ok(system_root) = std::env::var("SystemRoot") {
        let system32 = PathBuf::from(&system_root).join("System32").join(lib_name);
        if system32.exists() {
            return Some(system32);
        }
    }

    // Try CUDA installation
    if lib_name.contains("cuda") || lib_name.contains("nv") {
        if let Ok(cuda_path) = std::env::var("CUDA_PATH") {
            let cuda_lib = PathBuf::from(&cuda_path).join("bin").join(lib_name);
            if cuda_lib.exists() {
                return Some(cuda_lib);
            }
        }
    }

    None
}

#[cfg(not(windows))]
pub fn get_system_library_path(_lib_name: &str) -> Option<std::path::PathBuf> {
    // On Linux/macOS, library search paths are more secure by default
    None
}

/// Validate URL for SSRF prevention
pub fn is_safe_url(url: &str) -> bool {
    use std::net::IpAddr;

    let parsed = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return false,
    };

    // Only allow http/https
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }

    // Check if host is a private IP
    if let Some(host) = parsed.host_str() {
        if let Ok(ip) = host.parse::<IpAddr>() {
            match ip {
                IpAddr::V4(v4) => {
                    // Block private ranges
                    if v4.is_private() || v4.is_loopback() || v4.is_link_local() {
                        return false;
                    }
                }
                IpAddr::V6(v6) => {
                    if v6.is_loopback() {
                        return false;
                    }
                }
            }
        }
    }

    true
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_summary() {
        let summary = audit_summary();
        assert!(summary.total_findings > 0);
        assert!(summary.high >= 3); // We have at least 3 high severity findings
    }

    #[test]
    fn test_ssrf_validation() {
        // Safe URLs
        assert!(is_safe_url("https://example.com/video.mp4"));
        assert!(is_safe_url("http://cdn.example.com/stream"));

        // Unsafe URLs - private IPs
        assert!(!is_safe_url("http://192.168.1.1/admin"));
        assert!(!is_safe_url("http://10.0.0.1/internal"));
        assert!(!is_safe_url("http://127.0.0.1:8080/secret"));
        assert!(!is_safe_url("http://[::1]/localhost"));

        // Unsafe URLs - bad schemes
        assert!(!is_safe_url("file:///etc/passwd"));
        assert!(!is_safe_url("ftp://example.com/file"));
    }

    #[test]
    fn test_findings_for_file() {
        let gpu_findings = findings_for_file("gpu.rs");
        assert!(!gpu_findings.is_empty());
    }
}
