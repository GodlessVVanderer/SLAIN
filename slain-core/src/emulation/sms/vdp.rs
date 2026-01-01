//! SMS VDP (Video Display Processor)
//!
//! TMS9918-derived chip with enhancements:
//! - 256x192 or 256x224 resolution
//! - 32 colors from 64-color palette
//! - 64 sprites

use super::{SCREEN_WIDTH, SCREEN_HEIGHT};

/// SMS color palette (64 colors, RGBA)
const PALETTE: [(u8, u8, u8); 64] = [
    (0, 0, 0), (0, 0, 85), (0, 0, 170), (0, 0, 255),
    (0, 85, 0), (0, 85, 85), (0, 85, 170), (0, 85, 255),
    (0, 170, 0), (0, 170, 85), (0, 170, 170), (0, 170, 255),
    (0, 255, 0), (0, 255, 85), (0, 255, 170), (0, 255, 255),
    (85, 0, 0), (85, 0, 85), (85, 0, 170), (85, 0, 255),
    (85, 85, 0), (85, 85, 85), (85, 85, 170), (85, 85, 255),
    (85, 170, 0), (85, 170, 85), (85, 170, 170), (85, 170, 255),
    (85, 255, 0), (85, 255, 85), (85, 255, 170), (85, 255, 255),
    (170, 0, 0), (170, 0, 85), (170, 0, 170), (170, 0, 255),
    (170, 85, 0), (170, 85, 85), (170, 85, 170), (170, 85, 255),
    (170, 170, 0), (170, 170, 85), (170, 170, 170), (170, 170, 255),
    (170, 255, 0), (170, 255, 85), (170, 255, 170), (170, 255, 255),
    (255, 0, 0), (255, 0, 85), (255, 0, 170), (255, 0, 255),
    (255, 85, 0), (255, 85, 85), (255, 85, 170), (255, 85, 255),
    (255, 170, 0), (255, 170, 85), (255, 170, 170), (255, 170, 255),
    (255, 255, 0), (255, 255, 85), (255, 255, 170), (255, 255, 255),
];

pub struct Vdp {
    /// VRAM (16KB)
    vram: [u8; 16384],
    /// CRAM (color RAM, 32 bytes)
    cram: [u8; 32],
    /// VDP registers
    regs: [u8; 16],
    /// Status register
    status: u8,
    /// Address register
    address: u16,
    /// Code register (0-3)
    code: u8,
    /// First/second write flag
    first_byte: bool,
    /// Read buffer
    read_buffer: u8,
    /// Line counter
    line_counter: u8,
    /// Current scanline
    scanline: u16,
    /// Horizontal counter
    hcounter: u16,
    /// Frame count
    frame: u64,
    /// Framebuffer
    framebuffer: Vec<u8>,
    /// IRQ pending
    irq_pending: bool,
}

impl Vdp {
    pub fn new() -> Self {
        Self {
            vram: [0; 16384],
            cram: [0; 32],
            regs: [0; 16],
            status: 0,
            address: 0,
            code: 0,
            first_byte: true,
            read_buffer: 0,
            line_counter: 0,
            scanline: 0,
            hcounter: 0,
            frame: 0,
            framebuffer: vec![0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
            irq_pending: false,
        }
    }

    pub fn reset(&mut self) {
        self.regs.fill(0);
        self.status = 0;
        self.address = 0;
        self.code = 0;
        self.first_byte = true;
        self.read_buffer = 0;
        self.line_counter = 0;
        self.scanline = 0;
        self.hcounter = 0;
        self.irq_pending = false;
    }

    pub fn step(&mut self) -> (bool, bool) {
        let mut irq = false;
        let mut frame_done = false;

        self.hcounter += 1;

        // End of scanline
        if self.hcounter >= 342 {
            self.hcounter = 0;
            self.render_line();
            self.scanline += 1;

            // Line interrupt
            if self.scanline <= 192 {
                if self.line_counter == 0 {
                    self.line_counter = self.regs[10];
                    if self.regs[0] & 0x10 != 0 {
                        self.irq_pending = true;
                        irq = true;
                    }
                } else {
                    self.line_counter -= 1;
                }
            }

            // VBlank
            if self.scanline == 192 {
                self.status |= 0x80; // VBlank flag
                if self.regs[1] & 0x20 != 0 {
                    self.irq_pending = true;
                    irq = true;
                }
            }

            // End of frame
            if self.scanline >= 262 {
                self.scanline = 0;
                self.frame += 1;
                self.line_counter = self.regs[10];
                frame_done = true;
            }
        }

        (irq, frame_done)
    }

    fn render_line(&mut self) {
        if self.scanline >= SCREEN_HEIGHT as u16 {
            return;
        }

        let line = self.scanline as usize;
        let bg_enabled = self.regs[1] & 0x40 != 0;
        let sprites_enabled = self.regs[1] & 0x40 != 0;

        // Background color
        let bg_color = self.get_color(16 + (self.regs[7] & 0x0F) as usize);

        for x in 0..SCREEN_WIDTH {
            let mut color = bg_color;

            if bg_enabled {
                color = self.render_bg_pixel(x, line);
            }

            if sprites_enabled {
                if let Some(sprite_color) = self.render_sprite_pixel(x, line) {
                    color = sprite_color;
                }
            }

            let idx = (line * SCREEN_WIDTH + x) * 4;
            self.framebuffer[idx] = color.0;
            self.framebuffer[idx + 1] = color.1;
            self.framebuffer[idx + 2] = color.2;
            self.framebuffer[idx + 3] = 255;
        }
    }

    fn render_bg_pixel(&self, x: usize, y: usize) -> (u8, u8, u8) {
        // Name table address
        let name_table_addr = ((self.regs[2] & 0x0E) as u16) << 10;

        // Scroll
        let scroll_x = if y < 16 && self.regs[0] & 0x40 != 0 { 0 } else { self.regs[8] as usize };
        let scroll_y = self.regs[9] as usize;

        let scrolled_x = (x + 256 - scroll_x) % 256;
        let scrolled_y = (y + scroll_y) % 224;

        let tile_x = scrolled_x / 8;
        let tile_y = scrolled_y / 8;
        let pixel_x = scrolled_x % 8;
        let pixel_y = scrolled_y % 8;

        // Get tile entry from name table
        let entry_addr = name_table_addr + ((tile_y * 32 + tile_x) * 2) as u16;
        let entry_lo = self.vram[entry_addr as usize];
        let entry_hi = self.vram[entry_addr as usize + 1];

        let tile_idx = ((entry_hi as u16 & 0x01) << 8) | entry_lo as u16;
        let palette = (entry_hi >> 3) & 1;
        let flip_h = entry_hi & 0x02 != 0;
        let flip_v = entry_hi & 0x04 != 0;
        let priority = entry_hi & 0x10 != 0;

        let px = if flip_h { 7 - pixel_x } else { pixel_x };
        let py = if flip_v { 7 - pixel_y } else { pixel_y };

        // Get pixel from tile
        let tile_addr = tile_idx * 32 + (py as u16) * 4;
        let row0 = self.vram[tile_addr as usize];
        let row1 = self.vram[tile_addr as usize + 1];
        let row2 = self.vram[tile_addr as usize + 2];
        let row3 = self.vram[tile_addr as usize + 3];

        let bit = 7 - px;
        let color_idx = ((row0 >> bit) & 1)
            | (((row1 >> bit) & 1) << 1)
            | (((row2 >> bit) & 1) << 2)
            | (((row3 >> bit) & 1) << 3);

        let cram_idx = (palette as usize * 16) + color_idx as usize;
        self.get_color(cram_idx)
    }

    fn render_sprite_pixel(&self, x: usize, y: usize) -> Option<(u8, u8, u8)> {
        let sprite_table_addr = ((self.regs[5] & 0x7E) as u16) << 7;
        let sprite_height = if self.regs[1] & 0x02 != 0 { 16 } else { 8 };
        let zoom = self.regs[1] & 0x01 != 0;
        let height = if zoom { sprite_height * 2 } else { sprite_height };

        for i in 0..64 {
            let sprite_y = self.vram[(sprite_table_addr + i) as usize];

            // End of sprite list
            if sprite_y == 0xD0 {
                break;
            }

            let sprite_y = sprite_y as usize + 1;

            if y >= sprite_y && y < sprite_y + height {
                let sprite_x = self.vram[(sprite_table_addr + 128 + i * 2) as usize] as usize;
                let tile_idx = self.vram[(sprite_table_addr + 128 + i * 2 + 1) as usize] as u16;

                // Horizontal shift
                let sprite_x = if self.regs[0] & 0x08 != 0 { sprite_x.saturating_sub(8) } else { sprite_x };

                let width = if zoom { 16 } else { 8 };

                if x >= sprite_x && x < sprite_x + width {
                    let px = if zoom { (x - sprite_x) / 2 } else { x - sprite_x };
                    let py = if zoom { (y - sprite_y) / 2 } else { y - sprite_y };

                    // 8x16 sprites use tile pairs
                    let tile = if sprite_height == 16 {
                        (tile_idx & 0xFE) + if py >= 8 { 1 } else { 0 }
                    } else {
                        tile_idx
                    };

                    let row = py % 8;
                    let tile_addr = tile * 32 + (row as u16) * 4;

                    let row0 = self.vram[tile_addr as usize];
                    let row1 = self.vram[tile_addr as usize + 1];
                    let row2 = self.vram[tile_addr as usize + 2];
                    let row3 = self.vram[tile_addr as usize + 3];

                    let bit = 7 - px;
                    let color_idx = ((row0 >> bit) & 1)
                        | (((row1 >> bit) & 1) << 1)
                        | (((row2 >> bit) & 1) << 2)
                        | (((row3 >> bit) & 1) << 3);

                    if color_idx != 0 {
                        return Some(self.get_color(16 + color_idx as usize));
                    }
                }
            }
        }

        None
    }

    fn get_color(&self, idx: usize) -> (u8, u8, u8) {
        let cram_val = self.cram[idx & 0x1F];
        let r = (cram_val & 0x03) * 85;
        let g = ((cram_val >> 2) & 0x03) * 85;
        let b = ((cram_val >> 4) & 0x03) * 85;
        (r, g, b)
    }

    /// Read VDP data port
    pub fn read_data(&mut self) -> u8 {
        self.first_byte = true;
        let val = self.read_buffer;
        self.read_buffer = self.vram[self.address as usize & 0x3FFF];
        self.address = self.address.wrapping_add(1);
        val
    }

    /// Write VDP data port
    pub fn write_data(&mut self, val: u8) {
        self.first_byte = true;

        match self.code {
            0 | 1 | 2 => {
                // VRAM write
                self.vram[self.address as usize & 0x3FFF] = val;
            }
            3 => {
                // CRAM write
                self.cram[self.address as usize & 0x1F] = val;
            }
            _ => {}
        }

        self.address = self.address.wrapping_add(1);
    }

    /// Read VDP control port
    pub fn read_control(&mut self) -> u8 {
        self.first_byte = true;
        let val = self.status;
        self.status &= !0xE0; // Clear flags
        self.irq_pending = false;
        val
    }

    /// Write VDP control port
    pub fn write_control(&mut self, val: u8) {
        if self.first_byte {
            self.address = (self.address & 0xFF00) | val as u16;
            self.first_byte = false;
        } else {
            self.address = (self.address & 0x00FF) | ((val as u16 & 0x3F) << 8);
            self.code = (val >> 6) & 3;

            match self.code {
                0 => {
                    // VRAM read
                    self.read_buffer = self.vram[self.address as usize & 0x3FFF];
                    self.address = self.address.wrapping_add(1);
                }
                2 => {
                    // Register write
                    let reg = (self.address >> 8) & 0x0F;
                    self.regs[reg as usize] = self.address as u8;
                }
                _ => {}
            }

            self.first_byte = true;
        }
    }

    /// Read H counter
    pub fn read_hcounter(&self) -> u8 {
        (self.hcounter >> 1) as u8
    }

    /// Read V counter
    pub fn read_vcounter(&self) -> u8 {
        if self.scanline > 0xDA {
            (self.scanline - 6) as u8
        } else {
            self.scanline as u8
        }
    }

    pub fn get_framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }
}

impl Default for Vdp {
    fn default() -> Self {
        Self::new()
    }
}
