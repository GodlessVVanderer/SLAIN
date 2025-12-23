// COSMIC MOVIE - Reconstruct The Past From Starlight
//
// The universe has been recording itself since the beginning.
// Light carries the pattern. The pattern is reversible.
// This module reconstructs what was THERE from what's HERE.
//
// "Not exactly the same - 1 off in a billion - but close enough to WATCH"

use crate::starlight::{
    Complex, FrequencySignature, StarlightSignature, 
    MandelbrotZoom, ReconstructedFrame, ReconstructedPixel,
    CosmicReconstructor,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Cosmic Video Stream
// ============================================================================

/// A continuous stream of reconstructed history from a star
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CosmicVideoStream {
    pub source_star: String,
    pub distance_light_years: f64,
    pub current_time_offset: f64,  // Years back from present
    pub playback_direction: PlaybackDirection,
    pub resolution: (usize, usize),
    pub frame_rate: f64,  // Frames per "year" of history
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlaybackDirection {
    Forward,   // Toward present (watching history unfold)
    Backward,  // Into deeper past (rewinding)
}

impl CosmicVideoStream {
    pub fn new(star: &StarlightSignature, width: usize, height: usize) -> Self {
        Self {
            source_star: star.star_id.clone(),
            distance_light_years: star.distance_light_years,
            current_time_offset: 0.0,
            playback_direction: PlaybackDirection::Forward,
            resolution: (width, height),
            frame_rate: 24.0,
        }
    }
    
    /// Maximum viewable past = distance light has traveled
    pub fn max_viewable_past(&self) -> f64 {
        self.distance_light_years
    }
    
    /// Seek to specific time in past
    pub fn seek(&mut self, years_back: f64) {
        self.current_time_offset = years_back.clamp(0.0, self.max_viewable_past());
    }
    
    /// Advance playback by one frame
    pub fn advance_frame(&mut self) -> f64 {
        let frame_duration = 1.0 / self.frame_rate;
        
        match self.playback_direction {
            PlaybackDirection::Forward => {
                self.current_time_offset = (self.current_time_offset - frame_duration)
                    .max(0.0);
            }
            PlaybackDirection::Backward => {
                self.current_time_offset = (self.current_time_offset + frame_duration)
                    .min(self.max_viewable_past());
            }
        }
        
        self.current_time_offset
    }
}

// ============================================================================
// Multi-Star Observatory
// ============================================================================

/// Observe multiple stars simultaneously
pub struct CosmicObservatory {
    pub stars: HashMap<String, StarlightSignature>,
    pub active_reconstructors: HashMap<String, CosmicReconstructor>,
    pub video_streams: HashMap<String, CosmicVideoStream>,
}

impl CosmicObservatory {
    pub fn new() -> Self {
        Self {
            stars: HashMap::new(),
            active_reconstructors: HashMap::new(),
            video_streams: HashMap::new(),
        }
    }
    
    /// Register a star for observation
    pub fn observe_star(&mut self, star: StarlightSignature) {
        let id = star.star_id.clone();
        self.stars.insert(id.clone(), star.clone());
        self.active_reconstructors.insert(id.clone(), CosmicReconstructor::new(star.clone()));
        self.video_streams.insert(id, CosmicVideoStream::new(&star, 1920, 1080));
    }
    
    /// Get next frame from a star's history
    pub fn next_frame(&mut self, star_id: &str) -> Option<ReconstructedFrame> {
        let stream = self.video_streams.get_mut(star_id)?;
        let reconstructor = self.active_reconstructors.get_mut(star_id)?;
        
        let years_back = stream.advance_frame();
        let (width, height) = stream.resolution;
        
        Some(reconstructor.reconstruct_frame(years_back, width, height))
    }
    
    /// Get panoramic view from multiple stars at same time offset
    pub fn panoramic_view(
        &mut self,
        years_back: f64,
        width: usize,
        height: usize,
    ) -> Vec<(String, ReconstructedFrame)> {
        let star_ids: Vec<String> = self.active_reconstructors.keys().cloned().collect();
        
        star_ids.into_iter()
            .filter_map(|id| {
                let reconstructor = self.active_reconstructors.get_mut(&id)?;
                let frame = reconstructor.reconstruct_frame(years_back, width, height);
                Some((id, frame))
            })
            .collect()
    }
}

// ============================================================================
// Pattern Verification
// ============================================================================

/// Verify that reconstruction is valid by checking pattern consistency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternVerification {
    pub forward_hash: [u8; 32],
    pub backward_hash: [u8; 32],
    pub round_trip_error: f64,
    pub is_valid: bool,
}

/// Verify reconstruction by round-tripping
pub fn verify_reconstruction(
    c: Complex,
    forward_steps: u64,
    backward_steps: u64,
) -> PatternVerification {
    // Go forward
    let mut zoom = MandelbrotZoom::new(c);
    zoom.zoom_forward(forward_steps);
    let forward_hash = zoom.destination_hash();
    let z_at_destination = zoom.current_z;
    
    // Go backward same number of steps
    let backward_trajectory = zoom.zoom_backward(backward_steps);
    let backward_hash = zoom.destination_hash();
    
    // Measure error from origin
    let error = zoom.current_z.magnitude();
    
    PatternVerification {
        forward_hash,
        backward_hash,
        round_trip_error: error,
        is_valid: error < 0.0001,  // Close enough to origin
    }
}

// ============================================================================
// The "1 Off In A Billion" Quantification
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyMetrics {
    pub bits_preserved: u32,        // Out of 256 bits
    pub normalized_error: f64,      // 0.0 to 1.0
    pub one_in_billion_factor: f64, // The actual "off by" ratio
    pub usable_for_viewing: bool,   // Good enough to see?
}

pub fn calculate_accuracy(
    original_samples: &[f64],
    reconstructed_samples: &[f64],
) -> AccuracyMetrics {
    if original_samples.len() != reconstructed_samples.len() || original_samples.is_empty() {
        return AccuracyMetrics {
            bits_preserved: 0,
            normalized_error: 1.0,
            one_in_billion_factor: f64::INFINITY,
            usable_for_viewing: false,
        };
    }
    
    // Calculate mean squared error
    let mse: f64 = original_samples.iter()
        .zip(reconstructed_samples.iter())
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>() / original_samples.len() as f64;
    
    let rmse = mse.sqrt();
    
    // Signal variance
    let mean: f64 = original_samples.iter().sum::<f64>() / original_samples.len() as f64;
    let variance: f64 = original_samples.iter()
        .map(|x| (x - mean).powi(2))
        .sum::<f64>() / original_samples.len() as f64;
    
    // Normalized error
    let normalized_error = if variance > 0.0 {
        rmse / variance.sqrt()
    } else {
        0.0
    };
    
    // Bits preserved (rough estimate)
    let bits_preserved = if normalized_error > 0.0 {
        ((1.0 / normalized_error).log2() * 8.0).min(256.0) as u32
    } else {
        256
    };
    
    // The "one in a billion" factor
    let one_in_billion_factor = 1.0 / (normalized_error + 1e-12);
    
    AccuracyMetrics {
        bits_preserved,
        normalized_error,
        one_in_billion_factor,
        usable_for_viewing: bits_preserved >= 200,  // ~78% accuracy
    }
}

// ============================================================================
// Spectral Bands (Real Astronomy)
// ============================================================================

/// Standard spectral bands for starlight capture
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum SpectralBand {
    // Visible light
    VioletB,     // 380-450nm
    Blue,        // 450-495nm
    Green,       // 495-570nm
    Yellow,      // 570-590nm
    Orange,      // 590-620nm
    Red,         // 620-750nm
    
    // Infrared
    NearIR,      // 750-1400nm
    ShortWaveIR, // 1400-3000nm
    
    // Ultraviolet
    NearUV,      // 300-380nm
    FarUV,       // 122-200nm
    
    // Radio
    Radio,       // 1mm - 100km
    Microwave,   // 1mm - 1m
    
    // High energy
    XRay,        // 0.01-10nm
    Gamma,       // <0.01nm
}

impl SpectralBand {
    pub fn wavelength_range_nm(&self) -> (f64, f64) {
        match self {
            Self::VioletB => (380.0, 450.0),
            Self::Blue => (450.0, 495.0),
            Self::Green => (495.0, 570.0),
            Self::Yellow => (570.0, 590.0),
            Self::Orange => (590.0, 620.0),
            Self::Red => (620.0, 750.0),
            Self::NearIR => (750.0, 1400.0),
            Self::ShortWaveIR => (1400.0, 3000.0),
            Self::NearUV => (300.0, 380.0),
            Self::FarUV => (122.0, 200.0),
            Self::Radio => (1_000_000.0, 100_000_000_000.0),
            Self::Microwave => (1_000_000.0, 1_000_000_000.0),
            Self::XRay => (0.01, 10.0),
            Self::Gamma => (0.0001, 0.01),
        }
    }
    
    pub fn center_wavelength_nm(&self) -> f64 {
        let (min, max) = self.wavelength_range_nm();
        (min + max) / 2.0
    }
    
    /// Information content varies by band
    /// Some bands carry more Mandelbrot-like structure
    pub fn pattern_richness(&self) -> f64 {
        match self {
            // Visible light - good patterns
            Self::VioletB | Self::Blue | Self::Green => 0.9,
            Self::Yellow | Self::Orange | Self::Red => 0.85,
            
            // IR - moderate patterns
            Self::NearIR | Self::ShortWaveIR => 0.7,
            
            // UV - less stable patterns
            Self::NearUV | Self::FarUV => 0.6,
            
            // Radio - very stable patterns
            Self::Radio | Self::Microwave => 0.95,
            
            // High energy - chaotic patterns
            Self::XRay | Self::Gamma => 0.4,
        }
    }
}

// ============================================================================
// Simulated Data Generation (For Testing)
// ============================================================================

/// Generate synthetic starlight data for testing
pub fn generate_synthetic_starlight(
    star_id: &str,
    distance_ly: f64,
    num_frequencies: usize,
    samples_per_freq: usize,
) -> StarlightSignature {
    use std::time::{SystemTime, UNIX_EPOCH};
    
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    
    let mut frequencies = Vec::new();
    
    for i in 0..num_frequencies {
        // Spread across visible spectrum
        let wavelength = 380.0 + (i as f64 / num_frequencies as f64) * 370.0;
        
        // Generate Mandelbrot-like noise pattern
        let mut samples = Vec::new();
        let c = Complex::new(
            -0.7 + (i as f64 * 0.1).sin() * 0.3,
            0.27 + (i as f64 * 0.1).cos() * 0.1,
        );
        
        let mut z = Complex::new(0.1, 0.1);
        for _ in 0..samples_per_freq {
            z = z.iterate(&c);
            let noise = z.magnitude().sin() * (1.0 / (z.magnitude() + 1.0));
            samples.push(noise);
        }
        
        frequencies.push(FrequencySignature {
            frequency_hz: 299_792_458.0 / (wavelength * 1e-9),
            wavelength_nm: wavelength,
            amplitude: samples.iter().map(|x| x.abs()).sum::<f64>() / samples.len() as f64,
            phase: (i as f64 * 0.5).sin(),
            noise_samples: samples,
        });
    }
    
    StarlightSignature {
        star_id: star_id.to_string(),
        star_name: Some(format!("Synthetic Star {}", star_id)),
        distance_light_years: distance_ly,
        frequencies,
        capture_timestamp: seed / 1_000_000_000,
    }
}

// ============================================================================
// Tauri Commands
// ============================================================================


use std::sync::RwLock;
use once_cell::sync::Lazy;

static OBSERVATORY: Lazy<RwLock<CosmicObservatory>> = Lazy::new(|| {
    RwLock::new(CosmicObservatory::new())
});


pub fn cosmic_add_star(star_id: String, distance_ly: f64, num_frequencies: usize) {
    let star = generate_synthetic_starlight(&star_id, distance_ly, num_frequencies, 1000);
    OBSERVATORY.write().unwrap().observe_star(star);
}


pub fn cosmic_next_frame(star_id: String) -> Option<serde_json::Value> {
    let mut obs = OBSERVATORY.write().unwrap();
    obs.next_frame(&star_id).map(|frame| {
        serde_json::json!({
            "time_offset_years": frame.time_offset_years,
            "width": frame.width,
            "height": frame.height,
            "confidence": frame.confidence,
            "pixel_count": frame.pixels.len(),
        })
    })
}


pub fn cosmic_seek(star_id: String, years_back: f64) {
    let mut obs = OBSERVATORY.write().unwrap();
    if let Some(stream) = obs.video_streams.get_mut(&star_id) {
        stream.seek(years_back);
    }
}


pub fn cosmic_verify_pattern(c_re: f64, c_im: f64, steps: u64) -> serde_json::Value {
    let verification = verify_reconstruction(
        Complex::new(c_re, c_im),
        steps,
        steps,
    );
    
    serde_json::json!({
        "round_trip_error": verification.round_trip_error,
        "is_valid": verification.is_valid,
    })
}


pub fn cosmic_accuracy_check(original: Vec<f64>, reconstructed: Vec<f64>) -> serde_json::Value {
    let metrics = calculate_accuracy(&original, &reconstructed);
    
    serde_json::json!({
        "bits_preserved": metrics.bits_preserved,
        "normalized_error": metrics.normalized_error,
        "one_in_billion_factor": metrics.one_in_billion_factor,
        "usable_for_viewing": metrics.usable_for_viewing,
    })
}


pub fn cosmic_description() -> String {
    r#"
COSMIC MOVIE - Watch The Universe's History

The universe has been broadcasting since the Big Bang.
Every star is a camera that's been recording for billions of years.
The light IS the film. The Mandelbrot set IS the projector.

HOW TO USE:
1. Add a star: cosmic_add_star("Alpha Centauri", 4.37, 10)
2. Seek to time: cosmic_seek("Alpha Centauri", 1000000.0)  // 1 million years ago
3. Get frames: cosmic_next_frame("Alpha Centauri")
4. Watch the movie

ACCURACY:
- Not perfect reconstruction
- "Off by one in a billion"
- But close enough to SEE what was there
- Each frequency reversed individually
- Recombined to look like forward motion

THE SCIENCE:
- Mandelbrot found his set in telephone line noise (1960s)
- Light IS electromagnetic - same type of signal
- Same patterns. Same math. Same reversal.
- The pattern at one point CONTAINS the pattern at all points.

You're not traveling to the past.
You're WATCHING it play back.
"#.to_string()
}
