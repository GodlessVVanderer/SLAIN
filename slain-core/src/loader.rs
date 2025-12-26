//! Self-Contained Library Loader
//!
//! Loads all DLLs from explicit paths, never relying on PATH.
//! This avoids issues with System32 stub files and broken installs.

use std::path::{Path, PathBuf};
use std::env;

/// Known locations for NVIDIA driver libraries
const NVIDIA_PATHS: &[&str] = &[
    "C:\\Windows\\System32",
    "C:\\Windows\\SysWOW64",
    "C:\\Program Files\\NVIDIA Corporation\\NVSMI",
];

/// Known locations for Vulkan
const VULKAN_PATHS: &[&str] = &[
    "C:\\Windows\\System32",
    "C:\\Windows\\SysWOW64",
];

/// Get the directory where our executable lives
pub fn exe_dir() -> PathBuf {
    env::current_exe()
        .expect("Failed to get executable path")
        .parent()
        .expect("Executable has no parent directory")
        .to_path_buf()
}

/// Get the app's bundled libraries directory
pub fn lib_dir() -> PathBuf {
    exe_dir() // Libraries are next to the exe
}

/// Library loading result
#[derive(Debug)]
pub struct LoadedLibrary {
    pub path: PathBuf,
    pub library: libloading::Library,
}

/// Load a library from our app directory (bundled DLLs)
/// 
/// This is for FFmpeg and other libraries we ship with the app.
pub fn load_bundled(name: &str) -> Result<LoadedLibrary, String> {
    let path = lib_dir().join(name);
    
    if !path.exists() {
        return Err(format!(
            "Bundled library not found: {}\nExpected at: {}",
            name, path.display()
        ));
    }
    
    // Check it's not a stub (> 10KB for real DLLs)
    let metadata = std::fs::metadata(&path)
        .map_err(|e| format!("Failed to read {}: {}", name, e))?;
    
    if metadata.len() < 10_000 {
        return Err(format!(
            "Bundled library {} appears to be a stub ({} bytes). Reinstall SLAIN.",
            name, metadata.len()
        ));
    }
    
    let library = unsafe {
        libloading::Library::new(&path)
            .map_err(|e| format!("Failed to load {}: {}", path.display(), e))?
    };
    
    Ok(LoadedLibrary { path, library })
}

/// Load an NVIDIA driver library from known driver locations
/// 
/// This is for CUDA, NVOF, NVENC, etc. - libraries installed by NVIDIA drivers.
pub fn load_nvidia(name: &str) -> Result<LoadedLibrary, String> {
    for base in NVIDIA_PATHS {
        let path = Path::new(base).join(name);
        
        if path.exists() {
            // Verify it's not a stub
            if let Ok(metadata) = std::fs::metadata(&path) {
                if metadata.len() < 10_000 {
                    continue; // Skip stubs
                }
            }
            
            let library = unsafe {
                match libloading::Library::new(&path) {
                    Ok(lib) => lib,
                    Err(_) => continue,
                }
            };
            
            return Ok(LoadedLibrary { path, library });
        }
    }
    
    Err(format!(
        "NVIDIA library {} not found. Make sure NVIDIA drivers are installed.\n\
         Searched: {:?}",
        name, NVIDIA_PATHS
    ))
}

/// Load Vulkan library from known locations
pub fn load_vulkan() -> Result<LoadedLibrary, String> {
    let name = "vulkan-1.dll";
    
    for base in VULKAN_PATHS {
        let path = Path::new(base).join(name);
        
        if path.exists() {
            if let Ok(metadata) = std::fs::metadata(&path) {
                if metadata.len() < 10_000 {
                    continue;
                }
            }
            
            let library = unsafe {
                match libloading::Library::new(&path) {
                    Ok(lib) => lib,
                    Err(_) => continue,
                }
            };
            
            return Ok(LoadedLibrary { path, library });
        }
    }
    
    Err(format!(
        "Vulkan not found. Make sure GPU drivers are installed.\n\
         Searched: {:?}",
        VULKAN_PATHS
    ))
}

/// FFmpeg libraries we bundle with the app
pub struct FFmpegLibs {
    pub avcodec: LoadedLibrary,
    pub avformat: LoadedLibrary,
    pub avutil: LoadedLibrary,
    pub swresample: LoadedLibrary,
    pub swscale: LoadedLibrary,
}

impl FFmpegLibs {
    /// Load all FFmpeg libraries from our bundle
    pub fn load() -> Result<Self, String> {
        Ok(Self {
            avcodec: load_bundled("avcodec-61.dll")?,
            avformat: load_bundled("avformat-61.dll")?,
            avutil: load_bundled("avutil-59.dll")?,
            swresample: load_bundled("swresample-5.dll")?,
            swscale: load_bundled("swscale-8.dll")?,
        })
    }
}

/// NVIDIA libraries from driver install
pub struct NvidiaLibs {
    pub cuda: Option<LoadedLibrary>,
    pub nvof: Option<LoadedLibrary>,
    pub nvenc: Option<LoadedLibrary>,
    pub nvdec: Option<LoadedLibrary>,
}

impl NvidiaLibs {
    /// Try to load NVIDIA libraries (all optional)
    pub fn load() -> Self {
        Self {
            cuda: load_nvidia("nvcuda.dll").ok(),
            nvof: load_nvidia("nvofapi64.dll").ok(),
            nvenc: load_nvidia("nvEncodeAPI64.dll").ok(),
            nvdec: load_nvidia("nvcuvid.dll").ok(),
        }
    }
    
    /// Check what's available
    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        
        if let Some(ref lib) = self.cuda {
            lines.push(format!("CUDA: {}", lib.path.display()));
        } else {
            lines.push("CUDA: Not found".to_string());
        }
        
        if let Some(ref lib) = self.nvof {
            lines.push(format!("NVOF: {}", lib.path.display()));
        } else {
            lines.push("NVOF: Not found (Turing+ only)".to_string());
        }
        
        if let Some(ref lib) = self.nvenc {
            lines.push(format!("NVENC: {}", lib.path.display()));
        } else {
            lines.push("NVENC: Not found".to_string());
        }
        
        if let Some(ref lib) = self.nvdec {
            lines.push(format!("NVDEC: {}", lib.path.display()));
        } else {
            lines.push("NVDEC: Not found".to_string());
        }
        
        lines.join("\n")
    }
}

// ============================================================================
// Public Rust API
// ============================================================================

use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryStatus {
    pub name: String,
    pub found: bool,
    pub path: Option<String>,
    pub size_kb: Option<u64>,
}


pub fn check_bundled_libraries() -> Vec<LibraryStatus> {
    let libs = [
        "avcodec-61.dll",
        "avformat-61.dll",
        "avutil-59.dll",
        "swresample-5.dll",
        "swscale-8.dll",
    ];
    
    libs.iter().map(|name| {
        let path = lib_dir().join(name);
        let (found, size_kb) = if path.exists() {
            let size = std::fs::metadata(&path)
                .map(|m| m.len() / 1024)
                .unwrap_or(0);
            (true, Some(size))
        } else {
            (false, None)
        };
        
        LibraryStatus {
            name: name.to_string(),
            found,
            path: if found { Some(path.to_string_lossy().to_string()) } else { None },
            size_kb,
        }
    }).collect()
}


pub fn check_nvidia_libraries() -> Vec<LibraryStatus> {
    let libs = [
        ("nvcuda.dll", "CUDA Runtime"),
        ("nvofapi64.dll", "Optical Flow (Turing+)"),
        ("nvEncodeAPI64.dll", "Hardware Encoder"),
        ("nvcuvid.dll", "Hardware Decoder"),
    ];
    
    libs.iter().map(|(name, _desc)| {
        match load_nvidia(name) {
            Ok(lib) => {
                let size = std::fs::metadata(&lib.path)
                    .map(|m| m.len() / 1024)
                    .unwrap_or(0);
                LibraryStatus {
                    name: name.to_string(),
                    found: true,
                    path: Some(lib.path.to_string_lossy().to_string()),
                    size_kb: Some(size),
                }
            }
            Err(_) => LibraryStatus {
                name: name.to_string(),
                found: false,
                path: None,
                size_kb: None,
            }
        }
    }).collect()
}


pub fn check_vulkan() -> LibraryStatus {
    match load_vulkan() {
        Ok(lib) => {
            let size = std::fs::metadata(&lib.path)
                .map(|m| m.len() / 1024)
                .unwrap_or(0);
            LibraryStatus {
                name: "vulkan-1.dll".to_string(),
                found: true,
                path: Some(lib.path.to_string_lossy().to_string()),
                size_kb: Some(size),
            }
        }
        Err(_) => LibraryStatus {
            name: "vulkan-1.dll".to_string(),
            found: false,
            path: None,
            size_kb: None,
        }
    }
}


pub fn get_library_report() -> String {
    let mut report = String::new();
    
    report.push_str("=== SLAIN Library Status ===\n\n");
    
    report.push_str("Bundled Libraries:\n");
    for lib in check_bundled_libraries() {
        if lib.found {
            report.push_str(&format!("  [OK] {} ({} KB)\n", lib.name, lib.size_kb.unwrap_or(0)));
        } else {
            report.push_str(&format!("  [MISSING] {}\n", lib.name));
        }
    }
    
    report.push_str("\nNVIDIA Libraries:\n");
    for lib in check_nvidia_libraries() {
        if lib.found {
            report.push_str(&format!("  [OK] {} ({} KB)\n", lib.name, lib.size_kb.unwrap_or(0)));
        } else {
            report.push_str(&format!("  [ ] {} (not found)\n", lib.name));
        }
    }
    
    report.push_str("\nVulkan:\n");
    let vulkan = check_vulkan();
    if vulkan.found {
        report.push_str(&format!("  [OK] {} ({} KB)\n", vulkan.name, vulkan.size_kb.unwrap_or(0)));
    } else {
        report.push_str(&format!("  [MISSING] {}\n", vulkan.name));
    }
    
    report
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_exe_dir() {
        let dir = exe_dir();
        assert!(dir.exists());
        println!("Exe dir: {}", dir.display());
    }
    
    #[test]
    fn test_nvidia_detection() {
        let libs = NvidiaLibs::load();
        println!("{}", libs.report());
    }
}
