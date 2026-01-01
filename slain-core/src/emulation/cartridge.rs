//! ROM cartridge and disc image loading

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use crate::emulation::{EmulationError, EmulationResult};

/// iNES ROM header (NES)
#[derive(Debug, Clone)]
pub struct InesHeader {
    /// PRG ROM size in 16KB units
    pub prg_rom_size: u8,
    /// CHR ROM size in 8KB units
    pub chr_rom_size: u8,
    /// Mapper number (lower 4 bits from flag 6, upper 4 from flag 7)
    pub mapper: u8,
    /// Mirroring type (0=horizontal, 1=vertical)
    pub mirroring: u8,
    /// Battery-backed RAM present
    pub battery: bool,
    /// Trainer present (512 bytes at $7000-$71FF)
    pub trainer: bool,
    /// Four-screen VRAM
    pub four_screen: bool,
    /// NES 2.0 format
    pub nes2: bool,
}

impl InesHeader {
    pub fn parse(data: &[u8]) -> EmulationResult<Self> {
        if data.len() < 16 {
            return Err(EmulationError::InvalidRomFormat);
        }

        // Check magic "NES\x1A"
        if &data[0..4] != b"NES\x1a" {
            return Err(EmulationError::InvalidRomFormat);
        }

        let flags6 = data[6];
        let flags7 = data[7];

        let nes2 = (flags7 & 0x0C) == 0x08;
        let mapper = (flags6 >> 4) | (flags7 & 0xF0);

        Ok(Self {
            prg_rom_size: data[4],
            chr_rom_size: data[5],
            mapper,
            mirroring: flags6 & 0x01,
            battery: flags6 & 0x02 != 0,
            trainer: flags6 & 0x04 != 0,
            four_screen: flags6 & 0x08 != 0,
            nes2,
        })
    }
}

/// NES cartridge
pub struct NesCartridge {
    pub header: InesHeader,
    pub prg_rom: Vec<u8>,
    pub chr_rom: Vec<u8>,
    pub prg_ram: Vec<u8>,
    pub chr_ram: Vec<u8>,
    /// Mapper state
    pub mapper_state: MapperState,
}

impl NesCartridge {
    pub fn load(path: &Path) -> EmulationResult<Self> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        Self::from_bytes(&data)
    }

    pub fn from_bytes(data: &[u8]) -> EmulationResult<Self> {
        let header = InesHeader::parse(data)?;

        let prg_start = 16 + if header.trainer { 512 } else { 0 };
        let prg_size = (header.prg_rom_size as usize) * 16384;
        let chr_start = prg_start + prg_size;
        let chr_size = (header.chr_rom_size as usize) * 8192;

        if data.len() < chr_start + chr_size {
            return Err(EmulationError::InvalidRomFormat);
        }

        let prg_rom = data[prg_start..prg_start + prg_size].to_vec();
        let chr_rom = if chr_size > 0 {
            data[chr_start..chr_start + chr_size].to_vec()
        } else {
            Vec::new()
        };

        // CHR RAM if no CHR ROM
        let chr_ram = if chr_size == 0 {
            vec![0; 8192]
        } else {
            Vec::new()
        };

        let prg_ram = vec![0; 8192]; // Standard 8KB PRG RAM

        let mapper_state = MapperState::new(header.mapper, prg_size, chr_size);

        Ok(Self {
            header,
            prg_rom,
            chr_rom,
            prg_ram,
            chr_ram,
            mapper_state,
        })
    }

    /// Read from CPU address space
    pub fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // PRG RAM
                self.prg_ram[(addr - 0x6000) as usize & 0x1FFF]
            }
            0x8000..=0xFFFF => {
                // PRG ROM
                let mapped = self.mapper_state.map_prg(addr);
                self.prg_rom.get(mapped).copied().unwrap_or(0)
            }
            _ => 0,
        }
    }

    /// Write to CPU address space
    pub fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                // PRG RAM
                self.prg_ram[(addr - 0x6000) as usize & 0x1FFF] = val;
            }
            0x8000..=0xFFFF => {
                // Mapper register write
                self.mapper_state.write(addr, val);
            }
            _ => {}
        }
    }

    /// Read from PPU address space (CHR)
    pub fn ppu_read(&self, addr: u16) -> u8 {
        let mapped = self.mapper_state.map_chr(addr);
        if !self.chr_rom.is_empty() {
            self.chr_rom.get(mapped).copied().unwrap_or(0)
        } else {
            self.chr_ram.get(mapped).copied().unwrap_or(0)
        }
    }

    /// Write to PPU address space (CHR RAM only)
    pub fn ppu_write(&mut self, addr: u16, val: u8) {
        if self.chr_rom.is_empty() {
            let mapped = self.mapper_state.map_chr(addr);
            if let Some(byte) = self.chr_ram.get_mut(mapped) {
                *byte = val;
            }
        }
    }

    /// Get nametable mirroring mode
    pub fn mirroring(&self) -> Mirroring {
        if self.header.four_screen {
            Mirroring::FourScreen
        } else if self.mapper_state.mirroring_override.is_some() {
            self.mapper_state.mirroring_override.unwrap()
        } else if self.header.mirroring == 1 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }
}

/// Nametable mirroring modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    SingleScreenLow,
    SingleScreenHigh,
    FourScreen,
}

/// Mapper state machine
pub struct MapperState {
    mapper: u8,
    prg_size: usize,
    chr_size: usize,
    /// PRG bank registers
    prg_bank: [usize; 4],
    /// CHR bank registers
    chr_bank: [usize; 8],
    /// Shift register (for mapper 1)
    shift_reg: u8,
    shift_count: u8,
    /// Control register
    control: u8,
    /// Mirroring override
    pub mirroring_override: Option<Mirroring>,
}

impl MapperState {
    pub fn new(mapper: u8, prg_size: usize, chr_size: usize) -> Self {
        let prg_banks = prg_size / 16384;

        Self {
            mapper,
            prg_size,
            chr_size,
            prg_bank: [0, 1.min(prg_banks.saturating_sub(1)),
                       prg_banks.saturating_sub(2), prg_banks.saturating_sub(1)],
            chr_bank: [0, 1, 2, 3, 4, 5, 6, 7],
            shift_reg: 0,
            shift_count: 0,
            control: 0x0C,
            mirroring_override: None,
        }
    }

    pub fn map_prg(&self, addr: u16) -> usize {
        match self.mapper {
            0 => {
                // NROM - simple mirroring
                if self.prg_size <= 16384 {
                    (addr as usize - 0x8000) & 0x3FFF
                } else {
                    (addr as usize - 0x8000) & 0x7FFF
                }
            }
            1 => {
                // MMC1
                let mode = (self.control >> 2) & 3;
                match mode {
                    0 | 1 => {
                        // 32KB mode
                        let bank = (self.prg_bank[0] & 0xFE) * 16384;
                        bank + ((addr as usize - 0x8000) & 0x7FFF)
                    }
                    2 => {
                        // Fix first bank
                        if addr < 0xC000 {
                            (addr as usize - 0x8000) & 0x3FFF
                        } else {
                            self.prg_bank[0] * 16384 + ((addr as usize - 0xC000) & 0x3FFF)
                        }
                    }
                    3 => {
                        // Fix last bank
                        if addr < 0xC000 {
                            self.prg_bank[0] * 16384 + ((addr as usize - 0x8000) & 0x3FFF)
                        } else {
                            (self.prg_size - 16384) + ((addr as usize - 0xC000) & 0x3FFF)
                        }
                    }
                    _ => (addr as usize - 0x8000) & 0x7FFF,
                }
            }
            2 => {
                // UxROM
                if addr < 0xC000 {
                    self.prg_bank[0] * 16384 + ((addr as usize - 0x8000) & 0x3FFF)
                } else {
                    (self.prg_size - 16384) + ((addr as usize - 0xC000) & 0x3FFF)
                }
            }
            3 => {
                // CNROM - PRG same as NROM
                if self.prg_size <= 16384 {
                    (addr as usize - 0x8000) & 0x3FFF
                } else {
                    (addr as usize - 0x8000) & 0x7FFF
                }
            }
            4 => {
                // MMC3
                let bank = if addr < 0xA000 {
                    if self.control & 0x40 != 0 { self.prg_bank[2] } else { self.prg_bank[0] }
                } else if addr < 0xC000 {
                    self.prg_bank[1]
                } else if addr < 0xE000 {
                    if self.control & 0x40 != 0 { self.prg_bank[0] } else { self.prg_bank[2] }
                } else {
                    self.prg_bank[3]
                };
                bank * 8192 + ((addr as usize) & 0x1FFF)
            }
            _ => {
                // Default mapping
                (addr as usize - 0x8000) % self.prg_size
            }
        }
    }

    pub fn map_chr(&self, addr: u16) -> usize {
        let addr = addr as usize & 0x1FFF;

        match self.mapper {
            0 | 2 => addr,
            1 => {
                // MMC1
                let mode = self.control & 0x10;
                if mode == 0 {
                    // 8KB mode
                    (self.chr_bank[0] & 0xFE) * 4096 + (addr & 0x1FFF)
                } else {
                    // 4KB mode
                    if addr < 0x1000 {
                        self.chr_bank[0] * 4096 + (addr & 0x0FFF)
                    } else {
                        self.chr_bank[4] * 4096 + (addr & 0x0FFF)
                    }
                }
            }
            3 => {
                // CNROM
                self.chr_bank[0] * 8192 + addr
            }
            4 => {
                // MMC3
                let bank = if self.control & 0x80 != 0 {
                    match addr >> 10 {
                        0 => self.chr_bank[2],
                        1 => self.chr_bank[3],
                        2 => self.chr_bank[4],
                        3 => self.chr_bank[5],
                        4 => self.chr_bank[0],
                        5 => self.chr_bank[0] + 1,
                        6 => self.chr_bank[1],
                        7 => self.chr_bank[1] + 1,
                        _ => 0,
                    }
                } else {
                    match addr >> 10 {
                        0 => self.chr_bank[0],
                        1 => self.chr_bank[0] + 1,
                        2 => self.chr_bank[1],
                        3 => self.chr_bank[1] + 1,
                        4 => self.chr_bank[2],
                        5 => self.chr_bank[3],
                        6 => self.chr_bank[4],
                        7 => self.chr_bank[5],
                        _ => 0,
                    }
                };
                bank * 1024 + (addr & 0x3FF)
            }
            _ => addr,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match self.mapper {
            1 => {
                // MMC1
                if val & 0x80 != 0 {
                    self.shift_reg = 0;
                    self.shift_count = 0;
                    self.control |= 0x0C;
                } else {
                    self.shift_reg |= (val & 1) << self.shift_count;
                    self.shift_count += 1;

                    if self.shift_count == 5 {
                        let reg = (addr >> 13) & 3;
                        match reg {
                            0 => {
                                self.control = self.shift_reg;
                                self.mirroring_override = Some(match self.shift_reg & 3 {
                                    0 => Mirroring::SingleScreenLow,
                                    1 => Mirroring::SingleScreenHigh,
                                    2 => Mirroring::Vertical,
                                    _ => Mirroring::Horizontal,
                                });
                            }
                            1 => self.chr_bank[0] = self.shift_reg as usize,
                            2 => self.chr_bank[4] = self.shift_reg as usize,
                            3 => self.prg_bank[0] = (self.shift_reg & 0x0F) as usize,
                            _ => {}
                        }
                        self.shift_reg = 0;
                        self.shift_count = 0;
                    }
                }
            }
            2 => {
                // UxROM
                self.prg_bank[0] = (val & 0x0F) as usize;
            }
            3 => {
                // CNROM
                self.chr_bank[0] = (val & 0x03) as usize;
            }
            4 => {
                // MMC3
                match addr & 0xE001 {
                    0x8000 => self.control = val,
                    0x8001 => {
                        let reg = self.control & 0x07;
                        match reg {
                            0 => { self.chr_bank[0] = (val & 0xFE) as usize; }
                            1 => { self.chr_bank[1] = (val & 0xFE) as usize; }
                            2 => { self.chr_bank[2] = val as usize; }
                            3 => { self.chr_bank[3] = val as usize; }
                            4 => { self.chr_bank[4] = val as usize; }
                            5 => { self.chr_bank[5] = val as usize; }
                            6 => { self.prg_bank[0] = (val & 0x3F) as usize; }
                            7 => { self.prg_bank[1] = (val & 0x3F) as usize; }
                            _ => {}
                        }
                    }
                    0xA000 => {
                        self.mirroring_override = Some(if val & 1 != 0 {
                            Mirroring::Horizontal
                        } else {
                            Mirroring::Vertical
                        });
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// SMS ROM cartridge
pub struct SmsCartridge {
    pub rom: Vec<u8>,
    pub ram: Vec<u8>,
    /// ROM bank registers
    pub banks: [u8; 4],
    /// RAM enable
    pub ram_enabled: bool,
}

impl SmsCartridge {
    pub fn load(path: &Path) -> EmulationResult<Self> {
        let mut file = File::open(path)?;
        let mut rom = Vec::new();
        file.read_to_end(&mut rom)?;

        // Check for 512-byte header
        if rom.len() % 16384 == 512 {
            rom = rom[512..].to_vec();
        }

        Ok(Self {
            rom,
            ram: vec![0; 32768],
            banks: [0, 1, 2, 0],
            ram_enabled: false,
        })
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x03FF => {
                // First 1KB is always from bank 0
                self.rom.get(addr as usize).copied().unwrap_or(0xFF)
            }
            0x0400..=0x3FFF => {
                // Slot 0: Bank 0 (fixed) or bank from $FFFD
                let bank = self.banks[0] as usize;
                let offset = bank * 16384 + (addr as usize);
                self.rom.get(offset).copied().unwrap_or(0xFF)
            }
            0x4000..=0x7FFF => {
                // Slot 1: Bank from $FFFE
                let bank = self.banks[1] as usize;
                let offset = bank * 16384 + ((addr - 0x4000) as usize);
                self.rom.get(offset).copied().unwrap_or(0xFF)
            }
            0x8000..=0xBFFF => {
                if self.ram_enabled && self.banks[3] & 0x08 != 0 {
                    // Cartridge RAM
                    let bank = ((self.banks[3] >> 2) & 1) as usize;
                    self.ram[(bank * 16384 + (addr - 0x8000) as usize) & 0x7FFF]
                } else {
                    // Slot 2: Bank from $FFFF
                    let bank = self.banks[2] as usize;
                    let offset = bank * 16384 + ((addr - 0x8000) as usize);
                    self.rom.get(offset).copied().unwrap_or(0xFF)
                }
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x8000..=0xBFFF => {
                if self.ram_enabled && self.banks[3] & 0x08 != 0 {
                    let bank = ((self.banks[3] >> 2) & 1) as usize;
                    self.ram[(bank * 16384 + (addr - 0x8000) as usize) & 0x7FFF] = val;
                }
            }
            0xFFFC => {
                self.banks[3] = val;
                self.ram_enabled = val & 0x08 != 0;
            }
            0xFFFD => self.banks[0] = val,
            0xFFFE => self.banks[1] = val,
            0xFFFF => self.banks[2] = val,
            _ => {}
        }
    }
}

/// Atomiswave cartridge (NAOMI/Dreamcast based arcade)
pub struct AtomiswaveCartridge {
    /// Main ROM data
    pub rom: Vec<u8>,
    /// Game ID
    pub game_id: String,
    /// Decryption key (if encrypted)
    pub key: Option<[u8; 16]>,
}

impl AtomiswaveCartridge {
    /// Load from ZIP file (MAME format)
    pub fn load_zip(path: &Path) -> EmulationResult<Self> {
        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)
            .map_err(|e| EmulationError::RomLoadError(e.to_string()))?;

        let mut rom_data = Vec::new();
        let mut game_id = String::new();

        // Look for ROM files in the ZIP
        for i in 0..archive.len() {
            let mut file = archive.by_index(i)
                .map_err(|e| EmulationError::RomLoadError(e.to_string()))?;
            let name = file.name().to_lowercase();

            if name.ends_with(".ic") || name.contains("epr-") || name.contains("mpr-") {
                let mut data = Vec::new();
                file.read_to_end(&mut data)?;
                rom_data.extend(data);
            }

            if game_id.is_empty() {
                game_id = Path::new(file.name())
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("unknown")
                    .to_string();
            }
        }

        if rom_data.is_empty() {
            // Try reading as raw ROM
            let mut file = File::open(path)?;
            file.read_to_end(&mut rom_data)?;
        }

        Ok(Self {
            rom: rom_data,
            game_id,
            key: None,
        })
    }

    /// Load from directory (CHD or raw files)
    pub fn load_dir(path: &Path) -> EmulationResult<Self> {
        let mut rom_data = Vec::new();
        let game_id = path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();

        // Look for ROM files
        if let Ok(entries) = std::fs::read_dir(path) {
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let name = e.file_name().to_string_lossy().to_lowercase();
                    name.ends_with(".bin") || name.ends_with(".rom") || name.ends_with(".ic")
                })
                .collect();

            files.sort_by_key(|e| e.file_name());

            for entry in files {
                let mut file = File::open(entry.path())?;
                file.read_to_end(&mut rom_data)?;
            }
        }

        Ok(Self {
            rom: rom_data,
            game_id,
            key: None,
        })
    }

    pub fn read32(&self, addr: u32) -> u32 {
        let addr = addr as usize;
        if addr + 3 < self.rom.len() {
            u32::from_le_bytes([
                self.rom[addr],
                self.rom[addr + 1],
                self.rom[addr + 2],
                self.rom[addr + 3],
            ])
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ines_header() {
        let data = [
            0x4E, 0x45, 0x53, 0x1A, // NES\x1A
            0x02, // 2 PRG banks (32KB)
            0x01, // 1 CHR bank (8KB)
            0x01, // Vertical mirroring
            0x00, // No special flags
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        ];

        let header = InesHeader::parse(&data).unwrap();
        assert_eq!(header.prg_rom_size, 2);
        assert_eq!(header.chr_rom_size, 1);
        assert_eq!(header.mirroring, 1);
        assert_eq!(header.mapper, 0);
    }
}
