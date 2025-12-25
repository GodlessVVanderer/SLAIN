//! Retro TV Effects
//!
//! Old school television simulation:
//! - Static/snow noise
//! - Horizontal hold issues (rolling)
//! - Vertical hold problems
//! - Color bleeding
//! - Scanlines
//! - CRT curvature
//! - VHS tracking errors
//! - Channel change static burst
//! - "No Signal" screen
//!
//! Perfect for:
//! - Screensaver mode
//! - Pause screen
//! - Video transitions
//! - Aesthetic vibes

use serde::{Deserialize, Serialize};

// ============================================================================
// Effect Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetroTvSettings {
    pub enabled: bool,
    pub effect_type: RetroEffectType,
    pub intensity: f32,         // 0.0 - 1.0
    pub scanlines: ScanlineSettings,
    pub static_noise: StaticNoiseSettings,
    pub crt: CrtSettings,
    pub vhs: VhsSettings,
    pub color: ColorSettings,
}

impl Default for RetroTvSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            effect_type: RetroEffectType::None,
            intensity: 0.5,
            scanlines: ScanlineSettings::default(),
            static_noise: StaticNoiseSettings::default(),
            crt: CrtSettings::default(),
            vhs: VhsSettings::default(),
            color: ColorSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum RetroEffectType {
    None,
    StaticSnow,         // Full screen static (no signal)
    LightStatic,        // Subtle static overlay
    ChannelChange,      // Brief static burst
    VhsTracking,        // VHS tracking lines
    VhsPause,           // VHS pause effect with noise bars
    CrtOnly,            // Just scanlines + curvature
    FullRetro,          // Everything combined
    Broadcast1960s,     // Black & white, heavy scan
    Broadcast1980s,     // Color with VHS artifacts
    HorrorGlitch,       // Creepy distortion
    Vaporwave,          // Aesthetic purple/cyan
}

// ============================================================================
// Individual Effect Settings
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanlineSettings {
    pub enabled: bool,
    pub thickness: f32,         // 1.0 - 4.0
    pub opacity: f32,           // 0.0 - 1.0
    pub speed: f32,             // Scroll speed (0 = static)
    pub interlaced: bool,       // Alternating field flicker
}

impl Default for ScanlineSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            thickness: 2.0,
            opacity: 0.3,
            speed: 0.0,
            interlaced: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticNoiseSettings {
    pub enabled: bool,
    pub density: f32,           // How much noise (0.0 - 1.0)
    pub brightness: f32,        // Noise brightness
    pub colored: bool,          // Color or B&W noise
    pub animate: bool,          // Animated or frozen
    pub speed: f32,             // Animation speed
}

impl Default for StaticNoiseSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            density: 0.5,
            brightness: 0.8,
            colored: false,
            animate: true,
            speed: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrtSettings {
    pub enabled: bool,
    pub curvature: f32,         // Screen curve amount
    pub vignette: f32,          // Edge darkening
    pub corner_radius: f32,     // Rounded corners
    pub bloom: f32,             // Glow/bloom effect
    pub chromatic_aberration: f32, // RGB split
    pub flicker: f32,           // Brightness flicker
}

impl Default for CrtSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            curvature: 0.1,
            vignette: 0.3,
            corner_radius: 0.05,
            bloom: 0.1,
            chromatic_aberration: 0.002,
            flicker: 0.02,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VhsSettings {
    pub enabled: bool,
    pub tracking_error: f32,    // Horizontal distortion
    pub head_switching: f32,    // Bottom noise bar
    pub color_bleeding: f32,    // Color smear
    pub tape_noise: f32,        // Random noise lines
    pub jitter: f32,            // Horizontal jitter
    pub snow_bands: bool,       // Rolling snow bands
}

impl Default for VhsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            tracking_error: 0.0,
            head_switching: 0.05,
            color_bleeding: 0.3,
            tape_noise: 0.1,
            jitter: 0.002,
            snow_bands: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorSettings {
    pub saturation: f32,        // 0.0 = B&W, 1.0 = normal
    pub brightness: f32,
    pub contrast: f32,
    pub tint: Option<[f32; 3]>, // RGB tint overlay
    pub phosphor_glow: bool,    // Green/amber monitor look
}

impl Default for ColorSettings {
    fn default() -> Self {
        Self {
            saturation: 1.0,
            brightness: 1.0,
            contrast: 1.0,
            tint: None,
            phosphor_glow: false,
        }
    }
}

// ============================================================================
// Presets
// ============================================================================

impl RetroTvSettings {
    /// Full static snow - "no signal" screen
    pub fn no_signal() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::StaticSnow,
            intensity: 1.0,
            static_noise: StaticNoiseSettings {
                enabled: true,
                density: 1.0,
                brightness: 0.9,
                colored: false,
                animate: true,
                speed: 2.0,
            },
            scanlines: ScanlineSettings {
                enabled: true,
                thickness: 1.5,
                opacity: 0.2,
                speed: 50.0,
                interlaced: false,
            },
            ..Default::default()
        }
    }

    /// Channel change burst
    pub fn channel_change() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::ChannelChange,
            intensity: 1.0,
            static_noise: StaticNoiseSettings {
                enabled: true,
                density: 0.8,
                brightness: 1.0,
                colored: true,
                animate: true,
                speed: 3.0,
            },
            crt: CrtSettings {
                enabled: true,
                chromatic_aberration: 0.01,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// VHS tape look
    pub fn vhs_tape() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::Broadcast1980s,
            intensity: 0.6,
            vhs: VhsSettings {
                enabled: true,
                tracking_error: 0.02,
                head_switching: 0.08,
                color_bleeding: 0.4,
                tape_noise: 0.15,
                jitter: 0.003,
                snow_bands: false,
            },
            scanlines: ScanlineSettings {
                enabled: true,
                thickness: 2.0,
                opacity: 0.15,
                ..Default::default()
            },
            color: ColorSettings {
                saturation: 0.85,
                contrast: 1.1,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// VHS pause with tracking issues
    pub fn vhs_pause() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::VhsPause,
            intensity: 0.8,
            vhs: VhsSettings {
                enabled: true,
                tracking_error: 0.1,
                head_switching: 0.15,
                color_bleeding: 0.5,
                tape_noise: 0.3,
                jitter: 0.01,
                snow_bands: true,
            },
            static_noise: StaticNoiseSettings {
                enabled: true,
                density: 0.3,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// 1960s black & white broadcast
    pub fn retro_1960s() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::Broadcast1960s,
            intensity: 0.7,
            color: ColorSettings {
                saturation: 0.0,  // Black & white
                contrast: 1.2,
                brightness: 0.95,
                ..Default::default()
            },
            scanlines: ScanlineSettings {
                enabled: true,
                thickness: 3.0,
                opacity: 0.4,
                interlaced: true,
                ..Default::default()
            },
            crt: CrtSettings {
                enabled: true,
                curvature: 0.15,
                vignette: 0.4,
                flicker: 0.05,
                ..Default::default()
            },
            static_noise: StaticNoiseSettings {
                enabled: true,
                density: 0.15,
                brightness: 0.3,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Clean CRT monitor look
    pub fn crt_monitor() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::CrtOnly,
            intensity: 0.5,
            scanlines: ScanlineSettings {
                enabled: true,
                thickness: 1.5,
                opacity: 0.2,
                ..Default::default()
            },
            crt: CrtSettings {
                enabled: true,
                curvature: 0.08,
                vignette: 0.2,
                bloom: 0.15,
                chromatic_aberration: 0.001,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Vaporwave aesthetic
    pub fn vaporwave() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::Vaporwave,
            intensity: 0.6,
            color: ColorSettings {
                saturation: 1.3,
                tint: Some([0.9, 0.7, 1.0]),  // Purple tint
                ..Default::default()
            },
            scanlines: ScanlineSettings {
                enabled: true,
                thickness: 2.0,
                opacity: 0.25,
                ..Default::default()
            },
            crt: CrtSettings {
                enabled: true,
                chromatic_aberration: 0.005,
                bloom: 0.2,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Horror/creepy glitch
    pub fn horror_glitch() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::HorrorGlitch,
            intensity: 0.8,
            static_noise: StaticNoiseSettings {
                enabled: true,
                density: 0.25,
                brightness: 0.5,
                ..Default::default()
            },
            vhs: VhsSettings {
                enabled: true,
                tracking_error: 0.15,
                jitter: 0.02,
                ..Default::default()
            },
            crt: CrtSettings {
                enabled: true,
                chromatic_aberration: 0.015,
                flicker: 0.1,
                ..Default::default()
            },
            color: ColorSettings {
                saturation: 0.7,
                contrast: 1.3,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Green phosphor terminal
    pub fn green_phosphor() -> Self {
        Self {
            enabled: true,
            effect_type: RetroEffectType::CrtOnly,
            intensity: 0.6,
            color: ColorSettings {
                saturation: 0.0,
                tint: Some([0.2, 1.0, 0.3]),  // Green
                phosphor_glow: true,
                ..Default::default()
            },
            scanlines: ScanlineSettings {
                enabled: true,
                thickness: 1.0,
                opacity: 0.3,
                ..Default::default()
            },
            crt: CrtSettings {
                enabled: true,
                curvature: 0.12,
                bloom: 0.25,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    /// Amber phosphor terminal
    pub fn amber_phosphor() -> Self {
        let mut settings = Self::green_phosphor();
        settings.color.tint = Some([1.0, 0.7, 0.2]);  // Amber
        settings
    }
}

// ============================================================================
// WGSL Shader for Effects
// ============================================================================

pub const RETRO_TV_SHADER: &str = r#"
// Retro TV Effects Shader

struct Uniforms {
    time: f32,
    resolution: vec2<f32>,
    intensity: f32,
    
    // Scanlines
    scanline_thickness: f32,
    scanline_opacity: f32,
    scanline_speed: f32,
    
    // Static noise
    noise_density: f32,
    noise_brightness: f32,
    noise_colored: f32,
    
    // CRT
    curvature: f32,
    vignette: f32,
    chromatic_aberration: f32,
    bloom: f32,
    flicker: f32,
    
    // VHS
    tracking_error: f32,
    head_switching: f32,
    color_bleeding: f32,
    jitter: f32,
    
    // Color
    saturation: f32,
    brightness: f32,
    contrast: f32,
    tint_r: f32,
    tint_g: f32,
    tint_b: f32,
}

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var input_sampler: sampler;
@group(0) @binding(2) var<uniform> u: Uniforms;

// Pseudo-random noise
fn rand(co: vec2<f32>) -> f32 {
    return fract(sin(dot(co, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

// Animated noise
fn noise(uv: vec2<f32>, time: f32) -> f32 {
    let i = floor(uv);
    let f = fract(uv);
    
    let a = rand(i + vec2<f32>(0.0, 0.0) + time);
    let b = rand(i + vec2<f32>(1.0, 0.0) + time);
    let c = rand(i + vec2<f32>(0.0, 1.0) + time);
    let d = rand(i + vec2<f32>(1.0, 1.0) + time);
    
    let u_interp = f * f * (3.0 - 2.0 * f);
    
    return mix(mix(a, b, u_interp.x), mix(c, d, u_interp.x), u_interp.y);
}

// CRT curvature distortion
fn crt_curve(uv: vec2<f32>, amount: f32) -> vec2<f32> {
    let centered = uv * 2.0 - 1.0;
    let offset = centered.yx * centered.yx * amount;
    let curved = centered + centered * offset;
    return curved * 0.5 + 0.5;
}

// Vignette effect
fn vignette(uv: vec2<f32>, amount: f32) -> f32 {
    let center = uv * 2.0 - 1.0;
    let dist = length(center);
    return 1.0 - smoothstep(0.5, 1.5, dist * amount);
}

// Scanlines
fn scanlines(uv: vec2<f32>, time: f32) -> f32 {
    let y = uv.y * u.resolution.y + time * u.scanline_speed;
    let line = sin(y * 3.14159 / u.scanline_thickness);
    return 1.0 - (1.0 - line) * u.scanline_opacity;
}

// Static snow noise
fn static_snow(uv: vec2<f32>, time: f32) -> vec3<f32> {
    let n = rand(uv * u.resolution + vec2<f32>(time * 100.0, time * 57.0));
    
    if u.noise_colored > 0.5 {
        // Colored noise
        return vec3<f32>(
            rand(uv * u.resolution + vec2<f32>(time * 100.0, 0.0)),
            rand(uv * u.resolution + vec2<f32>(0.0, time * 100.0)),
            rand(uv * u.resolution + vec2<f32>(time * 50.0, time * 50.0))
        ) * u.noise_brightness;
    } else {
        // B&W noise
        return vec3<f32>(n * u.noise_brightness);
    }
}

// VHS tracking distortion
fn vhs_distort(uv: vec2<f32>, time: f32) -> vec2<f32> {
    var result = uv;
    
    // Horizontal jitter
    result.x += (rand(vec2<f32>(uv.y * 100.0, time)) - 0.5) * u.jitter;
    
    // Tracking error waves
    let wave = sin(uv.y * 20.0 + time * 5.0) * u.tracking_error;
    result.x += wave;
    
    // Head switching noise at bottom
    if uv.y > (1.0 - u.head_switching) {
        result.x += (rand(vec2<f32>(time, uv.y * 50.0)) - 0.5) * 0.1;
    }
    
    return result;
}

// Chromatic aberration
fn chromatic_aberration(uv: vec2<f32>) -> vec3<f32> {
    let center = uv - 0.5;
    let dist = length(center);
    let offset = center * dist * u.chromatic_aberration;
    
    let r = textureSample(input_texture, input_sampler, uv + offset).r;
    let g = textureSample(input_texture, input_sampler, uv).g;
    let b = textureSample(input_texture, input_sampler, uv - offset).b;
    
    return vec3<f32>(r, g, b);
}

// Color bleeding (VHS style)
fn color_bleed(uv: vec2<f32>) -> vec3<f32> {
    var color = vec3<f32>(0.0);
    let samples = 8;
    let blur_amount = u.color_bleeding * 0.01;
    
    for (var i = 0; i < samples; i++) {
        let offset = f32(i) * blur_amount;
        color += textureSample(input_texture, input_sampler, uv + vec2<f32>(offset, 0.0)).rgb;
    }
    
    return color / f32(samples);
}

// Adjust saturation
fn adjust_saturation(color: vec3<f32>, sat: f32) -> vec3<f32> {
    let luminance = dot(color, vec3<f32>(0.299, 0.587, 0.114));
    return mix(vec3<f32>(luminance), color, sat);
}

@fragment
fn main(@location(0) uv: vec2<f32>) -> @location(0) vec4<f32> {
    var coord = uv;
    
    // Apply CRT curvature
    if u.curvature > 0.0 {
        coord = crt_curve(coord, u.curvature);
        
        // Check if outside screen (black border)
        if coord.x < 0.0 || coord.x > 1.0 || coord.y < 0.0 || coord.y > 1.0 {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
    }
    
    // Apply VHS distortion
    if u.tracking_error > 0.0 || u.jitter > 0.0 {
        coord = vhs_distort(coord, u.time);
    }
    
    // Sample color with optional chromatic aberration
    var color: vec3<f32>;
    if u.chromatic_aberration > 0.0 {
        color = chromatic_aberration(coord);
    } else {
        color = textureSample(input_texture, input_sampler, coord).rgb;
    }
    
    // Apply color bleeding
    if u.color_bleeding > 0.0 {
        color = mix(color, color_bleed(coord), u.color_bleeding);
    }
    
    // Add static noise
    if u.noise_density > 0.0 {
        let snow = static_snow(coord, u.time);
        color = mix(color, snow, u.noise_density * u.intensity);
    }
    
    // Apply scanlines
    if u.scanline_opacity > 0.0 {
        color *= scanlines(coord, u.time);
    }
    
    // Apply vignette
    if u.vignette > 0.0 {
        color *= vignette(coord, u.vignette);
    }
    
    // Apply flicker
    if u.flicker > 0.0 {
        let flick = 1.0 + (rand(vec2<f32>(u.time, 0.0)) - 0.5) * u.flicker;
        color *= flick;
    }
    
    // Color adjustments
    color = adjust_saturation(color, u.saturation);
    color = (color - 0.5) * u.contrast + 0.5;
    color *= u.brightness;
    
    // Apply tint
    if u.tint_r > 0.0 || u.tint_g > 0.0 || u.tint_b > 0.0 {
        color *= vec3<f32>(u.tint_r, u.tint_g, u.tint_b);
    }
    
    return vec4<f32>(color, 1.0);
}
"#;

// ============================================================================
// Public Rust API
// ============================================================================


pub fn get_retro_presets() -> Vec<(String, RetroTvSettings)> {
    vec![
        ("No Signal".to_string(), RetroTvSettings::no_signal()),
        ("Channel Change".to_string(), RetroTvSettings::channel_change()),
        ("VHS Tape".to_string(), RetroTvSettings::vhs_tape()),
        ("VHS Pause".to_string(), RetroTvSettings::vhs_pause()),
        ("1960s Broadcast".to_string(), RetroTvSettings::retro_1960s()),
        ("CRT Monitor".to_string(), RetroTvSettings::crt_monitor()),
        ("Vaporwave".to_string(), RetroTvSettings::vaporwave()),
        ("Horror Glitch".to_string(), RetroTvSettings::horror_glitch()),
        ("Green Phosphor".to_string(), RetroTvSettings::green_phosphor()),
        ("Amber Phosphor".to_string(), RetroTvSettings::amber_phosphor()),
    ]
}


pub fn get_retro_shader() -> String {
    RETRO_TV_SHADER.to_string()
}
