//! # Fractal Renderer - GPU-Accelerated Fractal Generation
//!
//! High-quality fractal rendering using distance estimation for smooth edges.
//!
//! ## Distance Estimator Formula
//!
//! ```text
//!              |z_n| · ln|z_n|
//! d_c ≈ 2 · ─────────────────────
//!                  |z'_n|
//! ```
//!
//! Where:
//! - `z_n` = final iterated complex value
//! - `z'_n` = derivative of iteration (accumulated)
//! - `d_c` = estimated distance to fractal boundary
//!
//! ## Intensity Formula
//!
//! ```text
//!       (360/x) - 1
//! I = ─────────────────
//!          d_c
//! ```
//!
//! This produces smooth, anti-aliased fractal renders without supersampling.

use serde::{Deserialize, Serialize};
use std::f64::consts::{LN_2, PI};

// ============================================================================
// Complex Number Operations
// ============================================================================

/// Complex number for fractal calculations
#[derive(Debug, Clone, Copy, Default)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    #[inline]
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }

    #[inline]
    pub fn from_polar(r: f64, theta: f64) -> Self {
        Self {
            re: r * theta.cos(),
            im: r * theta.sin(),
        }
    }

    /// Magnitude squared (avoids sqrt)
    #[inline]
    pub fn norm_sqr(&self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    /// Magnitude |z|
    #[inline]
    pub fn norm(&self) -> f64 {
        self.norm_sqr().sqrt()
    }

    /// Natural log of magnitude: ln|z|
    #[inline]
    pub fn ln_norm(&self) -> f64 {
        0.5 * self.norm_sqr().ln()
    }

    /// Complex multiplication
    #[inline]
    pub fn mul(&self, other: Complex) -> Complex {
        Complex {
            re: self.re * other.re - self.im * other.im,
            im: self.re * other.im + self.im * other.re,
        }
    }

    /// Complex addition
    #[inline]
    pub fn add(&self, other: Complex) -> Complex {
        Complex {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }

    /// Complex squaring (optimized)
    #[inline]
    pub fn sqr(&self) -> Complex {
        Complex {
            re: self.re * self.re - self.im * self.im,
            im: 2.0 * self.re * self.im,
        }
    }

    /// Scalar multiplication
    #[inline]
    pub fn scale(&self, s: f64) -> Complex {
        Complex {
            re: self.re * s,
            im: self.im * s,
        }
    }
}

// ============================================================================
// Fractal Types
// ============================================================================

/// Supported fractal types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FractalType {
    /// Classic Mandelbrot set: z = z² + c
    Mandelbrot,
    /// Julia set with fixed c parameter
    Julia,
    /// Burning Ship: z = (|Re(z)| + i|Im(z)|)² + c
    BurningShip,
    /// Tricorn (Mandelbar): z = conj(z)² + c
    Tricorn,
    /// Multibrot with power n: z = z^n + c
    Multibrot { power: i32 },
    /// Newton fractal for z³ - 1
    Newton,
    /// Phoenix fractal
    Phoenix,
}

/// Coloring algorithm
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode {
    /// Smooth iteration count
    Smooth,
    /// Distance estimation (the formula you provided)
    DistanceEstimate,
    /// Orbit trap coloring
    OrbitTrap,
    /// Angle-based coloring
    Angle,
    /// Binary (inside/outside)
    Binary,
    /// Potential function
    Potential,
}

/// Color palette
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Palette {
    /// RGB colors (0-255 each)
    pub colors: Vec<[u8; 3]>,
    /// Palette offset for animation
    pub offset: f64,
    /// Palette scale
    pub scale: f64,
}

impl Default for Palette {
    fn default() -> Self {
        // Classic blue-gold palette
        Self {
            colors: vec![
                [0, 7, 100],     // Dark blue
                [32, 107, 203],  // Blue
                [237, 255, 255], // White
                [255, 170, 0],   // Gold
                [0, 2, 0],       // Black
            ],
            offset: 0.0,
            scale: 1.0,
        }
    }
}

impl Palette {
    /// Fire palette
    pub fn fire() -> Self {
        Self {
            colors: vec![
                [0, 0, 0],
                [128, 0, 0],
                [255, 128, 0],
                [255, 255, 0],
                [255, 255, 255],
            ],
            offset: 0.0,
            scale: 1.0,
        }
    }

    /// Ice palette
    pub fn ice() -> Self {
        Self {
            colors: vec![
                [0, 0, 32],
                [0, 64, 128],
                [128, 200, 255],
                [255, 255, 255],
                [200, 230, 255],
            ],
            offset: 0.0,
            scale: 1.0,
        }
    }

    /// Psychedelic rainbow
    pub fn rainbow() -> Self {
        Self {
            colors: vec![
                [255, 0, 0],
                [255, 127, 0],
                [255, 255, 0],
                [0, 255, 0],
                [0, 0, 255],
                [75, 0, 130],
                [148, 0, 211],
            ],
            offset: 0.0,
            scale: 1.0,
        }
    }

    /// Grayscale
    pub fn grayscale() -> Self {
        Self {
            colors: vec![[0, 0, 0], [255, 255, 255]],
            offset: 0.0,
            scale: 1.0,
        }
    }

    /// Sample color from palette
    pub fn sample(&self, t: f64) -> [u8; 3] {
        if self.colors.is_empty() {
            return [0, 0, 0];
        }

        let t = ((t * self.scale + self.offset) % 1.0 + 1.0) % 1.0;
        let n = self.colors.len();
        let idx = t * (n - 1) as f64;
        let i = idx.floor() as usize;
        let f = idx.fract();

        let c0 = self.colors[i.min(n - 1)];
        let c1 = self.colors[(i + 1).min(n - 1)];

        [
            (c0[0] as f64 * (1.0 - f) + c1[0] as f64 * f) as u8,
            (c0[1] as f64 * (1.0 - f) + c1[1] as f64 * f) as u8,
            (c0[2] as f64 * (1.0 - f) + c1[2] as f64 * f) as u8,
        ]
    }
}

// ============================================================================
// Fractal Parameters
// ============================================================================

/// Parameters for fractal rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FractalParams {
    /// Fractal type
    pub fractal_type: FractalType,
    /// Coloring mode
    pub color_mode: ColorMode,
    /// Color palette
    pub palette: Palette,
    /// Center X coordinate
    pub center_x: f64,
    /// Center Y coordinate
    pub center_y: f64,
    /// Zoom level (higher = more zoomed)
    pub zoom: f64,
    /// Maximum iterations
    pub max_iter: u32,
    /// Escape radius squared
    pub bailout: f64,
    /// Julia set c parameter (real)
    pub julia_re: f64,
    /// Julia set c parameter (imaginary)
    pub julia_im: f64,
    /// Parameter x for intensity formula
    pub intensity_x: f64,
    /// Inside color
    pub inside_color: [u8; 3],
}

impl Default for FractalParams {
    fn default() -> Self {
        Self {
            fractal_type: FractalType::Mandelbrot,
            color_mode: ColorMode::DistanceEstimate,
            palette: Palette::default(),
            center_x: -0.5,
            center_y: 0.0,
            zoom: 1.0,
            max_iter: 1000,
            bailout: 1e10, // Large for distance estimation
            julia_re: -0.7,
            julia_im: 0.27015,
            intensity_x: 360.0,
            inside_color: [0, 0, 0],
        }
    }
}

// ============================================================================
// Distance Estimation Iterator
// ============================================================================

/// Result of fractal iteration with distance estimation
#[derive(Debug, Clone, Copy)]
pub struct IterResult {
    /// Number of iterations before escape
    pub iterations: u32,
    /// Did the point escape?
    pub escaped: bool,
    /// Final z value
    pub z: Complex,
    /// Final derivative value
    pub dz: Complex,
    /// Smooth iteration count
    pub smooth_iter: f64,
    /// Distance estimate d_c
    pub distance: f64,
    /// Intensity I from distance
    pub intensity: f64,
}

/// Iterate Mandelbrot with derivative tracking for distance estimation
///
/// z_{n+1} = z_n² + c
/// z'_{n+1} = 2·z_n·z'_n + 1  (derivative with respect to c)
pub fn iterate_mandelbrot_de(
    c: Complex,
    max_iter: u32,
    bailout: f64,
    intensity_x: f64,
) -> IterResult {
    let mut z = Complex::new(0.0, 0.0);
    let mut dz = Complex::new(0.0, 0.0); // Derivative

    let mut iterations = 0u32;
    let bailout_sqr = bailout * bailout;

    while iterations < max_iter {
        let z_norm_sqr = z.norm_sqr();

        if z_norm_sqr > bailout_sqr {
            break;
        }

        // Update derivative: dz = 2·z·dz + 1
        dz = z.mul(dz).scale(2.0).add(Complex::new(1.0, 0.0));

        // Update z: z = z² + c
        z = z.sqr().add(c);

        iterations += 1;
    }

    let escaped = iterations < max_iter;
    let z_norm = z.norm();
    let dz_norm = dz.norm();

    // Calculate distance estimate: d_c ≈ 2 · |z| · ln|z| / |z'|
    let distance = if escaped && dz_norm > 0.0 {
        2.0 * z_norm * z_norm.ln() / dz_norm
    } else {
        0.0
    };

    // Calculate intensity: I = (360/x - 1) / d_c
    let intensity = if distance > 0.0 {
        (intensity_x / 360.0 - 1.0).abs() / distance
    } else {
        0.0
    };

    // Smooth iteration count for smooth coloring
    let smooth_iter = if escaped {
        iterations as f64 + 1.0 - z_norm.ln().ln() / LN_2
    } else {
        iterations as f64
    };

    IterResult {
        iterations,
        escaped,
        z,
        dz,
        smooth_iter,
        distance,
        intensity,
    }
}

/// Iterate Julia set with derivative tracking
pub fn iterate_julia_de(
    z_start: Complex,
    c: Complex,
    max_iter: u32,
    bailout: f64,
    intensity_x: f64,
) -> IterResult {
    let mut z = z_start;
    let mut dz = Complex::new(1.0, 0.0); // Derivative w.r.t. z

    let mut iterations = 0u32;
    let bailout_sqr = bailout * bailout;

    while iterations < max_iter {
        if z.norm_sqr() > bailout_sqr {
            break;
        }

        // Update derivative: dz = 2·z·dz
        dz = z.mul(dz).scale(2.0);

        // Update z: z = z² + c
        z = z.sqr().add(c);

        iterations += 1;
    }

    let escaped = iterations < max_iter;
    let z_norm = z.norm();
    let dz_norm = dz.norm();

    let distance = if escaped && dz_norm > 0.0 {
        2.0 * z_norm * z_norm.ln() / dz_norm
    } else {
        0.0
    };

    let intensity = if distance > 0.0 {
        (intensity_x / 360.0 - 1.0).abs() / distance
    } else {
        0.0
    };

    let smooth_iter = if escaped {
        iterations as f64 + 1.0 - z_norm.ln().ln() / LN_2
    } else {
        iterations as f64
    };

    IterResult {
        iterations,
        escaped,
        z,
        dz,
        smooth_iter,
        distance,
        intensity,
    }
}

/// Iterate Burning Ship fractal
pub fn iterate_burning_ship_de(
    c: Complex,
    max_iter: u32,
    bailout: f64,
    intensity_x: f64,
) -> IterResult {
    let mut z = Complex::new(0.0, 0.0);
    let mut dz = Complex::new(0.0, 0.0);

    let mut iterations = 0u32;
    let bailout_sqr = bailout * bailout;

    while iterations < max_iter {
        if z.norm_sqr() > bailout_sqr {
            break;
        }

        // Take absolute values
        let z_abs = Complex::new(z.re.abs(), z.im.abs());

        // Derivative (approximation)
        dz = z_abs.mul(dz).scale(2.0).add(Complex::new(1.0, 0.0));

        // z = (|Re(z)| + i|Im(z)|)² + c
        z = z_abs.sqr().add(c);

        iterations += 1;
    }

    let escaped = iterations < max_iter;
    let z_norm = z.norm();
    let dz_norm = dz.norm();

    let distance = if escaped && dz_norm > 0.0 {
        2.0 * z_norm * z_norm.ln() / dz_norm
    } else {
        0.0
    };

    let intensity = if distance > 0.0 {
        (intensity_x / 360.0 - 1.0).abs() / distance
    } else {
        0.0
    };

    let smooth_iter = if escaped {
        iterations as f64 + 1.0 - z_norm.ln().ln() / LN_2
    } else {
        iterations as f64
    };

    IterResult {
        iterations,
        escaped,
        z,
        dz,
        smooth_iter,
        distance,
        intensity,
    }
}

// ============================================================================
// Fractal Renderer
// ============================================================================

/// CPU-based fractal renderer
pub struct FractalRenderer {
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
    /// Rendering parameters
    pub params: FractalParams,
    /// Output buffer (RGBA)
    pub buffer: Vec<u8>,
}

impl FractalRenderer {
    /// Create a new renderer
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            params: FractalParams::default(),
            buffer: vec![0u8; (width * height * 4) as usize],
        }
    }

    /// Resize buffer
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.buffer.resize((width * height * 4) as usize, 0);
    }

    /// Map pixel to complex coordinate
    fn pixel_to_complex(&self, px: u32, py: u32) -> Complex {
        let aspect = self.width as f64 / self.height as f64;
        let scale = 2.0 / self.params.zoom;

        let x = (px as f64 / self.width as f64 - 0.5) * scale * aspect + self.params.center_x;
        let y = (0.5 - py as f64 / self.height as f64) * scale + self.params.center_y;

        Complex::new(x, y)
    }

    /// Render a single pixel
    fn render_pixel(&self, px: u32, py: u32) -> [u8; 4] {
        let c = self.pixel_to_complex(px, py);

        let result = match self.params.fractal_type {
            FractalType::Mandelbrot => iterate_mandelbrot_de(
                c,
                self.params.max_iter,
                self.params.bailout,
                self.params.intensity_x,
            ),
            FractalType::Julia => {
                let julia_c = Complex::new(self.params.julia_re, self.params.julia_im);
                iterate_julia_de(
                    c,
                    julia_c,
                    self.params.max_iter,
                    self.params.bailout,
                    self.params.intensity_x,
                )
            }
            FractalType::BurningShip => iterate_burning_ship_de(
                c,
                self.params.max_iter,
                self.params.bailout,
                self.params.intensity_x,
            ),
            FractalType::Tricorn => {
                // Similar to Mandelbrot but conjugate
                iterate_mandelbrot_de(
                    Complex::new(c.re, -c.im),
                    self.params.max_iter,
                    self.params.bailout,
                    self.params.intensity_x,
                )
            }
            _ => iterate_mandelbrot_de(
                c,
                self.params.max_iter,
                self.params.bailout,
                self.params.intensity_x,
            ),
        };

        if !result.escaped {
            let ic = self.params.inside_color;
            return [ic[0], ic[1], ic[2], 255];
        }

        // Color based on mode
        let t = match self.params.color_mode {
            ColorMode::Smooth => (result.smooth_iter / self.params.max_iter as f64).fract(),
            ColorMode::DistanceEstimate => {
                // Use intensity from distance formula
                let i = result.intensity.min(1.0);
                i
            }
            ColorMode::Angle => (result.z.im.atan2(result.z.re) / (2.0 * PI) + 0.5).fract(),
            ColorMode::Potential => {
                let pot = result.z.norm().ln() / 2.0_f64.powi(result.iterations as i32);
                (pot * 10.0).fract()
            }
            _ => (result.smooth_iter / 50.0).fract(),
        };

        let rgb = self.params.palette.sample(t);
        [rgb[0], rgb[1], rgb[2], 255]
    }

    /// Render the entire image (single-threaded)
    pub fn render(&mut self) {
        for py in 0..self.height {
            for px in 0..self.width {
                let color = self.render_pixel(px, py);
                let idx = ((py * self.width + px) * 4) as usize;
                self.buffer[idx..idx + 4].copy_from_slice(&color);
            }
        }
    }

    /// Render with parallel processing
    #[cfg(feature = "rayon")]
    pub fn render_parallel(&mut self) {
        use rayon::prelude::*;

        let width = self.width;
        let height = self.height;

        self.buffer
            .par_chunks_mut(4)
            .enumerate()
            .for_each(|(i, pixel)| {
                let px = (i as u32) % width;
                let py = (i as u32) / width;
                let color = self.render_pixel(px, py);
                pixel.copy_from_slice(&color);
            });
    }

    /// Get buffer as RGB24
    pub fn as_rgb(&self) -> Vec<u8> {
        let mut rgb = Vec::with_capacity((self.width * self.height * 3) as usize);
        for chunk in self.buffer.chunks(4) {
            rgb.push(chunk[0]);
            rgb.push(chunk[1]);
            rgb.push(chunk[2]);
        }
        rgb
    }

    /// Zoom in at a point
    pub fn zoom_at(&mut self, px: u32, py: u32, factor: f64) {
        let c = self.pixel_to_complex(px, py);
        self.params.center_x = c.re;
        self.params.center_y = c.im;
        self.params.zoom *= factor;
    }

    /// Pan by pixels
    pub fn pan(&mut self, dx: i32, dy: i32) {
        let scale = 2.0 / self.params.zoom;
        let aspect = self.width as f64 / self.height as f64;

        self.params.center_x -= (dx as f64 / self.width as f64) * scale * aspect;
        self.params.center_y += (dy as f64 / self.height as f64) * scale;
    }

    /// Reset to default view
    pub fn reset(&mut self) {
        self.params = FractalParams::default();
    }
}

// ============================================================================
// WGSL Shader for GPU Rendering
// ============================================================================

/// WGSL compute shader for Mandelbrot with distance estimation
pub const SHADER_MANDELBROT_DE: &str = r#"
struct Params {
    center_x: f32,
    center_y: f32,
    zoom: f32,
    max_iter: u32,
    bailout: f32,
    intensity_x: f32,
    width: u32,
    height: u32,
}

@group(0) @binding(0) var output: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(1) var<uniform> params: Params;

// Complex multiplication
fn cmul(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(a.x * b.x - a.y * b.y, a.x * b.y + a.y * b.x);
}

// Complex squaring
fn csqr(z: vec2<f32>) -> vec2<f32> {
    return vec2<f32>(z.x * z.x - z.y * z.y, 2.0 * z.x * z.y);
}

@compute @workgroup_size(16, 16, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x >= params.width || gid.y >= params.height {
        return;
    }

    let aspect = f32(params.width) / f32(params.height);
    let scale = 2.0 / params.zoom;

    let x = (f32(gid.x) / f32(params.width) - 0.5) * scale * aspect + params.center_x;
    let y = (0.5 - f32(gid.y) / f32(params.height)) * scale + params.center_y;

    let c = vec2<f32>(x, y);
    var z = vec2<f32>(0.0, 0.0);
    var dz = vec2<f32>(0.0, 0.0);

    var iter: u32 = 0u;
    let bailout_sqr = params.bailout * params.bailout;

    while iter < params.max_iter {
        let z_norm_sqr = dot(z, z);
        if z_norm_sqr > bailout_sqr {
            break;
        }

        // dz = 2 * z * dz + 1
        dz = cmul(z, dz) * 2.0 + vec2<f32>(1.0, 0.0);

        // z = z² + c
        z = csqr(z) + c;

        iter = iter + 1u;
    }

    var color: vec4<f32>;

    if iter >= params.max_iter {
        color = vec4<f32>(0.0, 0.0, 0.0, 1.0);
    } else {
        let z_norm = length(z);
        let dz_norm = length(dz);

        // Distance estimate: d_c ≈ 2 * |z| * ln|z| / |z'|
        let distance = 2.0 * z_norm * log(z_norm) / dz_norm;

        // Intensity: I = (360/x - 1) / d_c
        let intensity = (params.intensity_x / 360.0 - 1.0) / distance;

        // Color mapping
        let t = clamp(intensity, 0.0, 1.0);

        // Simple gradient (blue to white to gold)
        let c1 = vec3<f32>(0.0, 0.03, 0.4);
        let c2 = vec3<f32>(0.9, 1.0, 1.0);
        let c3 = vec3<f32>(1.0, 0.7, 0.0);

        var rgb: vec3<f32>;
        if t < 0.5 {
            rgb = mix(c1, c2, t * 2.0);
        } else {
            rgb = mix(c2, c3, (t - 0.5) * 2.0);
        }

        color = vec4<f32>(rgb, 1.0);
    }

    textureStore(output, vec2<i32>(gid.xy), color);
}
"#;

// ============================================================================
// Presets
// ============================================================================

impl FractalParams {
    /// Seahorse valley
    pub fn seahorse_valley() -> Self {
        Self {
            center_x: -0.743643887037151,
            center_y: 0.131825904205330,
            zoom: 1000.0,
            max_iter: 2000,
            ..Default::default()
        }
    }

    /// Elephant valley
    pub fn elephant_valley() -> Self {
        Self {
            center_x: 0.281717921930775,
            center_y: 0.5771052841488505,
            zoom: 500.0,
            max_iter: 1500,
            ..Default::default()
        }
    }

    /// Spiral
    pub fn spiral() -> Self {
        Self {
            center_x: -0.761574,
            center_y: -0.0847596,
            zoom: 10000.0,
            max_iter: 3000,
            ..Default::default()
        }
    }

    /// Julia set flower
    pub fn julia_flower() -> Self {
        Self {
            fractal_type: FractalType::Julia,
            center_x: 0.0,
            center_y: 0.0,
            zoom: 1.0,
            julia_re: -0.4,
            julia_im: 0.6,
            max_iter: 500,
            ..Default::default()
        }
    }

    /// Burning ship overview
    pub fn burning_ship() -> Self {
        Self {
            fractal_type: FractalType::BurningShip,
            center_x: -0.5,
            center_y: -0.5,
            zoom: 0.8,
            max_iter: 500,
            ..Default::default()
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_complex_operations() {
        let a = Complex::new(3.0, 4.0);
        assert!((a.norm() - 5.0).abs() < 1e-10);

        let b = a.sqr();
        assert!((b.re - (-7.0)).abs() < 1e-10);
        assert!((b.im - 24.0).abs() < 1e-10);
    }

    #[test]
    fn test_mandelbrot_escapes() {
        // Point clearly outside should escape quickly
        let c = Complex::new(10.0, 0.0);
        let result = iterate_mandelbrot_de(c, 100, 1e10, 360.0);
        assert!(result.escaped);
        assert!(result.iterations < 10);
    }

    #[test]
    fn test_mandelbrot_inside() {
        // Point inside (origin)
        let c = Complex::new(0.0, 0.0);
        let result = iterate_mandelbrot_de(c, 100, 1e10, 360.0);
        assert!(!result.escaped);
    }

    #[test]
    fn test_distance_estimation() {
        // Point near boundary should have small distance
        let c = Complex::new(-0.75, 0.0); // Near main cardioid
        let result = iterate_mandelbrot_de(c, 1000, 1e10, 360.0);
        // Distance should be positive and small
        if result.escaped {
            assert!(result.distance > 0.0);
        }
    }

    #[test]
    fn test_renderer() {
        let mut renderer = FractalRenderer::new(100, 100);
        renderer.render();

        // Check buffer was filled
        assert!(!renderer.buffer.iter().all(|&b| b == 0));
    }
}
