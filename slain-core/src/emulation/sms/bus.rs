//! SMS memory bus

use crate::emulation::cpu::z80::BusZ80;
use crate::emulation::cartridge::SmsCartridge;
use crate::emulation::memory::Ram;
use crate::emulation::input::ButtonState;
use super::vdp::Vdp;
use super::psg::Psg;

pub struct SmsBus {
    /// 8KB RAM
    pub ram: Ram,
    /// VDP
    pub vdp: Vdp,
    /// PSG
    pub psg: Psg,
    /// Cartridge
    cartridge: Option<SmsCartridge>,
    /// Controllers
    pub controller1: ButtonState,
    pub controller2: ButtonState,
    /// Region/IO control
    io_control: u8,
}

impl SmsBus {
    pub fn new() -> Self {
        Self {
            ram: Ram::new(0x2000),
            vdp: Vdp::new(),
            psg: Psg::new(),
            cartridge: None,
            controller1: ButtonState::default(),
            controller2: ButtonState::default(),
            io_control: 0xFF,
        }
    }

    pub fn load_cartridge(&mut self, cart: SmsCartridge) {
        self.cartridge = Some(cart);
    }
}

impl BusZ80 for SmsBus {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            // Cartridge ROM
            0x0000..=0xBFFF => {
                self.cartridge.as_ref().map(|c| c.read(addr)).unwrap_or(0xFF)
            }
            // RAM (with mirroring)
            0xC000..=0xFFFF => {
                // Handle mapper registers at $FFFC-$FFFF
                if addr >= 0xFFFC {
                    if let Some(c) = &self.cartridge {
                        return c.read(addr);
                    }
                }
                self.ram.read(addr & 0x1FFF)
            }
        }
    }

    fn write(&mut self, addr: u16, data: u8) {
        match addr {
            // Cartridge ROM (mapper registers)
            0x0000..=0xBFFF => {
                if let Some(c) = &mut self.cartridge {
                    c.write(addr, data);
                }
            }
            // RAM (with mirroring)
            0xC000..=0xFFFF => {
                // Handle mapper registers
                if addr >= 0xFFFC {
                    if let Some(c) = &mut self.cartridge {
                        c.write(addr, data);
                    }
                }
                self.ram.write(addr & 0x1FFF, data);
            }
        }
    }

    fn io_read(&mut self, port: u16) -> u8 {
        let port = port & 0xFF;

        match port {
            // I/O port control
            0x00 => 0xFF,

            // VDP V counter
            0x7E => self.vdp.read_vcounter(),

            // VDP H counter
            0x7F => self.vdp.read_hcounter(),

            // VDP data port
            0xBE => self.vdp.read_data(),

            // VDP control port
            0xBF => self.vdp.read_control(),

            // I/O port A
            0xDC => {
                let mut val = 0xFF;
                if self.controller1.up { val &= !0x01; }
                if self.controller1.down { val &= !0x02; }
                if self.controller1.left { val &= !0x04; }
                if self.controller1.right { val &= !0x08; }
                if self.controller1.a { val &= !0x10; }
                if self.controller1.b { val &= !0x20; }
                if self.controller2.up { val &= !0x40; }
                if self.controller2.down { val &= !0x80; }
                val
            }

            // I/O port B
            0xDD => {
                let mut val = 0xFF;
                if self.controller2.left { val &= !0x01; }
                if self.controller2.right { val &= !0x02; }
                if self.controller2.a { val &= !0x04; }
                if self.controller2.b { val &= !0x08; }
                // Reset button on bit 4
                val
            }

            _ => 0xFF,
        }
    }

    fn io_write(&mut self, port: u16, data: u8) {
        let port = port & 0xFF;

        match port {
            // Memory control
            0x3E => {
                self.io_control = data;
            }

            // I/O control
            0x3F => {}

            // PSG
            0x7E | 0x7F => {
                self.psg.write(data);
            }

            // VDP data port
            0xBE => {
                self.vdp.write_data(data);
            }

            // VDP control port
            0xBF => {
                self.vdp.write_control(data);
            }

            _ => {}
        }
    }
}

impl Default for SmsBus {
    fn default() -> Self {
        Self::new()
    }
}
