// Pure Rust Image Processing
// Using the `image` crate - no C dependencies

use std::fs;
use std::io::Cursor;
use std::path::Path;

use image::{
    DynamicImage, GenericImageView, ImageBuffer, ImageFormat,
    Rgba, RgbaImage, imageops, codecs::jpeg::JpegEncoder, codecs::png::PngEncoder,
};
use serde::{Deserialize, Serialize};


// ============================================================================
// Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageInfo {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub color_type: String,
    pub file_size: u64,
    pub has_alpha: bool,
    pub bits_per_pixel: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeOptions {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub filter: ResizeFilter,
    pub preserve_aspect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResizeFilter {
    Nearest,    // Fastest, pixelated
    Triangle,   // Bilinear, fast
    CatmullRom, // Good quality, balanced
    Gaussian,   // Smooth, slow
    Lanczos3,   // Best quality, slowest
}

impl Default for ResizeFilter {
    fn default() -> Self {
        ResizeFilter::CatmullRom
    }
}

impl From<ResizeFilter> for imageops::FilterType {
    fn from(filter: ResizeFilter) -> Self {
        match filter {
            ResizeFilter::Nearest => imageops::FilterType::Nearest,
            ResizeFilter::Triangle => imageops::FilterType::Triangle,
            ResizeFilter::CatmullRom => imageops::FilterType::CatmullRom,
            ResizeFilter::Gaussian => imageops::FilterType::Gaussian,
            ResizeFilter::Lanczos3 => imageops::FilterType::Lanczos3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlipDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RotateAngle {
    Rotate90,
    Rotate180,
    Rotate270,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorAdjustments {
    pub brightness: f32,   // -1.0 to 1.0
    pub contrast: f32,     // -1.0 to 1.0
    pub saturation: f32,   // -1.0 to 1.0
    pub hue_rotate: f32,   // Degrees
    pub invert: bool,
    pub grayscale: bool,
}

impl Default for ColorAdjustments {
    fn default() -> Self {
        Self {
            brightness: 0.0,
            contrast: 0.0,
            saturation: 0.0,
            hue_rotate: 0.0,
            invert: false,
            grayscale: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThumbnailOptions {
    pub max_width: u32,
    pub max_height: u32,
    pub quality: u8,       // 1-100 for JPEG
    pub format: String,    // "jpeg", "png", "webp"
}

impl Default for ThumbnailOptions {
    fn default() -> Self {
        Self {
            max_width: 256,
            max_height: 256,
            quality: 85,
            format: "jpeg".to_string(),
        }
    }
}

// ============================================================================
// Image Info
// ============================================================================

pub fn get_image_info<P: AsRef<Path>>(path: P) -> Result<ImageInfo, String> {
    let path = path.as_ref();
    
    let file_size = fs::metadata(path)
        .map_err(|e| format!("Failed to read file metadata: {}", e))?
        .len();
    
    let format = image::ImageFormat::from_path(path)
        .map_err(|e| format!("Failed to detect format: {}", e))?;
    
    let img = image::open(path)
        .map_err(|e| format!("Failed to open image: {}", e))?;
    
    let (width, height) = img.dimensions();
    let color_type = img.color();
    
    let has_alpha = matches!(color_type, 
        image::ColorType::La8 | image::ColorType::La16 |
        image::ColorType::Rgba8 | image::ColorType::Rgba16 |
        image::ColorType::Rgba32F
    );
    
    let bits_per_pixel = match color_type {
        image::ColorType::L8 => 8,
        image::ColorType::La8 => 16,
        image::ColorType::Rgb8 => 24,
        image::ColorType::Rgba8 => 32,
        image::ColorType::L16 => 16,
        image::ColorType::La16 => 32,
        image::ColorType::Rgb16 => 48,
        image::ColorType::Rgba16 => 64,
        image::ColorType::Rgb32F => 96,
        image::ColorType::Rgba32F => 128,
        _ => 0,
    };
    
    Ok(ImageInfo {
        path: path.to_string_lossy().to_string(),
        width,
        height,
        format: format!("{:?}", format),
        color_type: format!("{:?}", color_type),
        file_size,
        has_alpha,
        bits_per_pixel,
    })
}

// ============================================================================
// Image Loading/Saving
// ============================================================================

pub fn load_image<P: AsRef<Path>>(path: P) -> Result<DynamicImage, String> {
    image::open(path.as_ref())
        .map_err(|e| format!("Failed to load image: {}", e))
}

pub fn save_image<P: AsRef<Path>>(
    img: &DynamicImage,
    path: P,
    quality: Option<u8>,
) -> Result<(), String> {
    let path = path.as_ref();
    
    let format = ImageFormat::from_path(path)
        .map_err(|e| format!("Failed to detect output format: {}", e))?;
    
    match format {
        ImageFormat::Jpeg => {
            let q = quality.unwrap_or(90);
            let rgb = img.to_rgb8();
            let mut file = fs::File::create(path)
                .map_err(|e| format!("Failed to create file: {}", e))?;
            let encoder = JpegEncoder::new_with_quality(&mut file, q);
            rgb.write_with_encoder(encoder)
                .map_err(|e| format!("Failed to encode JPEG: {}", e))?;
        }
        ImageFormat::Png => {
            img.save(path)
                .map_err(|e| format!("Failed to save PNG: {}", e))?;
        }
        _ => {
            img.save(path)
                .map_err(|e| format!("Failed to save image: {}", e))?;
        }
    }
    
    Ok(())
}

pub fn image_to_bytes(img: &DynamicImage, format: &str, _quality: u8) -> Result<Vec<u8>, String> {
    let mut buffer = Vec::new();
    let mut cursor = Cursor::new(&mut buffer);
    
    let output_format = match format.to_lowercase().as_str() {
        "jpeg" | "jpg" => ImageFormat::Jpeg,
        "png" => ImageFormat::Png,
        "gif" => ImageFormat::Gif,
        "bmp" => ImageFormat::Bmp,
        "ico" => ImageFormat::Ico,
        "tiff" | "tif" => ImageFormat::Tiff,
        "webp" => ImageFormat::WebP,
        _ => return Err(format!("Unsupported format: {}", format)),
    };
    
    img.write_to(&mut cursor, output_format)
        .map_err(|e| format!("Failed to encode image: {}", e))?;
    
    Ok(buffer)
}

// ============================================================================
// Resize Operations
// ============================================================================

pub fn resize_image(
    img: &DynamicImage,
    options: &ResizeOptions,
) -> Result<DynamicImage, String> {
    let (orig_width, orig_height) = img.dimensions();
    
    let (new_width, new_height) = if options.preserve_aspect {
        calculate_aspect_size(
            orig_width,
            orig_height,
            options.width,
            options.height,
        )
    } else {
        (
            options.width.unwrap_or(orig_width),
            options.height.unwrap_or(orig_height),
        )
    };
    
    Ok(img.resize_exact(new_width, new_height, options.filter.into()))
}

fn calculate_aspect_size(
    orig_width: u32,
    orig_height: u32,
    target_width: Option<u32>,
    target_height: Option<u32>,
) -> (u32, u32) {
    let aspect = orig_width as f64 / orig_height as f64;
    
    match (target_width, target_height) {
        (Some(w), Some(h)) => {
            // Fit within bounds
            let scale_w = w as f64 / orig_width as f64;
            let scale_h = h as f64 / orig_height as f64;
            let scale = scale_w.min(scale_h);
            (
                (orig_width as f64 * scale) as u32,
                (orig_height as f64 * scale) as u32,
            )
        }
        (Some(w), None) => (w, (w as f64 / aspect) as u32),
        (None, Some(h)) => ((h as f64 * aspect) as u32, h),
        (None, None) => (orig_width, orig_height),
    }
}

pub fn create_thumbnail(
    img: &DynamicImage,
    options: &ThumbnailOptions,
) -> Result<DynamicImage, String> {
    Ok(img.thumbnail(options.max_width, options.max_height))
}

// ============================================================================
// Transform Operations
// ============================================================================

pub fn crop_image(img: &DynamicImage, rect: &CropRect) -> Result<DynamicImage, String> {
    let (width, height) = img.dimensions();
    
    // Validate crop bounds
    if rect.x + rect.width > width || rect.y + rect.height > height {
        return Err("Crop rectangle exceeds image bounds".to_string());
    }
    
    Ok(img.crop_imm(rect.x, rect.y, rect.width, rect.height))
}

pub fn flip_image(img: &DynamicImage, direction: FlipDirection) -> DynamicImage {
    match direction {
        FlipDirection::Horizontal => img.fliph(),
        FlipDirection::Vertical => img.flipv(),
    }
}

pub fn rotate_image(img: &DynamicImage, angle: RotateAngle) -> DynamicImage {
    match angle {
        RotateAngle::Rotate90 => img.rotate90(),
        RotateAngle::Rotate180 => img.rotate180(),
        RotateAngle::Rotate270 => img.rotate270(),
    }
}

// ============================================================================
// Color Adjustments
// ============================================================================

pub fn adjust_colors(img: &DynamicImage, adjustments: &ColorAdjustments) -> DynamicImage {
    let mut result = img.clone();
    
    // Apply grayscale first if requested
    if adjustments.grayscale {
        result = DynamicImage::ImageLuma8(result.to_luma8()).into_rgba8().into();
    }
    
    // Apply brightness/contrast
    if adjustments.brightness != 0.0 || adjustments.contrast != 0.0 {
        result = adjust_brightness_contrast(&result, adjustments.brightness, adjustments.contrast);
    }
    
    // Apply saturation
    if adjustments.saturation != 0.0 {
        result = adjust_saturation(&result, adjustments.saturation);
    }
    
    // Apply hue rotation
    if adjustments.hue_rotate != 0.0 {
        result = rotate_hue(&result, adjustments.hue_rotate);
    }
    
    // Invert
    if adjustments.invert {
        result.invert();
    }
    
    result
}

fn adjust_brightness_contrast(img: &DynamicImage, brightness: f32, contrast: f32) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    
    let mut output = RgbaImage::new(width, height);
    
    // Contrast multiplier (1.0 + contrast gives range 0.0 to 2.0)
    let contrast_factor = 1.0 + contrast;
    // Brightness offset (-255 to 255)
    let brightness_offset = (brightness * 255.0) as i32;
    
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let r = adjust_channel(pixel[0], brightness_offset, contrast_factor);
        let g = adjust_channel(pixel[1], brightness_offset, contrast_factor);
        let b = adjust_channel(pixel[2], brightness_offset, contrast_factor);
        output.put_pixel(x, y, Rgba([r, g, b, pixel[3]]));
    }
    
    DynamicImage::ImageRgba8(output)
}

fn adjust_channel(value: u8, brightness: i32, contrast: f32) -> u8 {
    let v = value as f32;
    // Apply contrast (centered at 128)
    let contrasted = ((v - 128.0) * contrast + 128.0) as i32;
    // Apply brightness
    let result = contrasted + brightness;
    result.clamp(0, 255) as u8
}

fn adjust_saturation(img: &DynamicImage, saturation: f32) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    
    let mut output = RgbaImage::new(width, height);
    let sat_factor = 1.0 + saturation;
    
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let r = pixel[0] as f32 / 255.0;
        let g = pixel[1] as f32 / 255.0;
        let b = pixel[2] as f32 / 255.0;
        
        // Convert to grayscale (luminance)
        let gray = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        
        // Interpolate between gray and original
        let new_r = ((gray + sat_factor * (r - gray)) * 255.0).clamp(0.0, 255.0) as u8;
        let new_g = ((gray + sat_factor * (g - gray)) * 255.0).clamp(0.0, 255.0) as u8;
        let new_b = ((gray + sat_factor * (b - gray)) * 255.0).clamp(0.0, 255.0) as u8;
        
        output.put_pixel(x, y, Rgba([new_r, new_g, new_b, pixel[3]]));
    }
    
    DynamicImage::ImageRgba8(output)
}

fn rotate_hue(img: &DynamicImage, degrees: f32) -> DynamicImage {
    let rgba = img.to_rgba8();
    let (width, height) = rgba.dimensions();
    
    let mut output = RgbaImage::new(width, height);
    let angle = degrees * std::f32::consts::PI / 180.0;
    
    for (x, y, pixel) in rgba.enumerate_pixels() {
        let (h, s, l) = rgb_to_hsl(pixel[0], pixel[1], pixel[2]);
        let new_h = (h + angle) % (2.0 * std::f32::consts::PI);
        let (r, g, b) = hsl_to_rgb(new_h, s, l);
        output.put_pixel(x, y, Rgba([r, g, b, pixel[3]]));
    }
    
    DynamicImage::ImageRgba8(output)
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    
    if max == min {
        return (0.0, 0.0, l);
    }
    
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    
    let h = if max == r {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) / 6.0
    } else if max == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    
    (h * 2.0 * std::f32::consts::PI, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (u8, u8, u8) {
    if s == 0.0 {
        let v = (l * 255.0) as u8;
        return (v, v, v);
    }
    
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let h_norm = h / (2.0 * std::f32::consts::PI);
    
    let r = hue_to_rgb(p, q, h_norm + 1.0/3.0);
    let g = hue_to_rgb(p, q, h_norm);
    let b = hue_to_rgb(p, q, h_norm - 1.0/3.0);
    
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 { t += 1.0; }
    if t > 1.0 { t -= 1.0; }
    
    if t < 1.0/6.0 { return p + (q - p) * 6.0 * t; }
    if t < 1.0/2.0 { return q; }
    if t < 2.0/3.0 { return p + (q - p) * (2.0/3.0 - t) * 6.0; }
    p
}

// ============================================================================
// Filters
// ============================================================================

pub fn apply_blur(img: &DynamicImage, sigma: f32) -> DynamicImage {
    img.blur(sigma)
}

pub fn apply_sharpen(img: &DynamicImage) -> DynamicImage {
    img.unsharpen(1.0, 1)
}

// ============================================================================
// Format Conversion
// ============================================================================

pub fn convert_format<P: AsRef<Path>>(
    input_path: P,
    output_path: P,
    quality: Option<u8>,
) -> Result<(), String> {
    let img = load_image(&input_path)?;
    save_image(&img, output_path, quality)
}

// ============================================================================
// Tauri Commands
// ============================================================================


pub async fn image_get_info(path: String) -> Result<ImageInfo, String> {
    get_image_info(&path)
}


pub async fn image_resize(
    input_path: String,
    output_path: String,
    width: Option<u32>,
    height: Option<u32>,
    filter: Option<String>,
    preserve_aspect: Option<bool>,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    let img = load_image(&input_path)?;
    
    let filter = match filter.as_deref() {
        Some("nearest") => ResizeFilter::Nearest,
        Some("triangle") | Some("bilinear") => ResizeFilter::Triangle,
        Some("gaussian") => ResizeFilter::Gaussian,
        Some("lanczos") | Some("lanczos3") => ResizeFilter::Lanczos3,
        _ => ResizeFilter::CatmullRom,
    };
    
    let options = ResizeOptions {
        width,
        height,
        filter,
        preserve_aspect: preserve_aspect.unwrap_or(true),
    };
    
    let resized = resize_image(&img, &options)?;
    save_image(&resized, &output_path, quality)?;
    
    get_image_info(&output_path)
}


pub async fn image_crop(
    input_path: String,
    output_path: String,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    let img = load_image(&input_path)?;
    let rect = CropRect { x, y, width, height };
    let cropped = crop_image(&img, &rect)?;
    save_image(&cropped, &output_path, quality)?;
    get_image_info(&output_path)
}


pub async fn image_rotate(
    input_path: String,
    output_path: String,
    angle: i32,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    let img = load_image(&input_path)?;
    
    let rotation = match angle {
        90 | -270 => RotateAngle::Rotate90,
        180 | -180 => RotateAngle::Rotate180,
        270 | -90 => RotateAngle::Rotate270,
        _ => return Err("Angle must be 90, 180, or 270 degrees".to_string()),
    };
    
    let rotated = rotate_image(&img, rotation);
    save_image(&rotated, &output_path, quality)?;
    get_image_info(&output_path)
}


pub async fn image_flip(
    input_path: String,
    output_path: String,
    direction: String,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    let img = load_image(&input_path)?;
    
    let dir = match direction.to_lowercase().as_str() {
        "horizontal" | "h" => FlipDirection::Horizontal,
        "vertical" | "v" => FlipDirection::Vertical,
        _ => return Err("Direction must be 'horizontal' or 'vertical'".to_string()),
    };
    
    let flipped = flip_image(&img, dir);
    save_image(&flipped, &output_path, quality)?;
    get_image_info(&output_path)
}


pub async fn image_thumbnail(
    input_path: String,
    output_path: String,
    max_width: Option<u32>,
    max_height: Option<u32>,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    let img = load_image(&input_path)?;
    
    let options = ThumbnailOptions {
        max_width: max_width.unwrap_or(256),
        max_height: max_height.unwrap_or(256),
        quality: quality.unwrap_or(85),
        format: "jpeg".to_string(),
    };
    
    let thumbnail = create_thumbnail(&img, &options)?;
    save_image(&thumbnail, &output_path, Some(options.quality))?;
    get_image_info(&output_path)
}


pub async fn image_convert(
    input_path: String,
    output_path: String,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    convert_format(&input_path, &output_path, quality)?;
    get_image_info(&output_path)
}


pub async fn image_adjust(
    input_path: String,
    output_path: String,
    brightness: Option<f32>,
    contrast: Option<f32>,
    saturation: Option<f32>,
    hue_rotate: Option<f32>,
    grayscale: Option<bool>,
    invert: Option<bool>,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    let img = load_image(&input_path)?;
    
    let adjustments = ColorAdjustments {
        brightness: brightness.unwrap_or(0.0).clamp(-1.0, 1.0),
        contrast: contrast.unwrap_or(0.0).clamp(-1.0, 1.0),
        saturation: saturation.unwrap_or(0.0).clamp(-1.0, 1.0),
        hue_rotate: hue_rotate.unwrap_or(0.0),
        grayscale: grayscale.unwrap_or(false),
        invert: invert.unwrap_or(false),
    };
    
    let adjusted = adjust_colors(&img, &adjustments);
    save_image(&adjusted, &output_path, quality)?;
    get_image_info(&output_path)
}


pub async fn image_blur(
    input_path: String,
    output_path: String,
    sigma: f32,
    quality: Option<u8>,
) -> Result<ImageInfo, String> {
    let img = load_image(&input_path)?;
    let blurred = apply_blur(&img, sigma);
    save_image(&blurred, &output_path, quality)?;
    get_image_info(&output_path)
}

// ============================================================================
// Supported Formats
// ============================================================================

pub fn supported_image_formats() -> Vec<&'static str> {
    vec![
        "png", "jpeg", "jpg", "gif", "webp", "bmp", "ico", "tiff", "tif",
        "pnm", "pbm", "pgm", "ppm", "pam", "dds", "tga", "farbfeld", "openexr",
    ]
}


pub async fn image_supported_formats() -> Vec<&'static str> {
    supported_image_formats()
}
