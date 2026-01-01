//! Emulator frontend integration
//!
//! Provides unified interface for running emulators within SLAIN player.

use std::path::Path;
use crate::emulation::{Emulator, EmulatorConfig, EmulationResult, Platform};
use crate::emulation::input::{ButtonState, DreamcastController, ArcadeStick, KeyMapping};

/// Unified emulator frontend for SLAIN integration
pub struct EmulatorFrontend {
    /// The emulator instance
    emulator: Emulator,
    /// Key mapping
    key_mapping: KeyMapping,
    /// Whether emulation is paused
    paused: bool,
    /// Fast forward mode
    fast_forward: bool,
    /// Rewind buffer
    rewind_buffer: Vec<Vec<u8>>,
    /// Rewind position
    rewind_pos: usize,
    /// Save state slots
    save_slots: [Option<Vec<u8>>; 10],
    /// Current ROM path
    rom_path: Option<String>,
    /// Audio buffer
    audio_buffer: Vec<f32>,
    /// Target sample rate
    sample_rate: u32,
}

impl EmulatorFrontend {
    pub fn new() -> Self {
        Self {
            emulator: Emulator::new(EmulatorConfig::default()),
            key_mapping: KeyMapping::default(),
            paused: true,
            fast_forward: false,
            rewind_buffer: Vec::new(),
            rewind_pos: 0,
            save_slots: Default::default(),
            rom_path: None,
            audio_buffer: Vec::new(),
            sample_rate: 44100,
        }
    }

    /// Create with custom config
    pub fn with_config(config: EmulatorConfig) -> Self {
        Self {
            emulator: Emulator::new(config),
            ..Self::new()
        }
    }

    /// Load a ROM file
    pub fn load_rom(&mut self, path: &str) -> EmulationResult<()> {
        let path = Path::new(path);
        self.emulator.load_rom(path)?;
        self.rom_path = Some(path.to_string_lossy().to_string());
        self.paused = false;
        self.rewind_buffer.clear();
        tracing::info!("Loaded ROM: {:?}", path);
        Ok(())
    }

    /// Run one frame
    pub fn run_frame(&mut self) -> EmulationResult<()> {
        if self.paused {
            return Ok(());
        }

        // Save state for rewind
        if self.emulator.config.rewind_enabled && self.emulator.frame_count() % 2 == 0 {
            if let Ok(state) = self.emulator.save_state() {
                let max_frames = self.emulator.config.rewind_buffer_seconds as usize * 30;
                if self.rewind_buffer.len() >= max_frames {
                    self.rewind_buffer.remove(0);
                }
                self.rewind_buffer.push(state);
            }
        }

        self.emulator.run_frame()?;

        // Collect audio
        let samples = self.emulator.get_audio_samples();
        self.audio_buffer.extend(samples);

        // Run extra frames for fast forward
        if self.fast_forward {
            for _ in 0..3 {
                self.emulator.run_frame()?;
            }
        }

        Ok(())
    }

    /// Get the current framebuffer
    pub fn get_framebuffer(&self) -> Option<&[u8]> {
        self.emulator.get_framebuffer()
    }

    /// Get screen dimensions
    pub fn get_dimensions(&self) -> (u32, u32) {
        self.emulator.get_dimensions()
    }

    /// Get audio samples and clear buffer
    pub fn get_audio(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.audio_buffer)
    }

    /// Process keyboard input
    pub fn process_key(&mut self, keycode: u32, pressed: bool) {
        let mut buttons = ButtonState::default();

        // Map key to button
        if keycode == self.key_mapping.up { buttons.up = pressed; }
        if keycode == self.key_mapping.down { buttons.down = pressed; }
        if keycode == self.key_mapping.left { buttons.left = pressed; }
        if keycode == self.key_mapping.right { buttons.right = pressed; }
        if keycode == self.key_mapping.a { buttons.a = pressed; }
        if keycode == self.key_mapping.b { buttons.b = pressed; }
        if keycode == self.key_mapping.start { buttons.start = pressed; }
        if keycode == self.key_mapping.select { buttons.select = pressed; }

        self.emulator.set_input(0, buttons);
    }

    /// Set controller state directly
    pub fn set_controller(&mut self, player: u8, buttons: ButtonState) {
        self.emulator.set_input(player, buttons);
    }

    /// Pause/unpause
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    /// Fast forward
    pub fn set_fast_forward(&mut self, enabled: bool) {
        self.fast_forward = enabled;
    }

    /// Reset the emulator
    pub fn reset(&mut self) {
        self.emulator.reset();
        self.rewind_buffer.clear();
    }

    /// Rewind one frame
    pub fn rewind_frame(&mut self) -> bool {
        if let Some(state) = self.rewind_buffer.pop() {
            let _ = self.emulator.load_state(&state);
            true
        } else {
            false
        }
    }

    /// Save state to slot
    pub fn save_state_slot(&mut self, slot: usize) -> EmulationResult<()> {
        if slot < 10 {
            let state = self.emulator.save_state()?;
            self.save_slots[slot] = Some(state);
            tracing::info!("Saved state to slot {}", slot);
        }
        Ok(())
    }

    /// Load state from slot
    pub fn load_state_slot(&mut self, slot: usize) -> EmulationResult<()> {
        if slot < 10 {
            if let Some(state) = &self.save_slots[slot] {
                self.emulator.load_state(state)?;
                tracing::info!("Loaded state from slot {}", slot);
            }
        }
        Ok(())
    }

    /// Get current frame count
    pub fn frame_count(&self) -> u64 {
        self.emulator.frame_count()
    }

    /// Get current platform
    pub fn platform(&self) -> Platform {
        self.emulator.platform()
    }

    /// Check if a ROM is loaded
    pub fn is_loaded(&self) -> bool {
        self.rom_path.is_some()
    }

    /// Get ROM path
    pub fn rom_path(&self) -> Option<&str> {
        self.rom_path.as_deref()
    }

    /// Set key mapping
    pub fn set_key_mapping(&mut self, mapping: KeyMapping) {
        self.key_mapping = mapping;
    }

    /// Get supported file extensions
    pub fn supported_extensions() -> &'static [&'static str] {
        &["nes", "sms", "sg", "bin", "zip", "chd"]
    }
}

impl Default for EmulatorFrontend {
    fn default() -> Self {
        Self::new()
    }
}

/// Atomiswave-specific frontend
pub struct AtomisawaveFrontend {
    /// Base frontend
    frontend: EmulatorFrontend,
    /// Arcade stick states
    pub sticks: [ArcadeStick; 2],
    /// Coin counters
    coin_counter: [u32; 2],
    /// Service/test mode
    service_mode: bool,
    test_mode: bool,
}

impl AtomisaweFrontend {
    pub fn new() -> Self {
        let mut config = EmulatorConfig::default();
        config.target_fps = 60.0;
        config.crt_filter = true;

        Self {
            frontend: EmulatorFrontend::with_config(config),
            sticks: [ArcadeStick::default(); 2],
            coin_counter: [0; 2],
            service_mode: false,
            test_mode: false,
        }
    }

    /// Load Fist of the North Star ROM
    pub fn load_hokuto(&mut self, rom_path: &str) -> EmulationResult<()> {
        self.frontend.load_rom(rom_path)?;
        tracing::info!("北斗の拳 (Fist of the North Star) loaded!");
        tracing::info!("\"Omae wa mou shindeiru.\"");
        Ok(())
    }

    /// Run frame
    pub fn run_frame(&mut self) -> EmulationResult<()> {
        self.frontend.run_frame()
    }

    /// Insert coin
    pub fn insert_coin(&mut self, player: u8) {
        if player < 2 {
            self.coin_counter[player as usize] += 1;
            tracing::info!("Coin inserted for P{}", player + 1);
        }
    }

    /// Get framebuffer
    pub fn get_framebuffer(&self) -> Option<&[u8]> {
        self.frontend.get_framebuffer()
    }

    /// Get audio
    pub fn get_audio(&mut self) -> Vec<f32> {
        self.frontend.get_audio()
    }

    /// Set arcade stick state
    pub fn set_stick(&mut self, player: u8, stick: ArcadeStick) {
        if player < 2 {
            self.sticks[player as usize] = stick;
            // Convert to standard button state for emulator
            let buttons = ButtonState {
                up: stick.up,
                down: stick.down,
                left: stick.left,
                right: stick.right,
                a: stick.lp,
                b: stick.mp,
                select: stick.coin,
                start: stick.start,
            };
            self.frontend.set_controller(player, buttons);
        }
    }

    /// Toggle test mode
    pub fn toggle_test_mode(&mut self) {
        self.test_mode = !self.test_mode;
    }

    /// Toggle service mode
    pub fn toggle_service_mode(&mut self) {
        self.service_mode = !self.service_mode;
    }
}

impl Default for AtomisaweFrontend {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontend_creation() {
        let frontend = EmulatorFrontend::new();
        assert!(frontend.is_paused());
        assert!(!frontend.is_loaded());
    }

    #[test]
    fn test_atomiswave_frontend() {
        let aw = AtomisaweFrontend::new();
        assert_eq!(aw.coin_counter[0], 0);
    }
}
