//! Sega Master System emulation
//!
//! Complete SMS emulation including:
//! - Z80 CPU
//! - VDP (Video Display Processor)
//! - PSG (Programmable Sound Generator)

mod vdp;
mod psg;
mod bus;

pub use vdp::Vdp;
pub use psg::Psg;
pub use bus::SmsBus;

use crate::emulation::cpu::Cpu;

use std::path::Path;
use crate::emulation::{EmulationResult, EmulationError};
use crate::emulation::cpu::z80::Z80;
use crate::emulation::cartridge::SmsCartridge;
use crate::emulation::input::ButtonState;

/// SMS screen dimensions
pub const SCREEN_WIDTH: usize = 256;
pub const SCREEN_HEIGHT: usize = 192;
pub const CPU_FREQ: u32 = 3579545;
pub const CYCLES_PER_FRAME: u32 = 59736;

/// Complete SMS system
pub struct Sms {
    cpu: Z80<SmsBus>,
    frame: u64,
}

impl Sms {
    pub fn new() -> Self {
        let bus = SmsBus::new();
        let cpu = Z80::new(bus);

        Self {
            cpu,
            frame: 0,
        }
    }

    pub fn load_rom(&mut self, path: &Path) -> EmulationResult<()> {
        let cart = SmsCartridge::load(path)?;
        self.cpu.bus.load_cartridge(cart);
        self.reset();
        Ok(())
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.cpu.bus.vdp.reset();
        self.cpu.bus.psg.reset();
        self.frame = 0;
    }

    pub fn run_frame(&mut self) -> EmulationResult<()> {
        let target_cycles = CYCLES_PER_FRAME as u64;
        let start_cycles = self.cpu.cycles;

        while self.cpu.cycles - start_cycles < target_cycles {
            let cpu_cycles = self.cpu.step();

            // VDP runs at same clock as CPU
            for _ in 0..cpu_cycles {
                let (irq, _) = self.cpu.bus.vdp.step();
                if irq && self.cpu.iff1 {
                    self.cpu.irq();
                }
            }
        }

        self.frame += 1;
        Ok(())
    }

    pub fn get_framebuffer(&self) -> &[u8] {
        self.cpu.bus.vdp.get_framebuffer()
    }

    pub fn get_audio_samples(&mut self) -> Vec<f32> {
        self.cpu.bus.psg.get_samples()
    }

    pub fn set_input(&mut self, player: u8, buttons: ButtonState) {
        if player == 0 {
            self.cpu.bus.controller1 = buttons;
        } else {
            self.cpu.bus.controller2 = buttons;
        }
    }

    pub fn save_state(&self) -> EmulationResult<Vec<u8>> {
        let mut state = Vec::new();
        // CPU registers
        state.push(self.cpu.a);
        state.push(self.cpu.f.to_byte());
        state.push(self.cpu.b);
        state.push(self.cpu.c);
        state.push(self.cpu.d);
        state.push(self.cpu.e);
        state.push(self.cpu.h);
        state.push(self.cpu.l);
        state.extend_from_slice(&self.cpu.sp.to_le_bytes());
        state.extend_from_slice(&self.cpu.pc.to_le_bytes());
        // RAM
        state.extend_from_slice(self.cpu.bus.ram.as_slice());
        Ok(state)
    }

    pub fn load_state(&mut self, data: &[u8]) -> EmulationResult<()> {
        if data.len() < 12 {
            return Err(EmulationError::RomLoadError("Invalid state".to_string()));
        }
        self.cpu.a = data[0];
        self.cpu.f = crate::emulation::cpu::z80::Z80Flags::from_byte(data[1]);
        self.cpu.b = data[2];
        self.cpu.c = data[3];
        self.cpu.d = data[4];
        self.cpu.e = data[5];
        self.cpu.h = data[6];
        self.cpu.l = data[7];
        self.cpu.sp = u16::from_le_bytes([data[8], data[9]]);
        self.cpu.pc = u16::from_le_bytes([data[10], data[11]]);
        Ok(())
    }
}

impl Default for Sms {
    fn default() -> Self {
        Self::new()
    }
}
