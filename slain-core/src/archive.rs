// Pure Rust Archive Handling
// ZIP, TAR, GZIP support - no C dependencies

use std::fs::{self, File};
use std::io::{self, Read, Write, BufReader, BufWriter, Seek};
use std::path::{Path, PathBuf};

use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use serde::{Deserialize, Serialize};


// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveInfo {
    pub path: String,
    pub format: ArchiveFormat,
    pub total_size: u64,
    pub compressed_size: u64,
    pub file_count: usize,
    pub entries: Vec<ArchiveEntry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveFormat {
    Zip,
    Tar,
    TarGz,
    Gz,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub compressed_size: Option<u64>,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub modified: Option<String>,
    pub permissions: Option<u32>,
    pub compression_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionProgress {
    pub current_file: String,
    pub files_done: usize,
    pub files_total: usize,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionOptions {
    pub level: u32,           // 0-9, where 9 is max compression
    pub include_hidden: bool,
    pub preserve_permissions: bool,
    pub follow_symlinks: bool,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self {
            level: 6,
            include_hidden: false,
            preserve_permissions: true,
            follow_symlinks: false,
        }
    }
}

// ============================================================================
// Format Detection
// ============================================================================

pub fn detect_archive_format<P: AsRef<Path>>(path: P) -> ArchiveFormat {
    let path = path.as_ref();
    
    // Check extension first
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase());
    
    match ext.as_deref() {
        Some("zip") => ArchiveFormat::Zip,
        Some("tar") => ArchiveFormat::Tar,
        Some("gz") | Some("gzip") => {
            // Check if it's .tar.gz
            let stem = path.file_stem().and_then(|s| s.to_str());
            if let Some(s) = stem {
                if s.ends_with(".tar") {
                    return ArchiveFormat::TarGz;
                }
            }
            ArchiveFormat::Gz
        }
        Some("tgz") => ArchiveFormat::TarGz,
        _ => {
            // Try magic bytes
            if let Ok(mut file) = File::open(path) {
                let mut magic = [0u8; 4];
                if file.read_exact(&mut magic).is_ok() {
                    // ZIP magic: PK\x03\x04
                    if magic[0..2] == [0x50, 0x4B] {
                        return ArchiveFormat::Zip;
                    }
                    // GZIP magic: \x1f\x8b
                    if magic[0..2] == [0x1F, 0x8B] {
                        return ArchiveFormat::Gz; // or TarGz
                    }
                }
            }
            ArchiveFormat::Unknown
        }
    }
}

// ============================================================================
// ZIP Operations
// ============================================================================

pub fn list_zip<P: AsRef<Path>>(path: P) -> Result<ArchiveInfo, String> {
    let path = path.as_ref();
    let file = File::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read ZIP: {}", e))?;
    
    let mut entries = Vec::new();
    let mut total_size: u64 = 0;
    let mut compressed_size: u64 = 0;
    
    for i in 0..archive.len() {
        let file = archive.by_index(i)
            .map_err(|e| format!("Failed to read entry {}: {}", i, e))?;
        
        let entry = ArchiveEntry {
            name: file.name().split('/').last().unwrap_or("").to_string(),
            path: file.name().to_string(),
            size: file.size(),
            compressed_size: Some(file.compressed_size()),
            is_dir: file.is_dir(),
            is_symlink: file.is_symlink(),
            modified: file.last_modified().map(|dt| {
                format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                    dt.year(), dt.month(), dt.day(),
                    dt.hour(), dt.minute(), dt.second())
            }),
            permissions: file.unix_mode(),
            compression_method: Some(format!("{:?}", file.compression())),
        };
        
        total_size += file.size();
        compressed_size += file.compressed_size();
        entries.push(entry);
    }
    
    let file_count = entries.iter().filter(|e| !e.is_dir).count();
    
    Ok(ArchiveInfo {
        path: path.to_string_lossy().to_string(),
        format: ArchiveFormat::Zip,
        total_size,
        compressed_size,
        file_count,
        entries,
    })
}

pub fn extract_zip<P: AsRef<Path>>(
    archive_path: P,
    output_dir: P,
    entries: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let archive_path = archive_path.as_ref();
    let output_dir = output_dir.as_ref();
    
    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output dir: {}", e))?;
    
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed to open archive: {}", e))?;
    
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Failed to read ZIP: {}", e))?;
    
    let mut extracted = Vec::new();
    
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)
            .map_err(|e| format!("Failed to read entry: {}", e))?;
        
        // Filter entries if specified
        if let Some(ref filter) = entries {
            if !filter.contains(&file.name().to_string()) {
                continue;
            }
        }
        
        let out_path = output_dir.join(file.name());
        
        // Security: prevent path traversal
        if !out_path.starts_with(output_dir) {
            continue;
        }
        
        if file.is_dir() {
            fs::create_dir_all(&out_path)
                .map_err(|e| format!("Failed to create dir: {}", e))?;
        } else {
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create parent dir: {}", e))?;
            }
            
            let mut out_file = File::create(&out_path)
                .map_err(|e| format!("Failed to create file: {}", e))?;
            
            io::copy(&mut file, &mut out_file)
                .map_err(|e| format!("Failed to extract file: {}", e))?;
            
            // Restore permissions on Unix
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    fs::set_permissions(&out_path, fs::Permissions::from_mode(mode)).ok();
                }
            }
            
            extracted.push(out_path.to_string_lossy().to_string());
        }
    }
    
    Ok(extracted)
}

pub fn create_zip<P: AsRef<Path>>(
    source_paths: Vec<P>,
    output_path: P,
    options: CompressionOptions,
) -> Result<u64, String> {
    let output_path = output_path.as_ref();
    
    let file = File::create(output_path)
        .map_err(|e| format!("Failed to create output file: {}", e))?;
    
    let mut zip = zip::ZipWriter::new(file);
    
    let compression = match options.level {
        0 => zip::CompressionMethod::Stored,
        _ => zip::CompressionMethod::Deflated,
    };
    
    let zip_options = zip::write::SimpleFileOptions::default()
        .compression_method(compression)
        .compression_level(Some(options.level as i64));
    
    let mut total_written: u64 = 0;
    
    for source in source_paths {
        let source = source.as_ref();
        
        if source.is_dir() {
            add_dir_to_zip(&mut zip, source, source, &zip_options, &options, &mut total_written)?;
        } else if source.is_file() {
            add_file_to_zip(&mut zip, source, source.file_name().unwrap().to_str().unwrap(), &zip_options, &mut total_written)?;
        }
    }
    
    zip.finish().map_err(|e| format!("Failed to finalize ZIP: {}", e))?;
    
    Ok(total_written)
}

fn add_dir_to_zip<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    dir: &Path,
    base: &Path,
    options: &zip::write::SimpleFileOptions,
    comp_options: &CompressionOptions,
    total: &mut u64,
) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("Failed to read dir: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let path = entry.path();
        
        // Skip hidden files unless requested
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !comp_options.include_hidden && name.starts_with('.') {
            continue;
        }
        
        let relative = path.strip_prefix(base)
            .map_err(|_| "Failed to get relative path")?;
        
        if path.is_dir() {
            let dir_name = format!("{}/", relative.to_string_lossy());
            zip.add_directory(&dir_name, *options)
                .map_err(|e| format!("Failed to add dir: {}", e))?;
            
            add_dir_to_zip(zip, &path, base, options, comp_options, total)?;
        } else {
            add_file_to_zip(zip, &path, &relative.to_string_lossy(), options, total)?;
        }
    }
    
    Ok(())
}

fn add_file_to_zip<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    path: &Path,
    name: &str,
    options: &zip::write::SimpleFileOptions,
    total: &mut u64,
) -> Result<(), String> {
    let mut file = File::open(path)
        .map_err(|e| format!("Failed to open {}: {}", name, e))?;
    
    zip.start_file(name, *options)
        .map_err(|e| format!("Failed to start file: {}", e))?;
    
    let written = io::copy(&mut file, zip)
        .map_err(|e| format!("Failed to write file: {}", e))?;
    
    *total += written;
    
    Ok(())
}

// ============================================================================
// TAR Operations
// ============================================================================

pub fn list_tar<P: AsRef<Path>>(path: P) -> Result<ArchiveInfo, String> {
    let path = path.as_ref();
    let file = File::open(path)
        .map_err(|e| format!("Failed to open file: {}", e))?;
    
    let format = detect_archive_format(path);
    
    let reader: Box<dyn Read> = match format {
        ArchiveFormat::TarGz | ArchiveFormat::Gz => {
            Box::new(GzDecoder::new(file))
        }
        _ => Box::new(file),
    };
    
    let mut archive = tar::Archive::new(reader);
    let mut entries_list = Vec::new();
    let mut total_size: u64 = 0;
    
    for entry in archive.entries().map_err(|e| format!("Failed to read TAR: {}", e))? {
        let entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        let header = entry.header();
        
        let path_str = entry.path()
            .map_err(|e| format!("Failed to read path: {}", e))?
            .to_string_lossy()
            .to_string();
        
        let name = path_str.split('/').last().unwrap_or("").to_string();
        let size = header.size().unwrap_or(0);
        
        let archive_entry = ArchiveEntry {
            name,
            path: path_str,
            size,
            compressed_size: None,
            is_dir: header.entry_type().is_dir(),
            is_symlink: header.entry_type().is_symlink(),
            modified: header.mtime().ok().map(|t| {
                // Convert Unix timestamp to string
                let secs = t as i64;
                format!("{}", secs)
            }),
            permissions: header.mode().ok(),
            compression_method: None,
        };
        
        total_size += size;
        entries_list.push(archive_entry);
    }
    
    let file_count = entries_list.iter().filter(|e| !e.is_dir).count();
    let compressed_size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    
    Ok(ArchiveInfo {
        path: path.to_string_lossy().to_string(),
        format,
        total_size,
        compressed_size,
        file_count,
        entries: entries_list,
    })
}

pub fn extract_tar<P: AsRef<Path>>(
    archive_path: P,
    output_dir: P,
    entries: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let archive_path = archive_path.as_ref();
    let output_dir = output_dir.as_ref();
    
    fs::create_dir_all(output_dir)
        .map_err(|e| format!("Failed to create output dir: {}", e))?;
    
    let file = File::open(archive_path)
        .map_err(|e| format!("Failed to open archive: {}", e))?;
    
    let format = detect_archive_format(archive_path);
    
    let reader: Box<dyn Read> = match format {
        ArchiveFormat::TarGz | ArchiveFormat::Gz => {
            Box::new(GzDecoder::new(file))
        }
        _ => Box::new(file),
    };
    
    let mut archive = tar::Archive::new(reader);
    let mut extracted = Vec::new();
    
    for entry in archive.entries().map_err(|e| format!("Failed to read TAR: {}", e))? {
        let mut entry = entry.map_err(|e| format!("Failed to read entry: {}", e))?;
        
        let path = entry.path()
            .map_err(|e| format!("Failed to read path: {}", e))?;
        
        // Filter entries if specified
        if let Some(ref filter) = entries {
            if !filter.contains(&path.to_string_lossy().to_string()) {
                continue;
            }
        }
        
        let out_path = output_dir.join(&path);
        
        // Security: prevent path traversal
        if !out_path.starts_with(output_dir) {
            continue;
        }
        
        entry.unpack(&out_path)
            .map_err(|e| format!("Failed to extract {}: {}", path.display(), e))?;
        
        extracted.push(out_path.to_string_lossy().to_string());
    }
    
    Ok(extracted)
}

pub fn create_tar_gz<P: AsRef<Path>>(
    source_paths: Vec<P>,
    output_path: P,
    options: CompressionOptions,
) -> Result<u64, String> {
    let output_path = output_path.as_ref();
    
    let file = File::create(output_path)
        .map_err(|e| format!("Failed to create output file: {}", e))?;
    
    let level = match options.level {
        0 => Compression::none(),
        1..=3 => Compression::fast(),
        4..=6 => Compression::default(),
        _ => Compression::best(),
    };
    
    let encoder = GzEncoder::new(BufWriter::new(file), level);
    let mut tar = tar::Builder::new(encoder);
    
    for source in source_paths {
        let source = source.as_ref();
        
        if source.is_dir() {
            tar.append_dir_all(source.file_name().unwrap(), source)
                .map_err(|e| format!("Failed to add dir: {}", e))?;
        } else if source.is_file() {
            let mut file = File::open(source)
                .map_err(|e| format!("Failed to open file: {}", e))?;
            tar.append_file(source.file_name().unwrap(), &mut file)
                .map_err(|e| format!("Failed to add file: {}", e))?;
        }
    }
    
    let encoder = tar.into_inner()
        .map_err(|e| format!("Failed to finalize TAR: {}", e))?;
    
    let writer = encoder.finish()
        .map_err(|e| format!("Failed to finalize GZIP: {}", e))?;
    
    writer.into_inner()
        .map_err(|e| format!("Failed to flush: {}", e))?;
    
    let size = fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
    Ok(size)
}

// ============================================================================
// GZIP Operations (single file)
// ============================================================================

pub fn compress_gzip<P: AsRef<Path>, Q: AsRef<Path>>(
    input_path: P,
    output_path: Option<Q>,
    level: u32,
) -> Result<u64, String> {
    let input_path = input_path.as_ref();
    let output_path = output_path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| {
            let mut p = input_path.to_path_buf();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            p.set_file_name(format!("{}.gz", name));
            p
        });
    
    let input = File::open(input_path)
        .map_err(|e| format!("Failed to open input: {}", e))?;
    let mut input = BufReader::new(input);
    
    let output = File::create(&output_path)
        .map_err(|e| format!("Failed to create output: {}", e))?;
    
    let compression = match level {
        0 => Compression::none(),
        1..=3 => Compression::fast(),
        4..=6 => Compression::default(),
        _ => Compression::best(),
    };
    
    let mut encoder = GzEncoder::new(BufWriter::new(output), compression);
    
    io::copy(&mut input, &mut encoder)
        .map_err(|e| format!("Failed to compress: {}", e))?;
    
    encoder.finish()
        .map_err(|e| format!("Failed to finalize: {}", e))?;
    
    let size = fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
    Ok(size)
}

pub fn decompress_gzip<P: AsRef<Path>, Q: AsRef<Path>>(
    input_path: P,
    output_path: Option<Q>,
) -> Result<u64, String> {
    let input_path = input_path.as_ref();
    let output_path = output_path
        .map(|p| p.as_ref().to_path_buf())
        .unwrap_or_else(|| {
            let mut p = input_path.to_path_buf();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            if name.ends_with(".gz") {
                p.set_file_name(&name[..name.len()-3]);
            } else {
                p.set_file_name(format!("{}.out", name));
            }
            p
        });
    
    let input = File::open(input_path)
        .map_err(|e| format!("Failed to open input: {}", e))?;
    
    let mut decoder = GzDecoder::new(BufReader::new(input));
    
    let output = File::create(&output_path)
        .map_err(|e| format!("Failed to create output: {}", e))?;
    let mut output = BufWriter::new(output);
    
    io::copy(&mut decoder, &mut output)
        .map_err(|e| format!("Failed to decompress: {}", e))?;
    
    output.flush()
        .map_err(|e| format!("Failed to flush: {}", e))?;
    
    let size = fs::metadata(&output_path).map(|m| m.len()).unwrap_or(0);
    Ok(size)
}

// ============================================================================
// Public Rust API
// ============================================================================


pub async fn archive_list(path: String) -> Result<ArchiveInfo, String> {
    let format = detect_archive_format(&path);
    
    match format {
        ArchiveFormat::Zip => list_zip(&path),
        ArchiveFormat::Tar | ArchiveFormat::TarGz => list_tar(&path),
        _ => Err("Unsupported archive format".to_string()),
    }
}


pub async fn archive_extract(
    archive_path: String,
    output_dir: String,
    entries: Option<Vec<String>>,
) -> Result<Vec<String>, String> {
    let format = detect_archive_format(&archive_path);
    
    match format {
        ArchiveFormat::Zip => extract_zip(&archive_path, &output_dir, entries),
        ArchiveFormat::Tar | ArchiveFormat::TarGz => extract_tar(&archive_path, &output_dir, entries),
        ArchiveFormat::Gz => {
            decompress_gzip(&archive_path, Some(&output_dir))?;
            Ok(vec![output_dir])
        }
        _ => Err("Unsupported archive format".to_string()),
    }
}


pub async fn archive_create_zip(
    sources: Vec<String>,
    output_path: String,
    compression_level: Option<u32>,
) -> Result<u64, String> {
    let options = CompressionOptions {
        level: compression_level.unwrap_or(6),
        ..Default::default()
    };
    
    let source_paths: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
    create_zip(source_paths, &output_path, options)
}


pub async fn archive_create_tar_gz(
    sources: Vec<String>,
    output_path: String,
    compression_level: Option<u32>,
) -> Result<u64, String> {
    let options = CompressionOptions {
        level: compression_level.unwrap_or(6),
        ..Default::default()
    };
    
    let source_paths: Vec<&str> = sources.iter().map(|s| s.as_str()).collect();
    create_tar_gz(source_paths, &output_path, options)
}


pub async fn archive_compress_gzip(
    input_path: String,
    output_path: Option<String>,
    level: Option<u32>,
) -> Result<u64, String> {
    compress_gzip(&input_path, output_path.as_deref(), level.unwrap_or(6))
}


pub async fn archive_decompress_gzip(
    input_path: String,
    output_path: Option<String>,
) -> Result<u64, String> {
    decompress_gzip(&input_path, output_path.as_deref())
}


pub async fn archive_detect_format(path: String) -> ArchiveFormat {
    detect_archive_format(&path)
}
