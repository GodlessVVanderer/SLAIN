//! NES PPU (Picture Processing Unit) - 2C02
//!
//! The PPU generates the video output for the NES.
//! - 256x240 pixel output
//! - 64 sprites (8 per scanline)
//! - 2KB VRAM + cartridge CHR
//! - 256 bytes OAM (Object Attribute Memory)

use crate::emulation::cartridge::{NesCartridge, Mirroring};

/// PPU register addresses (relative to $2000)
pub const PPUCTRL: u16 = 0;
pub const PPUMASK: u16 = 1;
pub const PPUSTATUS: u16 = 2;
pub const OAMADDR: u16 = 3;
pub const OAMDATA: u16 = 4;
pub const PPUSCROLL: u16 = 5;
pub const PPUADDR: u16 = 6;
pub const PPUDATA: u16 = 7;

/// Screen dimensions
pub const WIDTH: usize = 256;
pub const HEIGHT: usize = 240;

/// NES color palette (64 colors, RGBA format)
const PALETTE: [(u8, u8, u8); 64] = [
    (84, 84, 84), (0, 30, 116), (8, 16, 144), (48, 0, 136),
    (68, 0, 100), (92, 0, 48), (84, 4, 0), (60, 24, 0),
    (32, 42, 0), (8, 58, 0), (0, 64, 0), (0, 60, 0),
    (0, 50, 60), (0, 0, 0), (0, 0, 0), (0, 0, 0),
    (152, 150, 152), (8, 76, 196), (48, 50, 236), (92, 30, 228),
    (136, 20, 176), (160, 20, 100), (152, 34, 32), (120, 60, 0),
    (84, 90, 0), (40, 114, 0), (8, 124, 0), (0, 118, 40),
    (0, 102, 120), (0, 0, 0), (0, 0, 0), (0, 0, 0),
    (236, 238, 236), (76, 154, 236), (120, 124, 236), (176, 98, 236),
    (228, 84, 236), (236, 88, 180), (236, 106, 100), (212, 136, 32),
    (160, 170, 0), (116, 196, 0), (76, 208, 32), (56, 204, 108),
    (56, 180, 204), (60, 60, 60), (0, 0, 0), (0, 0, 0),
    (236, 238, 236), (168, 204, 236), (188, 188, 236), (212, 178, 236),
    (236, 174, 236), (236, 174, 212), (236, 180, 176), (228, 196, 144),
    (204, 210, 120), (180, 222, 120), (168, 226, 144), (152, 226, 180),
    (160, 214, 228), (160, 162, 160), (0, 0, 0), (0, 0, 0),
];

/// PPU state
pub struct Ppu {
    /// VRAM (2KB nametable memory)
    vram: [u8; 2048],
    /// Palette RAM (32 bytes)
    palette: [u8; 32],
    /// OAM (256 bytes)
    oam: [u8; 256],
    /// Secondary OAM for sprite evaluation
    secondary_oam: [u8; 32],

    /// Current scanline (0-261)
    scanline: u16,
    /// Current cycle within scanline (0-340)
    cycle: u16,
    /// Frame count
    frame: u64,
    /// Odd frame flag
    odd_frame: bool,

    /// PPUCTRL ($2000)
    ctrl: u8,
    /// PPUMASK ($2001)
    mask: u8,
    /// PPUSTATUS ($2002)
    status: u8,
    /// OAM address
    oam_addr: u8,

    /// VRAM address (15 bits)
    v: u16,
    /// Temporary VRAM address
    t: u16,
    /// Fine X scroll (3 bits)
    fine_x: u8,
    /// Write toggle (for $2005/$2006)
    w: bool,

    /// Data buffer for PPUDATA reads
    data_buffer: u8,

    /// Background shift registers
    bg_shifter_pattern_lo: u16,
    bg_shifter_pattern_hi: u16,
    bg_shifter_attrib_lo: u16,
    bg_shifter_attrib_hi: u16,

    /// Next tile data
    bg_next_tile_id: u8,
    bg_next_tile_attrib: u8,
    bg_next_tile_lo: u8,
    bg_next_tile_hi: u8,

    /// Sprite data for current scanline
    sprite_count: u8,
    sprite_patterns_lo: [u8; 8],
    sprite_patterns_hi: [u8; 8],
    sprite_positions: [u8; 8],
    sprite_priorities: [u8; 8],
    sprite_indexes: [u8; 8],
    sprite_zero_on_line: bool,
    sprite_zero_rendered: bool,

    /// NMI output
    nmi_output: bool,
    nmi_occurred: bool,
    nmi_delay: u8,

    /// Framebuffer (256x240 RGBA)
    framebuffer: Vec<u8>,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            vram: [0; 2048],
            palette: [0; 32],
            oam: [0; 256],
            secondary_oam: [0xFF; 32],
            scanline: 0,
            cycle: 0,
            frame: 0,
            odd_frame: false,
            ctrl: 0,
            mask: 0,
            status: 0,
            oam_addr: 0,
            v: 0,
            t: 0,
            fine_x: 0,
            w: false,
            data_buffer: 0,
            bg_shifter_pattern_lo: 0,
            bg_shifter_pattern_hi: 0,
            bg_shifter_attrib_lo: 0,
            bg_shifter_attrib_hi: 0,
            bg_next_tile_id: 0,
            bg_next_tile_attrib: 0,
            bg_next_tile_lo: 0,
            bg_next_tile_hi: 0,
            sprite_count: 0,
            sprite_patterns_lo: [0; 8],
            sprite_patterns_hi: [0; 8],
            sprite_positions: [0; 8],
            sprite_priorities: [0; 8],
            sprite_indexes: [0; 8],
            sprite_zero_on_line: false,
            sprite_zero_rendered: false,
            nmi_output: false,
            nmi_occurred: false,
            nmi_delay: 0,
            framebuffer: vec![0; WIDTH * HEIGHT * 4],
        }
    }

    pub fn reset(&mut self) {
        self.ctrl = 0;
        self.mask = 0;
        self.status = 0;
        self.oam_addr = 0;
        self.v = 0;
        self.t = 0;
        self.fine_x = 0;
        self.w = false;
        self.data_buffer = 0;
        self.scanline = 0;
        self.cycle = 0;
        self.odd_frame = false;
        self.nmi_output = false;
        self.nmi_occurred = false;
    }

    /// Execute one PPU cycle
    pub fn step(&mut self, cart: &mut Option<NesCartridge>) -> (bool, bool) {
        let mut nmi_triggered = false;

        // Handle NMI delay
        if self.nmi_delay > 0 {
            self.nmi_delay -= 1;
            if self.nmi_delay == 0 && self.nmi_output && self.nmi_occurred {
                nmi_triggered = true;
            }
        }

        // Visible scanlines (0-239)
        if self.scanline < 240 {
            self.render_pixel(cart);
            self.update_shifters();
            self.fetch_tile_data(cart);
        }

        // Post-render scanline (240) - idle

        // Vertical blank (241-260)
        if self.scanline == 241 && self.cycle == 1 {
            self.status |= 0x80; // Set VBlank flag
            self.nmi_occurred = true;
            if self.ctrl & 0x80 != 0 {
                self.nmi_output = true;
                self.nmi_delay = 2;
            }
        }

        // Pre-render scanline (261)
        if self.scanline == 261 {
            if self.cycle == 1 {
                self.status &= !0xE0; // Clear VBlank, Sprite 0 hit, Overflow
                self.nmi_occurred = false;
                self.nmi_output = false;
                self.sprite_zero_rendered = false;
            }

            // Copy vertical bits from t to v
            if self.cycle >= 280 && self.cycle <= 304 && self.rendering_enabled() {
                self.v = (self.v & 0x041F) | (self.t & 0x7BE0);
            }

            self.fetch_tile_data(cart);
        }

        // Advance cycle/scanline
        self.cycle += 1;
        if self.cycle > 340 {
            self.cycle = 0;
            self.scanline += 1;

            if self.scanline > 261 {
                self.scanline = 0;
                self.frame += 1;
                self.odd_frame = !self.odd_frame;

                // Skip cycle on odd frame
                if self.odd_frame && self.rendering_enabled() {
                    self.cycle = 1;
                }
            }
        }

        (nmi_triggered, self.scanline == 241)
    }

    fn rendering_enabled(&self) -> bool {
        self.mask & 0x18 != 0
    }

    fn render_pixel(&mut self, cart: &Option<NesCartridge>) {
        if self.cycle == 0 || self.cycle > 256 || self.scanline >= 240 {
            return;
        }

        let x = self.cycle - 1;
        let y = self.scanline;

        // Background pixel
        let mut bg_pixel = 0u8;
        let mut bg_palette = 0u8;

        if self.mask & 0x08 != 0 && (self.mask & 0x02 != 0 || x >= 8) {
            let bit_mux = 0x8000 >> self.fine_x;
            let p0 = if self.bg_shifter_pattern_lo & bit_mux != 0 { 1 } else { 0 };
            let p1 = if self.bg_shifter_pattern_hi & bit_mux != 0 { 2 } else { 0 };
            bg_pixel = p0 | p1;

            let a0 = if self.bg_shifter_attrib_lo & bit_mux != 0 { 1 } else { 0 };
            let a1 = if self.bg_shifter_attrib_hi & bit_mux != 0 { 2 } else { 0 };
            bg_palette = a0 | a1;
        }

        // Sprite pixel
        let mut fg_pixel = 0u8;
        let mut fg_palette = 0u8;
        let mut fg_priority = false;
        let mut sprite_zero = false;

        if self.mask & 0x10 != 0 && (self.mask & 0x04 != 0 || x >= 8) {
            for i in 0..self.sprite_count as usize {
                if self.sprite_positions[i] == 0 {
                    let p0 = if self.sprite_patterns_lo[i] & 0x80 != 0 { 1 } else { 0 };
                    let p1 = if self.sprite_patterns_hi[i] & 0x80 != 0 { 2 } else { 0 };
                    fg_pixel = p0 | p1;

                    if fg_pixel != 0 {
                        fg_palette = (self.secondary_oam[i * 4 + 2] & 0x03) + 4;
                        fg_priority = self.sprite_priorities[i] == 0;
                        if self.sprite_indexes[i] == 0 {
                            sprite_zero = true;
                        }
                        break;
                    }
                }
            }

            // Shift sprite patterns
            for i in 0..self.sprite_count as usize {
                if self.sprite_positions[i] > 0 {
                    self.sprite_positions[i] -= 1;
                } else {
                    self.sprite_patterns_lo[i] <<= 1;
                    self.sprite_patterns_hi[i] <<= 1;
                }
            }
        }

        // Priority decision
        let (pixel, palette) = if bg_pixel == 0 && fg_pixel == 0 {
            (0, 0)
        } else if bg_pixel == 0 && fg_pixel != 0 {
            (fg_pixel, fg_palette)
        } else if bg_pixel != 0 && fg_pixel == 0 {
            (bg_pixel, bg_palette)
        } else {
            // Sprite 0 hit detection
            if sprite_zero && self.sprite_zero_on_line && !self.sprite_zero_rendered {
                if self.mask & 0x18 == 0x18 && x < 255 {
                    self.status |= 0x40;
                    self.sprite_zero_rendered = true;
                }
            }

            if fg_priority {
                (fg_pixel, fg_palette)
            } else {
                (bg_pixel, bg_palette)
            }
        };

        // Get color from palette
        let palette_addr = if pixel == 0 { 0 } else { (palette << 2) | pixel };
        let color_idx = self.palette[palette_addr as usize & 0x1F] & 0x3F;
        let (r, g, b) = PALETTE[color_idx as usize];

        // Write to framebuffer
        let idx = (y as usize * WIDTH + x as usize) * 4;
        if idx + 3 < self.framebuffer.len() {
            self.framebuffer[idx] = r;
            self.framebuffer[idx + 1] = g;
            self.framebuffer[idx + 2] = b;
            self.framebuffer[idx + 3] = 255;
        }
    }

    fn update_shifters(&mut self) {
        if self.mask & 0x08 != 0 {
            self.bg_shifter_pattern_lo <<= 1;
            self.bg_shifter_pattern_hi <<= 1;
            self.bg_shifter_attrib_lo <<= 1;
            self.bg_shifter_attrib_hi <<= 1;
        }
    }

    fn fetch_tile_data(&mut self, cart: &mut Option<NesCartridge>) {
        if !self.rendering_enabled() {
            return;
        }

        if self.cycle >= 1 && self.cycle <= 256 || self.cycle >= 321 && self.cycle <= 336 {
            match self.cycle % 8 {
                1 => {
                    // Load shifters
                    self.load_shifters();
                    // Fetch nametable byte
                    let addr = 0x2000 | (self.v & 0x0FFF);
                    self.bg_next_tile_id = self.ppu_read(addr, cart);
                }
                3 => {
                    // Fetch attribute byte
                    let addr = 0x23C0
                        | (self.v & 0x0C00)
                        | ((self.v >> 4) & 0x38)
                        | ((self.v >> 2) & 0x07);
                    let shift = ((self.v >> 4) & 4) | (self.v & 2);
                    self.bg_next_tile_attrib = (self.ppu_read(addr, cart) >> shift) & 0x03;
                }
                5 => {
                    // Fetch pattern low byte
                    let table = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0 };
                    let addr = table
                        + (self.bg_next_tile_id as u16 * 16)
                        + ((self.v >> 12) & 7);
                    self.bg_next_tile_lo = self.ppu_read(addr, cart);
                }
                7 => {
                    // Fetch pattern high byte
                    let table = if self.ctrl & 0x10 != 0 { 0x1000 } else { 0 };
                    let addr = table
                        + (self.bg_next_tile_id as u16 * 16)
                        + ((self.v >> 12) & 7)
                        + 8;
                    self.bg_next_tile_hi = self.ppu_read(addr, cart);
                }
                0 => {
                    // Increment horizontal position
                    self.increment_x();
                }
                _ => {}
            }
        }

        if self.cycle == 256 {
            self.increment_y();
        }

        if self.cycle == 257 {
            self.load_shifters();
            // Copy horizontal bits from t to v
            self.v = (self.v & 0x7BE0) | (self.t & 0x041F);
            // Sprite evaluation
            self.evaluate_sprites(cart);
        }

        if self.cycle == 337 || self.cycle == 339 {
            // Dummy nametable fetches
            let addr = 0x2000 | (self.v & 0x0FFF);
            self.bg_next_tile_id = self.ppu_read(addr, cart);
        }
    }

    fn load_shifters(&mut self) {
        self.bg_shifter_pattern_lo = (self.bg_shifter_pattern_lo & 0xFF00) | self.bg_next_tile_lo as u16;
        self.bg_shifter_pattern_hi = (self.bg_shifter_pattern_hi & 0xFF00) | self.bg_next_tile_hi as u16;

        let attrib_lo = if self.bg_next_tile_attrib & 1 != 0 { 0xFF } else { 0 };
        let attrib_hi = if self.bg_next_tile_attrib & 2 != 0 { 0xFF } else { 0 };
        self.bg_shifter_attrib_lo = (self.bg_shifter_attrib_lo & 0xFF00) | attrib_lo;
        self.bg_shifter_attrib_hi = (self.bg_shifter_attrib_hi & 0xFF00) | attrib_hi;
    }

    fn increment_x(&mut self) {
        if (self.v & 0x001F) == 31 {
            self.v &= !0x001F;
            self.v ^= 0x0400; // Switch horizontal nametable
        } else {
            self.v += 1;
        }
    }

    fn increment_y(&mut self) {
        if (self.v & 0x7000) != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= !0x7000;
            let mut y = (self.v & 0x03E0) >> 5;
            if y == 29 {
                y = 0;
                self.v ^= 0x0800; // Switch vertical nametable
            } else if y == 31 {
                y = 0;
            } else {
                y += 1;
            }
            self.v = (self.v & !0x03E0) | (y << 5);
        }
    }

    fn evaluate_sprites(&mut self, cart: &mut Option<NesCartridge>) {
        self.sprite_count = 0;
        self.sprite_zero_on_line = false;
        self.secondary_oam.fill(0xFF);

        let sprite_height = if self.ctrl & 0x20 != 0 { 16 } else { 8 };

        for i in 0..64 {
            let y = self.oam[i * 4] as i16;
            let diff = self.scanline as i16 - y;

            if diff >= 0 && diff < sprite_height {
                if self.sprite_count < 8 {
                    let idx = self.sprite_count as usize;
                    self.secondary_oam[idx * 4] = self.oam[i * 4];
                    self.secondary_oam[idx * 4 + 1] = self.oam[i * 4 + 1];
                    self.secondary_oam[idx * 4 + 2] = self.oam[i * 4 + 2];
                    self.secondary_oam[idx * 4 + 3] = self.oam[i * 4 + 3];
                    self.sprite_indexes[idx] = i as u8;

                    if i == 0 {
                        self.sprite_zero_on_line = true;
                    }

                    self.sprite_count += 1;
                } else {
                    self.status |= 0x20; // Sprite overflow
                    break;
                }
            }
        }

        // Fetch sprite patterns
        for i in 0..self.sprite_count as usize {
            let tile_idx = self.secondary_oam[i * 4 + 1];
            let attr = self.secondary_oam[i * 4 + 2];
            let x = self.secondary_oam[i * 4 + 3];
            let y = self.secondary_oam[i * 4] as u16;

            let flip_v = attr & 0x80 != 0;
            let flip_h = attr & 0x40 != 0;

            let mut row = self.scanline.wrapping_sub(y) as u8;

            let (pattern_addr, pattern_row) = if self.ctrl & 0x20 != 0 {
                // 8x16 sprites
                let table = (tile_idx & 1) as u16 * 0x1000;
                let tile = tile_idx & 0xFE;

                if row >= 8 {
                    row -= 8;
                    if flip_v { row = 7 - row; }
                    (table + (tile as u16 + 1) * 16, row)
                } else {
                    if flip_v { row = 7 - row; }
                    (table + tile as u16 * 16, row)
                }
            } else {
                // 8x8 sprites
                if flip_v { row = 7 - row; }
                let table = if self.ctrl & 0x08 != 0 { 0x1000 } else { 0 };
                (table + tile_idx as u16 * 16, row)
            };

            let lo = self.ppu_read(pattern_addr + pattern_row as u16, cart);
            let hi = self.ppu_read(pattern_addr + pattern_row as u16 + 8, cart);

            self.sprite_patterns_lo[i] = if flip_h { Self::reverse_bits(lo) } else { lo };
            self.sprite_patterns_hi[i] = if flip_h { Self::reverse_bits(hi) } else { hi };
            self.sprite_positions[i] = x;
            self.sprite_priorities[i] = (attr >> 5) & 1;
        }
    }

    fn reverse_bits(mut b: u8) -> u8 {
        b = (b & 0xF0) >> 4 | (b & 0x0F) << 4;
        b = (b & 0xCC) >> 2 | (b & 0x33) << 2;
        b = (b & 0xAA) >> 1 | (b & 0x55) << 1;
        b
    }

    fn mirror_nametable_addr(&self, addr: u16, cart: &Option<NesCartridge>) -> usize {
        let addr = addr & 0x0FFF;
        let mirroring = cart.as_ref()
            .map(|c| c.mirroring())
            .unwrap_or(Mirroring::Vertical);

        match mirroring {
            Mirroring::Vertical => (addr & 0x07FF) as usize,
            Mirroring::Horizontal => {
                let a = addr & 0x03FF;
                let b = (addr & 0x0800) >> 1;
                (a | b) as usize
            }
            Mirroring::SingleScreenLow => (addr & 0x03FF) as usize,
            Mirroring::SingleScreenHigh => (0x400 + (addr & 0x03FF)) as usize,
            Mirroring::FourScreen => addr as usize,
        }
    }

    fn ppu_read(&self, addr: u16, cart: &Option<NesCartridge>) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                cart.as_ref().map(|c| c.ppu_read(addr)).unwrap_or(0)
            }
            0x2000..=0x3EFF => {
                let idx = self.mirror_nametable_addr(addr, cart);
                self.vram[idx]
            }
            0x3F00..=0x3FFF => {
                let addr = (addr & 0x1F) as usize;
                let addr = if addr == 0x10 || addr == 0x14 || addr == 0x18 || addr == 0x1C {
                    addr - 0x10
                } else {
                    addr
                };
                self.palette[addr]
            }
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, val: u8, cart: &mut Option<NesCartridge>) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if let Some(c) = cart {
                    c.ppu_write(addr, val);
                }
            }
            0x2000..=0x3EFF => {
                let idx = self.mirror_nametable_addr(addr, cart);
                self.vram[idx] = val;
            }
            0x3F00..=0x3FFF => {
                let addr = (addr & 0x1F) as usize;
                let addr = if addr == 0x10 || addr == 0x14 || addr == 0x18 || addr == 0x1C {
                    addr - 0x10
                } else {
                    addr
                };
                self.palette[addr] = val;
            }
            _ => {}
        }
    }

    /// Read from PPU register
    pub fn read_register(&mut self, addr: u16, cart: &Option<NesCartridge>) -> u8 {
        match addr & 7 {
            PPUSTATUS => {
                let val = (self.status & 0xE0) | (self.data_buffer & 0x1F);
                self.status &= !0x80; // Clear VBlank
                self.nmi_occurred = false;
                self.w = false;
                val
            }
            OAMDATA => {
                self.oam[self.oam_addr as usize]
            }
            PPUDATA => {
                let mut val = self.data_buffer;
                self.data_buffer = self.ppu_read(self.v, cart);

                if self.v >= 0x3F00 {
                    val = self.data_buffer;
                    self.data_buffer = self.ppu_read(self.v - 0x1000, cart);
                }

                self.v = self.v.wrapping_add(if self.ctrl & 0x04 != 0 { 32 } else { 1 });
                val
            }
            _ => 0,
        }
    }

    /// Write to PPU register
    pub fn write_register(&mut self, addr: u16, val: u8, cart: &mut Option<NesCartridge>) {
        match addr & 7 {
            PPUCTRL => {
                let old_nmi = self.ctrl & 0x80 != 0;
                self.ctrl = val;
                self.t = (self.t & 0xF3FF) | ((val as u16 & 0x03) << 10);

                // Trigger NMI if enabled during vblank
                if !old_nmi && (val & 0x80 != 0) && self.nmi_occurred {
                    self.nmi_output = true;
                    self.nmi_delay = 2;
                }
            }
            PPUMASK => {
                self.mask = val;
            }
            OAMADDR => {
                self.oam_addr = val;
            }
            OAMDATA => {
                self.oam[self.oam_addr as usize] = val;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            PPUSCROLL => {
                if !self.w {
                    self.t = (self.t & 0xFFE0) | ((val as u16) >> 3);
                    self.fine_x = val & 0x07;
                } else {
                    self.t = (self.t & 0x8C1F)
                        | (((val as u16) & 0x07) << 12)
                        | (((val as u16) & 0xF8) << 2);
                }
                self.w = !self.w;
            }
            PPUADDR => {
                if !self.w {
                    self.t = (self.t & 0x00FF) | (((val as u16) & 0x3F) << 8);
                } else {
                    self.t = (self.t & 0xFF00) | (val as u16);
                    self.v = self.t;
                }
                self.w = !self.w;
            }
            PPUDATA => {
                self.ppu_write(self.v, val, cart);
                self.v = self.v.wrapping_add(if self.ctrl & 0x04 != 0 { 32 } else { 1 });
            }
            _ => {}
        }
    }

    /// DMA write to OAM
    pub fn write_oam_data(&mut self, val: u8) {
        self.oam[self.oam_addr as usize] = val;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    /// Get framebuffer
    pub fn get_framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// Save PPU state
    pub fn save_state(&self) -> Vec<u8> {
        let mut state = Vec::new();
        state.extend_from_slice(&self.vram);
        state.extend_from_slice(&self.palette);
        state.extend_from_slice(&self.oam);
        state.extend_from_slice(&self.scanline.to_le_bytes());
        state.extend_from_slice(&self.cycle.to_le_bytes());
        state.push(self.ctrl);
        state.push(self.mask);
        state.push(self.status);
        state
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Self::new()
    }
}
