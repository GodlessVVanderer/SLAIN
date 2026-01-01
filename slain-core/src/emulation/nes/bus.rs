//! NES memory bus
//!
//! Memory map:
//! $0000-$07FF: 2KB internal RAM
//! $0800-$1FFF: Mirrors of RAM
//! $2000-$2007: PPU registers
//! $2008-$3FFF: Mirrors of PPU registers
//! $4000-$4017: APU and I/O registers
//! $4018-$401F: APU and I/O (normally disabled)
//! $4020-$FFFF: Cartridge space

use crate::emulation::cpu::mos6502::Bus6502;
use crate::emulation::cartridge::NesCartridge;
use crate::emulation::memory::Ram;
use super::ppu::Ppu;
use super::apu::Apu;

/// NES memory bus
pub struct NesBus {
    /// 2KB internal RAM
    pub ram: Ram,
    /// PPU
    pub ppu: Ppu,
    /// APU
    pub apu: Apu,
    /// Cartridge
    pub cartridge: Option<NesCartridge>,
    /// Controller state
    pub controller1: u8,
    pub controller2: u8,
    /// Controller shift registers
    controller1_shift: u8,
    controller2_shift: u8,
    /// Strobe state
    strobe: bool,
    /// DMA pending
    pub dma_pending: bool,
    pub dma_page: u8,
}

impl NesBus {
    pub fn new() -> Self {
        Self {
            ram: Ram::new(0x800),
            ppu: Ppu::new(),
            apu: Apu::new(),
            cartridge: None,
            controller1: 0,
            controller2: 0,
            controller1_shift: 0,
            controller2_shift: 0,
            strobe: false,
            dma_pending: false,
            dma_page: 0,
        }
    }

    pub fn load_cartridge(&mut self, cart: NesCartridge) {
        self.cartridge = Some(cart);
    }
}

impl Bus6502 for NesBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // RAM (with mirroring)
            0x0000..=0x1FFF => {
                self.ram.read(addr & 0x07FF)
            }

            // PPU registers (with mirroring)
            0x2000..=0x3FFF => {
                self.ppu.read_register(addr, &self.cartridge)
            }

            // APU and I/O
            0x4000..=0x4013 => {
                self.apu.read_register(addr)
            }

            // OAM DMA (write-only, returns open bus)
            0x4014 => 0,

            // APU status
            0x4015 => {
                self.apu.read_register(addr)
            }

            // Controller 1
            0x4016 => {
                let val = (self.controller1_shift & 0x80) >> 7;
                if !self.strobe {
                    self.controller1_shift <<= 1;
                }
                val | 0x40 // Open bus bits
            }

            // Controller 2
            0x4017 => {
                let val = (self.controller2_shift & 0x80) >> 7;
                if !self.strobe {
                    self.controller2_shift <<= 1;
                }
                val | 0x40 // Open bus bits
            }

            // Cartridge space
            0x4020..=0xFFFF => {
                self.cartridge.as_ref()
                    .map(|c| c.cpu_read(addr))
                    .unwrap_or(0)
            }

            _ => 0,
        }
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            // RAM (with mirroring)
            0x0000..=0x1FFF => {
                self.ram.write(addr & 0x07FF, data);
            }

            // PPU registers (with mirroring)
            0x2000..=0x3FFF => {
                self.ppu.write_register(addr, data, &mut self.cartridge);
            }

            // APU registers
            0x4000..=0x4013 => {
                self.apu.write_register(addr, data);
            }

            // OAM DMA
            0x4014 => {
                self.dma_pending = true;
                self.dma_page = data;
            }

            // APU status
            0x4015 => {
                self.apu.write_register(addr, data);
            }

            // Controller strobe
            0x4016 => {
                self.strobe = data & 1 != 0;
                if self.strobe {
                    self.controller1_shift = self.controller1;
                    self.controller2_shift = self.controller2;
                }
            }

            // APU frame counter
            0x4017 => {
                self.apu.write_register(addr, data);
            }

            // Cartridge space
            0x4020..=0xFFFF => {
                if let Some(c) = &mut self.cartridge {
                    c.cpu_write(addr, data);
                }
            }

            _ => {}
        }
    }
}

impl Default for NesBus {
    fn default() -> Self {
        Self::new()
    }
}
