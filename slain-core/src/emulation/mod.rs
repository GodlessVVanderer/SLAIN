//! # SLAIN Emulation Core
//!
//! Fist of the North Star (Hokuto no Ken) multi-platform emulator.
//!
//! Supports:
//! - NES/Famicom (6502 CPU, 2C02 PPU, 2A03 APU)
//! - Sega Master System (Z80 CPU, VDP, PSG)
//!
//! "You are already dead." - Kenshiro

pub mod cpu;
pub mod memory;
pub mod cartridge;
pub mod input;

// NES/Famicom emulation
pub mod nes;

// Sega Master System emulation
pub mod sms;

// Frontend integration
pub mod frontend;

use std::path::Path;
use thiserror::Error;

/// Emulation errors
#[derive(Debug, Error)]
pub enum EmulationError {
    #[error("Failed to load ROM: {0}")]
    RomLoadError(String),

    #[error("Unsupported mapper: {0}")]
    UnsupportedMapper(u8),

    #[error("Invalid ROM format")]
    InvalidRomFormat,

    #[error("CPU error: {0}")]
    CpuError(String),

    #[error("PPU error: {0}")]
    PpuError(String),

    #[error("APU error: {0}")]
    ApuError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type EmulationResult<T> = Result<T, EmulationError>;

/// Supported emulation platforms
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// Nintendo Entertainment System / Famicom
    Nes,
    /// Sega Master System
    Sms,
}

impl Platform {
    /// Detect platform from ROM file extension
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()?.to_lowercase().as_str() {
            "nes" => Some(Platform::Nes),
            "sms" | "sg" => Some(Platform::Sms),
            _ => None,
        }
    }
}

/// Emulator configuration
#[derive(Debug, Clone)]
pub struct EmulatorConfig {
    /// Target frames per second
    pub target_fps: f64,
    /// Enable audio
    pub audio_enabled: bool,
    /// Audio sample rate
    pub sample_rate: u32,
    /// Enable CRT filter effects
    pub crt_filter: bool,
    /// Scale factor for rendering
    pub scale: u32,
    /// Enable save states
    pub save_states_enabled: bool,
    /// Enable rewind feature
    pub rewind_enabled: bool,
    /// Rewind buffer size in seconds
    pub rewind_buffer_seconds: u32,
}

impl Default for EmulatorConfig {
    fn default() -> Self {
        Self {
            target_fps: 60.0,
            audio_enabled: true,
            sample_rate: 44100,
            crt_filter: true,
            scale: 3,
            save_states_enabled: true,
            rewind_enabled: true,
            rewind_buffer_seconds: 30,
        }
    }
}

/// Main emulator instance
pub struct Emulator {
    platform: Platform,
    /// Emulator configuration
    pub config: EmulatorConfig,
    nes: Option<nes::Nes>,
    sms: Option<sms::Sms>,
    running: bool,
    frame_count: u64,
}

impl Emulator {
    /// Create a new emulator instance
    pub fn new(config: EmulatorConfig) -> Self {
        Self {
            platform: Platform::Nes,
            config,
            nes: None,
            sms: None,
            running: false,
            frame_count: 0,
        }
    }

    /// Load a ROM file
    pub fn load_rom(&mut self, path: &Path) -> EmulationResult<()> {
        let platform = Platform::from_path(path)
            .ok_or_else(|| EmulationError::RomLoadError(
                "Unknown file extension".to_string()
            ))?;

        self.platform = platform;

        match platform {
            Platform::Nes => {
                let mut nes = nes::Nes::new();
                nes.load_rom(path)?;
                self.nes = Some(nes);
                tracing::info!("Loaded NES ROM: {:?}", path);
            }
            Platform::Sms => {
                let mut sms = sms::Sms::new();
                sms.load_rom(path)?;
                self.sms = Some(sms);
                tracing::info!("Loaded SMS ROM: {:?}", path);
            }
        }

        Ok(())
    }

    /// Reset the emulator
    pub fn reset(&mut self) {
        match self.platform {
            Platform::Nes => {
                if let Some(nes) = &mut self.nes {
                    nes.reset();
                }
            }
            Platform::Sms => {
                if let Some(sms) = &mut self.sms {
                    sms.reset();
                }
            }
        }
        self.frame_count = 0;
    }

    /// Run one frame of emulation
    pub fn run_frame(&mut self) -> EmulationResult<()> {
        match self.platform {
            Platform::Nes => {
                if let Some(nes) = &mut self.nes {
                    nes.run_frame()?;
                }
            }
            Platform::Sms => {
                if let Some(sms) = &mut self.sms {
                    sms.run_frame()?;
                }
            }
        }
        self.frame_count += 1;
        Ok(())
    }

    /// Get the current framebuffer (RGBA format)
    pub fn get_framebuffer(&self) -> Option<&[u8]> {
        match self.platform {
            Platform::Nes => self.nes.as_ref().map(|nes| nes.get_framebuffer()),
            Platform::Sms => self.sms.as_ref().map(|sms| sms.get_framebuffer()),
        }
    }

    /// Get screen dimensions
    pub fn get_dimensions(&self) -> (u32, u32) {
        match self.platform {
            Platform::Nes => (256, 240),
            Platform::Sms => (256, 192),
        }
    }

    /// Get audio samples
    pub fn get_audio_samples(&mut self) -> Vec<f32> {
        match self.platform {
            Platform::Nes => {
                self.nes.as_mut()
                    .map(|nes| nes.get_audio_samples())
                    .unwrap_or_default()
            }
            Platform::Sms => {
                self.sms.as_mut()
                    .map(|sms| sms.get_audio_samples())
                    .unwrap_or_default()
            }
        }
    }

    /// Set controller input
    pub fn set_input(&mut self, player: u8, buttons: input::ButtonState) {
        match self.platform {
            Platform::Nes => {
                if let Some(nes) = &mut self.nes {
                    nes.set_input(player, buttons);
                }
            }
            Platform::Sms => {
                if let Some(sms) = &mut self.sms {
                    sms.set_input(player, buttons);
                }
            }
        }
    }

    /// Get current frame count
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Get current platform
    pub fn platform(&self) -> Platform {
        self.platform
    }

    /// Save state to bytes
    pub fn save_state(&self) -> EmulationResult<Vec<u8>> {
        match self.platform {
            Platform::Nes => {
                self.nes.as_ref()
                    .map(|nes| nes.save_state())
                    .unwrap_or_else(|| Ok(Vec::new()))
            }
            Platform::Sms => {
                self.sms.as_ref()
                    .map(|sms| sms.save_state())
                    .unwrap_or_else(|| Ok(Vec::new()))
            }
        }
    }

    /// Load state from bytes
    pub fn load_state(&mut self, data: &[u8]) -> EmulationResult<()> {
        match self.platform {
            Platform::Nes => {
                if let Some(nes) = &mut self.nes {
                    nes.load_state(data)?;
                }
            }
            Platform::Sms => {
                if let Some(sms) = &mut self.sms {
                    sms.load_state(data)?;
                }
            }
        }
        Ok(())
    }
}

/// "Omae wa mou shindeiru" - Classic quote counter
pub struct HokutoQuotes {
    quotes: Vec<&'static str>,
    index: usize,
}

impl HokutoQuotes {
    pub fn new() -> Self {
        Self {
            quotes: vec![
                "Omae wa mou shindeiru.",  // You are already dead
                "Nani?!",                    // What?!
                "Hokuto Hyakuretsu Ken!",   // Hundred Crack Fist
                "Watawa!",                   // Screaming death
                "Hidebu!",                   // Death scream
                "Abeshi!",                   // Death scream
                "Tawaba!",                   // Death scream
                "Kenshiro... Ken...shiro!", // Dying words
                "Hokuto Shinken wa muteki da!", // Hokuto Shinken is invincible
                "Ai wo torimodose!",        // Take back the love
            ],
            index: 0,
        }
    }

    pub fn next(&mut self) -> &'static str {
        let quote = self.quotes[self.index];
        self.index = (self.index + 1) % self.quotes.len();
        quote
    }
}

impl Default for HokutoQuotes {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emulator_creation() {
        let config = EmulatorConfig::default();
        let emu = Emulator::new(config);
        assert_eq!(emu.frame_count(), 0);
    }

    #[test]
    fn test_quotes() {
        let mut quotes = HokutoQuotes::new();
        assert_eq!(quotes.next(), "Omae wa mou shindeiru.");
        assert_eq!(quotes.next(), "Nani?!");
    }
}
