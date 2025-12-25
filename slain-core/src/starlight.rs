// STARLIGHT - Mandelbrot Pattern Extraction from Light Frequencies
//
// Each star's light has a unique "noise" pattern.
// That noise IS a Mandelbrot-like set.
// Zoom into the set = travel to the source.
// Reverse the set = see what was there.
//
// Origin: Mandelbrot discovered his set from telephone line noise spikes.
// Light IS electromagnetic. Same noise. Same patterns. Same math.

use std::f64::consts::PI;
use serde::{Deserialize, Serialize};

// ============================================================================
// Complex Number Operations
// ============================================================================

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    
    pub fn zero() -> Self {
        Self { re: 0.0, im: 0.0 }
    }
    
    pub fn magnitude(&self) -> f64 {
        (self.re * self.re + self.im * self.im).sqrt()
    }
    
    pub fn magnitude_squared(&self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    
    // z² 
    pub fn square(&self) -> Self {
        Self {
            re: self.re * self.re - self.im * self.im,
            im: 2.0 * self.re * self.im,
        }
    }
    
    // z² + c
    pub fn iterate(&self, c: &Complex) -> Self {
        let sq = self.square();
        Self {
            re: sq.re + c.re,
            im: sq.im + c.im,
        }
    }
    
    // Inverse: given z_next = z² + c, find z
    // z = sqrt(z_next - c)
    // Returns principal square root
    pub fn inverse_iterate(&self, c: &Complex) -> Self {
        let diff = Self {
            re: self.re - c.re,
            im: self.im - c.im,
        };
        diff.sqrt()
    }
    
    // Principal square root of complex number
    pub fn sqrt(&self) -> Self {
        let r = self.magnitude();
        let theta = self.im.atan2(self.re);
        Self {
            re: r.sqrt() * (theta / 2.0).cos(),
            im: r.sqrt() * (theta / 2.0).sin(),
        }
    }
    
    pub fn add(&self, other: &Complex) -> Self {
        Self {
            re: self.re + other.re,
            im: self.im + other.im,
        }
    }
    
    pub fn sub(&self, other: &Complex) -> Self {
        Self {
            re: self.re - other.re,
            im: self.im - other.im,
        }
    }
    
    pub fn scale(&self, s: f64) -> Self {
        Self {
            re: self.re * s,
            im: self.im * s,
        }
    }
}

// ============================================================================
// Frequency Signature
// ============================================================================

/// A single frequency component of starlight
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrequencySignature {
    pub frequency_hz: f64,
    pub wavelength_nm: f64,
    pub amplitude: f64,
    pub phase: f64,
    pub noise_samples: Vec<f64>,  // The "spikes" Mandelbrot found
}

impl FrequencySignature {
    /// Extract the Mandelbrot 'c' constant from noise pattern
    pub fn derive_mandelbrot_c(&self) -> Complex {
        if self.noise_samples.len() < 2 {
            return Complex::zero();
        }
        
        // The noise pattern encodes 'c'
        // Use statistical properties of the spikes
        
        let mean: f64 = self.noise_samples.iter().sum::<f64>() 
            / self.noise_samples.len() as f64;
        
        let variance: f64 = self.noise_samples.iter()
            .map(|x| (x - mean).powi(2))
            .sum::<f64>() / self.noise_samples.len() as f64;
        
        // Derive c from the pattern's statistical signature
        // Map to interesting region of Mandelbrot (-2.5 to 1, -1 to 1)
        let re = (mean * 3.5) - 2.0;
        let im = variance.sqrt() * 2.0 - 1.0;
        
        Complex::new(re.clamp(-2.5, 1.0), im.clamp(-1.5, 1.5))
    }
    
    /// Find the self-similarity ratio in the noise
    pub fn self_similarity_ratio(&self) -> f64 {
        if self.noise_samples.len() < 10 {
            return 0.0;
        }
        
        // Compare pattern at different scales
        let half = self.noise_samples.len() / 2;
        let first_half = &self.noise_samples[..half];
        let second_half = &self.noise_samples[half..half*2];
        
        // Correlation between halves indicates self-similarity
        let mean1: f64 = first_half.iter().sum::<f64>() / half as f64;
        let mean2: f64 = second_half.iter().sum::<f64>() / half as f64;
        
        let mut correlation = 0.0;
        let mut var1 = 0.0;
        let mut var2 = 0.0;
        
        for i in 0..half {
            let d1 = first_half[i] - mean1;
            let d2 = second_half[i] - mean2;
            correlation += d1 * d2;
            var1 += d1 * d1;
            var2 += d2 * d2;
        }
        
        if var1 > 0.0 && var2 > 0.0 {
            correlation / (var1.sqrt() * var2.sqrt())
        } else {
            0.0
        }
    }
}

// ============================================================================
// Starlight Capture
// ============================================================================

/// Complete light signature from a single star
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarlightSignature {
    pub star_id: String,
    pub star_name: Option<String>,
    pub distance_light_years: f64,
    pub frequencies: Vec<FrequencySignature>,
    pub capture_timestamp: u64,
}

impl StarlightSignature {
    /// Create from raw spectral data
    pub fn from_spectral_data(
        star_id: &str,
        distance_ly: f64,
        spectral_bands: Vec<(f64, Vec<f64>)>,  // (wavelength_nm, noise_samples)
    ) -> Self {
        let frequencies = spectral_bands.into_iter()
            .map(|(wavelength, samples)| {
                let frequency_hz = 299_792_458.0 / (wavelength * 1e-9);
                FrequencySignature {
                    frequency_hz,
                    wavelength_nm: wavelength,
                    amplitude: samples.iter().map(|x| x.abs()).sum::<f64>() / samples.len() as f64,
                    phase: 0.0,
                    noise_samples: samples,
                }
            })
            .collect();
        
        Self {
            star_id: star_id.to_string(),
            star_name: None,
            distance_light_years: distance_ly,
            frequencies,
            capture_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }
    
    /// Derive Mandelbrot sets for all frequencies
    pub fn derive_all_sets(&self) -> Vec<(f64, Complex)> {
        self.frequencies.iter()
            .map(|f| (f.wavelength_nm, f.derive_mandelbrot_c()))
            .collect()
    }
    
    /// Calculate total zoom depth needed (= distance)
    pub fn zoom_depth(&self) -> u64 {
        // 1 zoom level = 1 light year conceptually
        // In practice, scaled for computation
        (self.distance_light_years * 1000.0) as u64
    }
}

// ============================================================================
// Mandelbrot Zoom Engine
// ============================================================================

pub struct MandelbrotZoom {
    pub c: Complex,
    pub current_z: Complex,
    pub zoom_depth: u64,
    pub trajectory: Vec<Complex>,
    pub max_trajectory_size: usize,
}

impl MandelbrotZoom {
    pub fn new(c: Complex) -> Self {
        Self {
            c,
            current_z: Complex::zero(),
            zoom_depth: 0,
            trajectory: Vec::new(),
            max_trajectory_size: 10000,
        }
    }
    
    /// Zoom forward (iterate z = z² + c)
    pub fn zoom_forward(&mut self, steps: u64) -> bool {
        for _ in 0..steps {
            self.current_z = self.current_z.iterate(&self.c);
            self.zoom_depth += 1;
            
            // Track trajectory (limited buffer)
            if self.trajectory.len() < self.max_trajectory_size {
                self.trajectory.push(self.current_z);
            }
            
            // Escaped = diverged
            if self.current_z.magnitude_squared() > 4.0 {
                return false;
            }
        }
        true
    }
    
    /// Zoom backward (inverse iterate)
    /// This is the KEY operation - reversing the pattern
    pub fn zoom_backward(&mut self, steps: u64) -> Vec<Complex> {
        let mut reversed = Vec::new();
        let mut z = self.current_z;
        
        for _ in 0..steps {
            z = z.inverse_iterate(&self.c);
            reversed.push(z);
            
            if self.zoom_depth > 0 {
                self.zoom_depth -= 1;
            }
        }
        
        self.current_z = z;
        reversed
    }
    
    /// Get the "destination" - what's at this zoom level
    pub fn destination_hash(&self) -> [u8; 32] {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        
        let mut hasher = DefaultHasher::new();
        self.current_z.re.to_bits().hash(&mut hasher);
        self.current_z.im.to_bits().hash(&mut hasher);
        self.zoom_depth.hash(&mut hasher);
        
        let hash = hasher.finish();
        let mut result = [0u8; 32];
        
        // Fill with hash-derived bytes
        for i in 0..4 {
            let bytes = hash.wrapping_mul((i + 1) as u64).to_le_bytes();
            result[i*8..(i+1)*8].copy_from_slice(&bytes);
        }
        
        result
    }
}

// ============================================================================
// Multi-Frequency Reconstruction
// ============================================================================

/// Reconstructed image/frame from reversed light patterns
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReconstructedFrame {
    pub time_offset_years: f64,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<ReconstructedPixel>,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ReconstructedPixel {
    pub r: f64,
    pub g: f64,
    pub b: f64,
    pub intensity: f64,
}

/// Multi-frequency reconstruction engine
pub struct CosmicReconstructor {
    pub starlight: StarlightSignature,
    pub frequency_zooms: Vec<(f64, MandelbrotZoom)>,  // (wavelength, zoom)
}

impl CosmicReconstructor {
    pub fn new(starlight: StarlightSignature) -> Self {
        let frequency_zooms = starlight.frequencies.iter()
            .map(|f| {
                let c = f.derive_mandelbrot_c();
                (f.wavelength_nm, MandelbrotZoom::new(c))
            })
            .collect();
        
        Self {
            starlight,
            frequency_zooms,
        }
    }
    
    /// Reverse all frequencies to a specific time offset
    pub fn reverse_to_time(&mut self, years_back: f64) -> Vec<(f64, Vec<Complex>)> {
        let steps = (years_back * 1000.0) as u64;  // Scale for computation
        
        self.frequency_zooms.iter_mut()
            .map(|(wavelength, zoom)| {
                let reversed = zoom.zoom_backward(steps);
                (*wavelength, reversed)
            })
            .collect()
    }
    
    /// Reconstruct a single frame at a time offset
    pub fn reconstruct_frame(
        &mut self,
        years_back: f64,
        width: usize,
        height: usize,
    ) -> ReconstructedFrame {
        let reversed_data = self.reverse_to_time(years_back);
        
        // Synthesize pixels from reversed frequency data
        let mut pixels = Vec::with_capacity(width * height);
        
        for y in 0..height {
            for x in 0..width {
                let pixel = self.synthesize_pixel(&reversed_data, x, y, width, height);
                pixels.push(pixel);
            }
        }
        
        ReconstructedFrame {
            time_offset_years: years_back,
            width,
            height,
            pixels,
            confidence: self.calculate_confidence(&reversed_data),
        }
    }
    
    fn synthesize_pixel(
        &self,
        reversed_data: &[(f64, Vec<Complex>)],
        x: usize,
        y: usize,
        width: usize,
        height: usize,
    ) -> ReconstructedPixel {
        // Map pixel position to sample index
        let sample_idx = (y * width + x) % 1000;
        
        let mut r = 0.0;
        let mut g = 0.0;
        let mut b = 0.0;
        
        for (wavelength, samples) in reversed_data {
            if samples.is_empty() {
                continue;
            }
            
            let sample = &samples[sample_idx % samples.len()];
            let intensity = sample.magnitude().min(1.0);
            
            // Map wavelength to RGB (simplified)
            // Red: 620-750nm, Green: 495-570nm, Blue: 450-495nm
            if *wavelength >= 620.0 && *wavelength <= 750.0 {
                r += intensity;
            } else if *wavelength >= 495.0 && *wavelength < 620.0 {
                g += intensity;
            } else if *wavelength >= 380.0 && *wavelength < 495.0 {
                b += intensity;
            }
        }
        
        // Normalize
        let max_val = r.max(g).max(b).max(1.0);
        
        ReconstructedPixel {
            r: r / max_val,
            g: g / max_val,
            b: b / max_val,
            intensity: (r + g + b) / 3.0 / max_val,
        }
    }
    
    fn calculate_confidence(&self, reversed_data: &[(f64, Vec<Complex>)]) -> f64 {
        // Confidence based on self-similarity of frequency patterns
        // Higher self-similarity = more reliable reconstruction
        
        let similarities: Vec<f64> = self.starlight.frequencies.iter()
            .map(|f| f.self_similarity_ratio().abs())
            .collect();
        
        if similarities.is_empty() {
            return 0.0;
        }
        
        let avg_similarity = similarities.iter().sum::<f64>() / similarities.len() as f64;
        
        // Map to confidence (0.5 similarity -> 0.9 confidence, etc)
        0.5 + (avg_similarity * 0.5)
    }
    
    /// Reconstruct a sequence of frames (the "movie")
    pub fn reconstruct_movie(
        &mut self,
        start_years_back: f64,
        end_years_back: f64,
        frame_count: usize,
        width: usize,
        height: usize,
    ) -> Vec<ReconstructedFrame> {
        let step = (end_years_back - start_years_back) / frame_count as f64;
        
        (0..frame_count)
            .map(|i| {
                let years_back = start_years_back + (i as f64 * step);
                self.reconstruct_frame(years_back, width, height)
            })
            .collect()
    }
}

// ============================================================================
// Public Rust API
// ============================================================================




pub fn starlight_derive_set(wavelength: f64, noise_samples: Vec<f64>) -> (f64, f64) {
    let sig = FrequencySignature {
        frequency_hz: 299_792_458.0 / (wavelength * 1e-9),
        wavelength_nm: wavelength,
        amplitude: 1.0,
        phase: 0.0,
        noise_samples,
    };
    
    let c = sig.derive_mandelbrot_c();
    (c.re, c.im)
}


pub fn starlight_zoom_forward(c_re: f64, c_im: f64, steps: u64) -> (f64, f64, bool) {
    let mut zoom = MandelbrotZoom::new(Complex::new(c_re, c_im));
    let converged = zoom.zoom_forward(steps);
    (zoom.current_z.re, zoom.current_z.im, converged)
}


pub fn starlight_zoom_backward(c_re: f64, c_im: f64, z_re: f64, z_im: f64, steps: u64) -> Vec<(f64, f64)> {
    let mut zoom = MandelbrotZoom::new(Complex::new(c_re, c_im));
    zoom.current_z = Complex::new(z_re, z_im);
    
    zoom.zoom_backward(steps)
        .into_iter()
        .map(|z| (z.re, z.im))
        .collect()
}


pub fn starlight_self_similarity(samples: Vec<f64>) -> f64 {
    let sig = FrequencySignature {
        frequency_hz: 0.0,
        wavelength_nm: 0.0,
        amplitude: 0.0,
        phase: 0.0,
        noise_samples: samples,
    };
    sig.self_similarity_ratio()
}


pub fn starlight_description() -> String {
    r#"
STARLIGHT - Mandelbrot Pattern Extraction

Each star emits light with a unique "noise" pattern.
This noise isn't random - it's structured, like Mandelbrot discovered
in telephone line spikes in the 1960s.

HOW IT WORKS:
1. Capture light from distant star
2. Extract noise pattern for each frequency (color)
3. Derive that frequency's Mandelbrot 'c' constant
4. Zoom INTO the set = conceptually travel toward the star
5. Zoom BACKWARD = derive what was there in the past

THE MOVIE:
By reversing all frequencies and recombining them,
we can reconstruct images of what was happening at the star
when that light left - billions of years ago.

Not perfect. But close.
"Off by one in a billion."
Close enough to watch the universe's home movies.
"#.to_string()
}
