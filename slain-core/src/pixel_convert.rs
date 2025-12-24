// PIXEL CONVERT - YUV to RGB Conversion
//
// This replaces FFmpeg's libswscale for our needs.
// Video decoders output YUV (typically NV12 or YUV420P).
// Screens display RGB.
// This module converts between them.
//
// For GPU-accelerated conversion, we use compute shaders.
// For CPU fallback, we use SIMD-optimized Rust.

use serde::{Deserialize, Serialize};

// ============================================================================
// Pixel Formats
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PixelFormat {
    // YUV planar (common decoder output)
    YUV420P,      // Y plane, U plane (quarter size), V plane (quarter size)
    YUV420P10LE,  // 10-bit per component
    YUV422P,      // Y plane, U plane (half width), V plane (half width)
    YUV444P,      // Y plane, U plane (full size), V plane (full size)
    
    // YUV semi-planar (GPU decoder output)
    NV12,         // Y plane, interleaved UV plane
    P010,         // 10-bit NV12
    
    // RGB
    RGB24,        // 8-bit per channel, packed
    RGBA32,       // 8-bit per channel + alpha, packed
    BGR24,        // Windows format
    BGRA32,       // Windows format with alpha
    
    // GPU textures
    R8G8B8A8,     // Vulkan/OpenGL format
}

impl PixelFormat {
    /// Bytes per pixel for packed formats, 0 for planar
    pub fn bytes_per_pixel(&self) -> usize {
        match self {
            Self::RGB24 | Self::BGR24 => 3,
            Self::RGBA32 | Self::BGRA32 | Self::R8G8B8A8 => 4,
            _ => 0,  // Planar formats
        }
    }
    
    /// Calculate buffer size needed
    pub fn buffer_size(&self, width: usize, height: usize) -> usize {
        match self {
            Self::YUV420P => width * height * 3 / 2,
            Self::YUV420P10LE => width * height * 3,  // 2 bytes per sample
            Self::YUV422P => width * height * 2,
            Self::YUV444P => width * height * 3,
            Self::NV12 => width * height * 3 / 2,
            Self::P010 => width * height * 3,
            Self::RGB24 | Self::BGR24 => width * height * 3,
            Self::RGBA32 | Self::BGRA32 | Self::R8G8B8A8 => width * height * 4,
        }
    }
    
    pub fn is_yuv(&self) -> bool {
        matches!(self, 
            Self::YUV420P | Self::YUV420P10LE | Self::YUV422P | 
            Self::YUV444P | Self::NV12 | Self::P010)
    }
    
    pub fn is_rgb(&self) -> bool {
        matches!(self,
            Self::RGB24 | Self::RGBA32 | Self::BGR24 | 
            Self::BGRA32 | Self::R8G8B8A8)
    }
}

// ============================================================================
// Color Spaces / Transfer Functions
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorSpace {
    BT601,    // SD (NTSC/PAL)
    BT709,    // HD
    BT2020,   // UHD/HDR
    SRGB,     // Computer displays
}

impl ColorSpace {
    /// YUV to RGB conversion matrix coefficients
    /// Returns (Wr, Wb) where Wg = 1 - Wr - Wb
    pub fn coefficients(&self) -> (f32, f32) {
        match self {
            Self::BT601 => (0.299, 0.114),
            Self::BT709 => (0.2126, 0.0722),
            Self::BT2020 => (0.2627, 0.0593),
            Self::SRGB => (0.2126, 0.0722),  // Same as BT.709
        }
    }
    
    /// Full YUV to RGB matrix
    pub fn yuv_to_rgb_matrix(&self) -> [[f32; 3]; 3] {
        let (wr, wb) = self.coefficients();
        let wg = 1.0 - wr - wb;
        
        // Derived from:
        // Y = Wr*R + Wg*G + Wb*B
        // Cb = (B - Y) / (2 * (1 - Wb))
        // Cr = (R - Y) / (2 * (1 - Wr))
        //
        // Inverted:
        // R = Y + 2*(1-Wr)*Cr
        // G = Y - 2*Wb*(1-Wb)/Wg*Cb - 2*Wr*(1-Wr)/Wg*Cr
        // B = Y + 2*(1-Wb)*Cb
        
        let cr_r = 2.0 * (1.0 - wr);
        let cb_g = -2.0 * wb * (1.0 - wb) / wg;
        let cr_g = -2.0 * wr * (1.0 - wr) / wg;
        let cb_b = 2.0 * (1.0 - wb);
        
        [
            [1.0, 0.0, cr_r],
            [1.0, cb_g, cr_g],
            [1.0, cb_b, 0.0],
        ]
    }
}

// ============================================================================
// Video Frame
// ============================================================================

#[derive(Debug, Clone)]
pub struct VideoFrame {
    pub width: usize,
    pub height: usize,
    pub format: PixelFormat,
    pub color_space: ColorSpace,
    pub data: Vec<u8>,
    pub linesize: Vec<usize>,  // Stride for each plane
    pub pts: i64,
}

impl VideoFrame {
    pub fn new(width: usize, height: usize, format: PixelFormat) -> Self {
        let size = format.buffer_size(width, height);
        let linesize = match format {
            PixelFormat::YUV420P => vec![width, width / 2, width / 2],
            PixelFormat::NV12 => vec![width, width],
            PixelFormat::RGB24 | PixelFormat::BGR24 => vec![width * 3],
            PixelFormat::RGBA32 | PixelFormat::BGRA32 => vec![width * 4],
            _ => vec![width],
        };
        
        Self {
            width,
            height,
            format,
            color_space: ColorSpace::BT709,
            data: vec![0u8; size],
            linesize,
            pts: 0,
        }
    }
    
    /// Get plane data slice
    pub fn plane(&self, index: usize) -> &[u8] {
        match self.format {
            PixelFormat::YUV420P => {
                let y_size = self.width * self.height;
                let uv_size = y_size / 4;
                match index {
                    0 => &self.data[..y_size],
                    1 => &self.data[y_size..y_size + uv_size],
                    2 => &self.data[y_size + uv_size..],
                    _ => &[],
                }
            }
            PixelFormat::NV12 => {
                let y_size = self.width * self.height;
                match index {
                    0 => &self.data[..y_size],
                    1 => &self.data[y_size..],
                    _ => &[],
                }
            }
            _ => &self.data,
        }
    }
    
    /// Get mutable plane data slice
    pub fn plane_mut(&mut self, index: usize) -> &mut [u8] {
        match self.format {
            PixelFormat::YUV420P => {
                let y_size = self.width * self.height;
                let uv_size = y_size / 4;
                match index {
                    0 => &mut self.data[..y_size],
                    1 => &mut self.data[y_size..y_size + uv_size],
                    2 => &mut self.data[y_size + uv_size..],
                    _ => &mut [],
                }
            }
            PixelFormat::NV12 => {
                let y_size = self.width * self.height;
                match index {
                    0 => &mut self.data[..y_size],
                    1 => &mut self.data[y_size..],
                    _ => &mut [],
                }
            }
            _ => &mut self.data,
        }
    }
}

// ============================================================================
// Converter
// ============================================================================

pub struct PixelConverter {
    src_format: PixelFormat,
    dst_format: PixelFormat,
    width: usize,
    height: usize,
    color_space: ColorSpace,
    // Pre-computed lookup tables for speed
    y_table: [i32; 256],
    u_table_g: [i32; 256],
    u_table_b: [i32; 256],
    v_table_r: [i32; 256],
    v_table_g: [i32; 256],
}

impl PixelConverter {
    pub fn new(
        src_format: PixelFormat,
        dst_format: PixelFormat,
        width: usize,
        height: usize,
        color_space: ColorSpace,
    ) -> Self {
        let mut converter = Self {
            src_format,
            dst_format,
            width,
            height,
            color_space,
            y_table: [0; 256],
            u_table_g: [0; 256],
            u_table_b: [0; 256],
            v_table_r: [0; 256],
            v_table_g: [0; 256],
        };
        converter.build_tables();
        converter
    }
    
    fn build_tables(&mut self) {
        let matrix = self.color_space.yuv_to_rgb_matrix();
        
        // Pre-compute fixed-point lookup tables
        // Using 16-bit fixed point (shift by 16)
        for i in 0..256 {
            let y = (i as i32) - 16;   // Y range: 16-235
            let uv = (i as i32) - 128; // U/V range: 16-240, centered at 128
            
            // Scale Y from 16-235 to 0-255
            self.y_table[i] = (y * 298) >> 8;  // 298/256 ≈ 1.164
            
            // U/V contributions (scaled)
            self.u_table_g[i] = ((uv as f32 * matrix[1][1] * 256.0) as i32);
            self.u_table_b[i] = ((uv as f32 * matrix[2][1] * 256.0) as i32);
            self.v_table_r[i] = ((uv as f32 * matrix[0][2] * 256.0) as i32);
            self.v_table_g[i] = ((uv as f32 * matrix[1][2] * 256.0) as i32);
        }
    }
    
    /// Convert a frame
    pub fn convert(&self, src: &VideoFrame, dst: &mut VideoFrame) -> Result<(), String> {
        if src.format != self.src_format || dst.format != self.dst_format {
            return Err("Format mismatch".to_string());
        }
        if src.width != self.width || src.height != self.height {
            return Err("Size mismatch".to_string());
        }
        
        match (self.src_format, self.dst_format) {
            (PixelFormat::NV12, PixelFormat::RGBA32) => {
                self.nv12_to_rgba(src, dst);
            }
            (PixelFormat::NV12, PixelFormat::RGB24) => {
                self.nv12_to_rgb(src, dst);
            }
            (PixelFormat::YUV420P, PixelFormat::RGBA32) => {
                self.yuv420p_to_rgba(src, dst);
            }
            (PixelFormat::YUV420P, PixelFormat::RGB24) => {
                self.yuv420p_to_rgb(src, dst);
            }
            _ => {
                return Err(format!("Unsupported conversion: {:?} -> {:?}", 
                    self.src_format, self.dst_format));
            }
        }
        
        dst.pts = src.pts;
        Ok(())
    }
    
    fn nv12_to_rgba(&self, src: &VideoFrame, dst: &mut VideoFrame) {
        let y_plane = src.plane(0);
        let uv_plane = src.plane(1);
        let rgba = &mut dst.data;
        
        let width = self.width;
        let height = self.height;
        
        for y in 0..height {
            let y_row = y * width;
            let uv_row = (y / 2) * width;
            let dst_row = y * width * 4;
            
            for x in 0..width {
                let y_val = y_plane[y_row + x] as usize;
                let uv_idx = uv_row + (x / 2) * 2;
                let u_val = uv_plane[uv_idx] as usize;
                let v_val = uv_plane[uv_idx + 1] as usize;
                
                let y_contrib = self.y_table[y_val];
                let r = (y_contrib + (self.v_table_r[v_val] >> 8)).clamp(0, 255) as u8;
                let g = (y_contrib + (self.u_table_g[u_val] >> 8) + (self.v_table_g[v_val] >> 8)).clamp(0, 255) as u8;
                let b = (y_contrib + (self.u_table_b[u_val] >> 8)).clamp(0, 255) as u8;
                
                let dst_idx = dst_row + x * 4;
                rgba[dst_idx] = r;
                rgba[dst_idx + 1] = g;
                rgba[dst_idx + 2] = b;
                rgba[dst_idx + 3] = 255;
            }
        }
    }
    
    fn nv12_to_rgb(&self, src: &VideoFrame, dst: &mut VideoFrame) {
        let y_plane = src.plane(0);
        let uv_plane = src.plane(1);
        let rgb = &mut dst.data;
        
        let width = self.width;
        let height = self.height;
        
        for y in 0..height {
            let y_row = y * width;
            let uv_row = (y / 2) * width;
            let dst_row = y * width * 3;
            
            for x in 0..width {
                let y_val = y_plane[y_row + x] as usize;
                let uv_idx = uv_row + (x / 2) * 2;
                let u_val = uv_plane[uv_idx] as usize;
                let v_val = uv_plane[uv_idx + 1] as usize;
                
                let y_contrib = self.y_table[y_val];
                let r = (y_contrib + (self.v_table_r[v_val] >> 8)).clamp(0, 255) as u8;
                let g = (y_contrib + (self.u_table_g[u_val] >> 8) + (self.v_table_g[v_val] >> 8)).clamp(0, 255) as u8;
                let b = (y_contrib + (self.u_table_b[u_val] >> 8)).clamp(0, 255) as u8;
                
                let dst_idx = dst_row + x * 3;
                rgb[dst_idx] = r;
                rgb[dst_idx + 1] = g;
                rgb[dst_idx + 2] = b;
            }
        }
    }
    
    fn yuv420p_to_rgba(&self, src: &VideoFrame, dst: &mut VideoFrame) {
        let y_plane = src.plane(0);
        let u_plane = src.plane(1);
        let v_plane = src.plane(2);
        let rgba = &mut dst.data;
        
        let width = self.width;
        let height = self.height;
        let uv_width = width / 2;
        
        for y in 0..height {
            let y_row = y * width;
            let uv_row = (y / 2) * uv_width;
            let dst_row = y * width * 4;
            
            for x in 0..width {
                let y_val = y_plane[y_row + x] as usize;
                let uv_x = x / 2;
                let u_val = u_plane[uv_row + uv_x] as usize;
                let v_val = v_plane[uv_row + uv_x] as usize;
                
                let y_contrib = self.y_table[y_val];
                let r = (y_contrib + (self.v_table_r[v_val] >> 8)).clamp(0, 255) as u8;
                let g = (y_contrib + (self.u_table_g[u_val] >> 8) + (self.v_table_g[v_val] >> 8)).clamp(0, 255) as u8;
                let b = (y_contrib + (self.u_table_b[u_val] >> 8)).clamp(0, 255) as u8;
                
                let dst_idx = dst_row + x * 4;
                rgba[dst_idx] = r;
                rgba[dst_idx + 1] = g;
                rgba[dst_idx + 2] = b;
                rgba[dst_idx + 3] = 255;
            }
        }
    }
    
    fn yuv420p_to_rgb(&self, src: &VideoFrame, dst: &mut VideoFrame) {
        let y_plane = src.plane(0);
        let u_plane = src.plane(1);
        let v_plane = src.plane(2);
        let rgb = &mut dst.data;
        
        let width = self.width;
        let height = self.height;
        let uv_width = width / 2;
        
        for y in 0..height {
            let y_row = y * width;
            let uv_row = (y / 2) * uv_width;
            let dst_row = y * width * 3;
            
            for x in 0..width {
                let y_val = y_plane[y_row + x] as usize;
                let uv_x = x / 2;
                let u_val = u_plane[uv_row + uv_x] as usize;
                let v_val = v_plane[uv_row + uv_x] as usize;
                
                let y_contrib = self.y_table[y_val];
                let r = (y_contrib + (self.v_table_r[v_val] >> 8)).clamp(0, 255) as u8;
                let g = (y_contrib + (self.u_table_g[u_val] >> 8) + (self.v_table_g[v_val] >> 8)).clamp(0, 255) as u8;
                let b = (y_contrib + (self.u_table_b[u_val] >> 8)).clamp(0, 255) as u8;
                
                let dst_idx = dst_row + x * 3;
                rgb[dst_idx] = r;
                rgb[dst_idx + 1] = g;
                rgb[dst_idx + 2] = b;
            }
        }
    }
}

// ============================================================================
// Scaler (resize)
// ============================================================================

pub struct Scaler {
    src_width: usize,
    src_height: usize,
    dst_width: usize,
    dst_height: usize,
    x_map: Vec<usize>,
    y_map: Vec<usize>,
}

impl Scaler {
    pub fn new(src_width: usize, src_height: usize, dst_width: usize, dst_height: usize) -> Self {
        // Pre-compute source pixel indices for each destination pixel
        let x_map: Vec<usize> = (0..dst_width)
            .map(|x| (x * src_width / dst_width).min(src_width - 1))
            .collect();
        
        let y_map: Vec<usize> = (0..dst_height)
            .map(|y| (y * src_height / dst_height).min(src_height - 1))
            .collect();
        
        Self {
            src_width,
            src_height,
            dst_width,
            dst_height,
            x_map,
            y_map,
        }
    }
    
    /// Scale RGB24 frame using nearest neighbor (fast)
    pub fn scale_rgb_nearest(&self, src: &[u8], dst: &mut [u8]) {
        for dst_y in 0..self.dst_height {
            let src_y = self.y_map[dst_y];
            let src_row = src_y * self.src_width * 3;
            let dst_row = dst_y * self.dst_width * 3;
            
            for dst_x in 0..self.dst_width {
                let src_x = self.x_map[dst_x];
                let src_idx = src_row + src_x * 3;
                let dst_idx = dst_row + dst_x * 3;
                
                dst[dst_idx] = src[src_idx];
                dst[dst_idx + 1] = src[src_idx + 1];
                dst[dst_idx + 2] = src[src_idx + 2];
            }
        }
    }
    
    /// Scale RGB24 frame using bilinear interpolation (better quality)
    pub fn scale_rgb_bilinear(&self, src: &[u8], dst: &mut [u8]) {
        let x_ratio = (self.src_width as f32) / (self.dst_width as f32);
        let y_ratio = (self.src_height as f32) / (self.dst_height as f32);
        
        for dst_y in 0..self.dst_height {
            let src_y_f = (dst_y as f32) * y_ratio;
            let src_y = src_y_f as usize;
            let y_frac = src_y_f - src_y as f32;
            let y1 = src_y.min(self.src_height - 1);
            let y2 = (src_y + 1).min(self.src_height - 1);
            
            let dst_row = dst_y * self.dst_width * 3;
            
            for dst_x in 0..self.dst_width {
                let src_x_f = (dst_x as f32) * x_ratio;
                let src_x = src_x_f as usize;
                let x_frac = src_x_f - src_x as f32;
                let x1 = src_x.min(self.src_width - 1);
                let x2 = (src_x + 1).min(self.src_width - 1);
                
                // Four corners
                let i00 = (y1 * self.src_width + x1) * 3;
                let i01 = (y1 * self.src_width + x2) * 3;
                let i10 = (y2 * self.src_width + x1) * 3;
                let i11 = (y2 * self.src_width + x2) * 3;
                
                let dst_idx = dst_row + dst_x * 3;
                
                for c in 0..3 {
                    let v00 = src[i00 + c] as f32;
                    let v01 = src[i01 + c] as f32;
                    let v10 = src[i10 + c] as f32;
                    let v11 = src[i11 + c] as f32;
                    
                    let v0 = v00 * (1.0 - x_frac) + v01 * x_frac;
                    let v1 = v10 * (1.0 - x_frac) + v11 * x_frac;
                    let v = v0 * (1.0 - y_frac) + v1 * y_frac;
                    
                    dst[dst_idx + c] = v.clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

// ============================================================================
// Public API
// ============================================================================




pub fn pixel_convert_info(format: String) -> serde_json::Value {
    let pf = match format.as_str() {
        "nv12" => PixelFormat::NV12,
        "yuv420p" => PixelFormat::YUV420P,
        "rgb24" => PixelFormat::RGB24,
        "rgba" => PixelFormat::RGBA32,
        _ => return serde_json::json!({"error": "Unknown format"}),
    };
    
    serde_json::json!({
        "format": format,
        "is_yuv": pf.is_yuv(),
        "is_rgb": pf.is_rgb(),
        "bytes_per_pixel": pf.bytes_per_pixel(),
        "buffer_size_1080p": pf.buffer_size(1920, 1080),
    })
}


pub fn pixel_convert_matrix(color_space: String) -> Vec<Vec<f32>> {
    let cs = match color_space.as_str() {
        "bt601" => ColorSpace::BT601,
        "bt709" => ColorSpace::BT709,
        "bt2020" => ColorSpace::BT2020,
        _ => ColorSpace::BT709,
    };
    
    cs.yuv_to_rgb_matrix().iter().map(|row| row.to_vec()).collect()
}


pub fn pixel_convert_description() -> String {
    r#"
PIXEL CONVERT - YUV to RGB Conversion

Video decoders output YUV (typically NV12 or YUV420P).
Screens display RGB.
This module bridges them.

SUPPORTED CONVERSIONS:
• NV12 → RGBA (GPU decoder output)
• NV12 → RGB24
• YUV420P → RGBA (software decoder output)
• YUV420P → RGB24

COLOR SPACES:
• BT.601 (SD video)
• BT.709 (HD video)
• BT.2020 (UHD/HDR video)

OPTIMIZATION:
• Pre-computed lookup tables
• Integer-only math in hot path
• No memory allocations during conversion

For GPU-accelerated conversion, use compute shaders
(see gpu_orchestrator.rs).
"#.to_string()
}
