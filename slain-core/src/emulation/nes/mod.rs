//! NES/Famicom emulation
//!
//! Complete NES emulation including:
//! - 2A03 CPU (6502 variant)
//! - 2C02 PPU (Picture Processing Unit)
//! - 2A03 APU (Audio Processing Unit)
//! - Memory mapping and cartridge interface

mod ppu;
mod apu;
mod bus;

pub use ppu::Ppu;
pub use apu::Apu;
pub use bus::NesBus;

use crate::emulation::cpu::Cpu;
use crate::emulation::cpu::mos6502::Bus6502;

use std::path::Path;
use crate::emulation::{EmulationResult, EmulationError};
use crate::emulation::cpu::mos6502::Mos6502;
use crate::emulation::cartridge::NesCartridge;
use crate::emulation::input::ButtonState;

/// NES system constants
pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 240;
pub const CPU_FREQ: u32 = 1789773; // NTSC
pub const CYCLES_PER_FRAME: u32 = 29780;

/// Complete NES system
pub struct Nes {
    /// CPU
    cpu: Mos6502<NesBus>,
    /// Frame count
    frame: u64,
    /// Controller state
    controller1: u8,
    controller2: u8,
    /// Controller shift registers
    controller1_shift: u8,
    controller2_shift: u8,
    /// Strobe state
    strobe: bool,
}

impl Nes {
    pub fn new() -> Self {
        let bus = NesBus::new();
        let cpu = Mos6502::new(bus);

        Self {
            cpu,
            frame: 0,
            controller1: 0,
            controller2: 0,
            controller1_shift: 0,
            controller2_shift: 0,
            strobe: false,
        }
    }

    /// Load a ROM file
    pub fn load_rom(&mut self, path: &Path) -> EmulationResult<()> {
        let cart = NesCartridge::load(path)?;
        self.cpu.bus.load_cartridge(cart);
        self.reset();
        Ok(())
    }

    /// Load ROM from bytes
    pub fn load_rom_bytes(&mut self, data: &[u8]) -> EmulationResult<()> {
        let cart = NesCartridge::from_bytes(data)?;
        self.cpu.bus.load_cartridge(cart);
        self.reset();
        Ok(())
    }

    /// Reset the system
    pub fn reset(&mut self) {
        self.cpu.reset();
        self.cpu.bus.ppu.reset();
        self.cpu.bus.apu.reset();
        self.frame = 0;
    }

    /// Run one frame of emulation
    pub fn run_frame(&mut self) -> EmulationResult<()> {
        let target_cycles = CYCLES_PER_FRAME as u64;
        let start_cycles = self.cpu.cycles;

        while self.cpu.cycles - start_cycles < target_cycles {
            self.step();
        }

        self.frame += 1;
        Ok(())
    }

    /// Execute one CPU step
    fn step(&mut self) {
        let cpu_cycles = self.cpu.step();

        // PPU runs at 3x CPU speed
        for _ in 0..(cpu_cycles * 3) {
            let (nmi, _) = self.cpu.bus.ppu.step(&mut self.cpu.bus.cartridge);
            if nmi {
                self.cpu.nmi();
            }
        }

        // APU runs at CPU speed
        for _ in 0..cpu_cycles {
            self.cpu.bus.apu.step();
        }

        // Handle DMA
        if self.cpu.bus.dma_pending {
            self.cpu.bus.dma_pending = false;
            let page = self.cpu.bus.dma_page as u16;
            for i in 0..256u16 {
                let addr = (page << 8) | i;
                let data = self.cpu.bus.read(addr);
                self.cpu.bus.ppu.write_oam_data(data);
            }
            self.cpu.add_stall(513 + if self.cpu.cycles % 2 == 1 { 1 } else { 0 });
        }
    }

    /// Get the framebuffer (256x240 RGBA)
    pub fn get_framebuffer(&self) -> &[u8] {
        self.cpu.bus.ppu.get_framebuffer()
    }

    /// Get audio samples
    pub fn get_audio_samples(&mut self) -> Vec<f32> {
        self.cpu.bus.apu.get_samples()
    }

    /// Set controller input
    pub fn set_input(&mut self, player: u8, buttons: ButtonState) {
        let byte = buttons.to_nes_byte();
        if player == 0 {
            self.controller1 = byte;
            self.cpu.bus.controller1 = byte;
        } else {
            self.controller2 = byte;
            self.cpu.bus.controller2 = byte;
        }
    }

    /// Save state to bytes
    pub fn save_state(&self) -> EmulationResult<Vec<u8>> {
        // Simple state format: CPU regs + RAM + PPU state
        let mut state = Vec::new();

        // CPU registers
        state.extend_from_slice(&self.cpu.a.to_le_bytes());
        state.extend_from_slice(&self.cpu.x.to_le_bytes());
        state.extend_from_slice(&self.cpu.y.to_le_bytes());
        state.extend_from_slice(&self.cpu.sp.to_le_bytes());
        state.extend_from_slice(&self.cpu.pc.to_le_bytes());
        state.push(self.cpu.get_status());

        // RAM
        state.extend_from_slice(self.cpu.bus.ram.as_slice());

        // PPU state
        state.extend_from_slice(&self.cpu.bus.ppu.save_state());

        Ok(state)
    }

    /// Load state from bytes
    pub fn load_state(&mut self, data: &[u8]) -> EmulationResult<()> {
        if data.len() < 8 {
            return Err(EmulationError::RomLoadError("Invalid state data".to_string()));
        }

        // CPU registers
        self.cpu.a = data[0];
        self.cpu.x = data[1];
        self.cpu.y = data[2];
        self.cpu.sp = data[3];
        self.cpu.pc = u16::from_le_bytes([data[4], data[5]]);
        self.cpu.set_status(data[6]);

        // RAM
        let ram_start = 7;
        let ram_end = ram_start + 0x800;
        if data.len() >= ram_end {
            self.cpu.bus.ram.as_mut_slice().copy_from_slice(&data[ram_start..ram_end]);
        }

        Ok(())
    }

    /// Get current frame count
    pub fn frame_count(&self) -> u64 {
        self.frame
    }
}

impl Default for Nes {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nes_creation() {
        let nes = Nes::new();
        assert_eq!(nes.frame_count(), 0);
    }
}
