//! Dreamcast/Atomiswave System Emulation
//!
//! The Sega Dreamcast uses:
//! - Hitachi SH-4 CPU @ 200MHz
//! - NEC PowerVR2 GPU (CLX2)
//! - Yamaha AICA sound processor with ARM7TDMI
//! - 16MB main RAM, 8MB VRAM, 2MB ARAM
//!
//! Atomiswave is arcade hardware based on Dreamcast.

pub mod pvr2;
pub mod aica;
pub mod holly;
pub mod bus;

use crate::emulation::cpu::sh4::Sh4;
use crate::emulation::memory::Ram32;
use crate::emulation::input::ArcadeStick;
use crate::emulation::cartridge::AtomiswaveCartridge;

pub use pvr2::PowerVR2;
pub use aica::Aica;
pub use holly::Holly;
pub use bus::DreamcastBus;

/// Dreamcast system state
pub struct Dreamcast {
    pub cpu: Sh4,
    pub bus: DreamcastBus,
    pub running: bool,
    frame_count: u64,
    cycles_per_frame: u64,
}

/// Atomiswave arcade system (Dreamcast-based)
pub struct Atomiswave {
    pub dc: Dreamcast,
    pub sticks: [ArcadeStick; 2],
    pub cartridge: Option<AtomiswaveCartridge>,
    pub coin_counter: [u32; 2],
    pub service_mode: bool,
    pub test_mode: bool,
    dip_switches: u8,
}

impl Dreamcast {
    pub fn new() -> Self {
        let bus = DreamcastBus::new();
        let cpu = Sh4::new();

        Self {
            cpu,
            bus,
            running: false,
            frame_count: 0,
            // SH-4 @ 200MHz, 60fps = ~3.33M cycles per frame
            cycles_per_frame: 200_000_000 / 60,
        }
    }

    pub fn reset(&mut self) {
        self.cpu.reset();
        self.bus.reset();
        self.frame_count = 0;
    }

    pub fn load_bios(&mut self, bios: &[u8]) {
        self.bus.load_bios(bios);
    }

    pub fn load_flash(&mut self, flash: &[u8]) {
        self.bus.load_flash(flash);
    }

    /// Run one frame of emulation
    pub fn run_frame(&mut self) -> &[u32] {
        let target_cycles = self.cpu.cycles + self.cycles_per_frame;

        while self.cpu.cycles < target_cycles && self.running {
            // Execute CPU instruction
            let cycles = self.step_cpu();

            // Step GPU
            self.bus.pvr2.step(cycles);

            // Step sound processor
            self.bus.aica.step(cycles);

            // Check for interrupts
            self.handle_interrupts();
        }

        self.frame_count += 1;
        self.bus.pvr2.framebuffer()
    }

    fn step_cpu(&mut self) -> u32 {
        // Fetch instruction
        let pc = self.cpu.pc;
        let instr = self.bus.read16(pc);
        self.cpu.pc = pc.wrapping_add(2);

        // Execute
        self.cpu.execute(instr, &mut self.bus)
    }

    fn handle_interrupts(&mut self) {
        // Check Holly interrupt controller
        if let Some(irq) = self.bus.holly.pending_interrupt() {
            if self.cpu.interrupts_enabled() {
                self.cpu.handle_interrupt(irq);
                self.bus.holly.acknowledge_interrupt(irq);
            }
        }
    }

    pub fn framebuffer(&self) -> &[u32] {
        self.bus.pvr2.framebuffer()
    }

    pub fn audio_samples(&mut self) -> Vec<i16> {
        self.bus.aica.drain_samples()
    }
}

impl Default for Dreamcast {
    fn default() -> Self {
        Self::new()
    }
}

impl Atomiswave {
    pub fn new() -> Self {
        Self {
            dc: Dreamcast::new(),
            sticks: [ArcadeStick::new(), ArcadeStick::new()],
            cartridge: None,
            coin_counter: [0; 2],
            service_mode: false,
            test_mode: false,
            dip_switches: 0xFF,
        }
    }

    pub fn load_cartridge(&mut self, cart: AtomiswaveCartridge) {
        // Load cartridge ROM into memory
        if let Some(ref rom_data) = cart.rom_data {
            self.dc.bus.load_atomiswave_rom(rom_data);
        }
        self.cartridge = Some(cart);
    }

    pub fn insert_coin(&mut self, player: usize) {
        if player < 2 {
            self.coin_counter[player] += 1;
            // Trigger coin interrupt
            self.dc.bus.holly.trigger_external_interrupt(0x10 + player as u32);
        }
    }

    pub fn enter_service_mode(&mut self) {
        self.service_mode = true;
        self.dc.bus.holly.trigger_external_interrupt(0x20);
    }

    pub fn exit_service_mode(&mut self) {
        self.service_mode = false;
    }

    pub fn enter_test_mode(&mut self) {
        self.test_mode = true;
        self.dc.bus.holly.trigger_external_interrupt(0x21);
    }

    pub fn set_dip_switch(&mut self, switch: u8, on: bool) {
        if on {
            self.dip_switches |= 1 << switch;
        } else {
            self.dip_switches &= !(1 << switch);
        }
    }

    pub fn run_frame(&mut self) -> &[u32] {
        // Update input state in bus
        self.dc.bus.update_arcade_input(&self.sticks, self.coin_counter, self.dip_switches);

        self.dc.run_frame()
    }

    pub fn reset(&mut self) {
        self.dc.reset();
        self.coin_counter = [0; 2];
        self.service_mode = false;
        self.test_mode = false;
    }
}

impl Default for Atomiswave {
    fn default() -> Self {
        Self::new()
    }
}

/// Fist of the North Star specific configuration
pub struct HokutoConfig {
    pub difficulty: HokutoDifficulty,
    pub rounds_to_win: u8,
    pub time_limit: u8,
    pub blood_enabled: bool,
    pub fatal_ko_enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HokutoDifficulty {
    VeryEasy = 0,
    Easy = 1,
    Normal = 2,
    Hard = 3,
    VeryHard = 4,
    Shin = 5,      // Hardest - named after Shin
    Raoh = 6,      // Ultra hard - named after Raoh
    Kaioh = 7,     // Maximum - named after Kaioh
}

impl Default for HokutoConfig {
    fn default() -> Self {
        Self {
            difficulty: HokutoDifficulty::Normal,
            rounds_to_win: 2,
            time_limit: 99,
            blood_enabled: true,
            fatal_ko_enabled: true,
        }
    }
}

impl HokutoConfig {
    /// Convert config to DIP switch settings
    pub fn to_dip_switches(&self) -> u8 {
        let mut dips = 0u8;

        // Bits 0-2: Difficulty
        dips |= (self.difficulty as u8) & 0x07;

        // Bit 3: Blood
        if self.blood_enabled {
            dips |= 0x08;
        }

        // Bit 4: Fatal KO
        if self.fatal_ko_enabled {
            dips |= 0x10;
        }

        // Bits 5-6: Rounds (1-4 encoded as 0-3)
        dips |= ((self.rounds_to_win.saturating_sub(1).min(3)) << 5) & 0x60;

        // Bit 7: Time limit (0 = 60s, 1 = 99s)
        if self.time_limit >= 99 {
            dips |= 0x80;
        }

        dips
    }
}

/// Character roster for Fist of the North Star
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HokutoCharacter {
    // Main protagonists
    Kenshiro,
    Rei,
    Toki,
    Mamiya,

    // Villains
    Shin,
    Jagi,
    Raoh,
    Souther,

    // Hidden/unlockable
    Thouzer,
    Ryuga,
    Juda,

    // Bosses
    Kaioh,
    HyohHokuto,
}

impl HokutoCharacter {
    pub fn all() -> &'static [HokutoCharacter] {
        &[
            HokutoCharacter::Kenshiro,
            HokutoCharacter::Rei,
            HokutoCharacter::Toki,
            HokutoCharacter::Mamiya,
            HokutoCharacter::Shin,
            HokutoCharacter::Jagi,
            HokutoCharacter::Raoh,
            HokutoCharacter::Souther,
            HokutoCharacter::Thouzer,
            HokutoCharacter::Ryuga,
            HokutoCharacter::Juda,
            HokutoCharacter::Kaioh,
            HokutoCharacter::HyohHokuto,
        ]
    }

    pub fn name(&self) -> &'static str {
        match self {
            HokutoCharacter::Kenshiro => "Kenshiro",
            HokutoCharacter::Rei => "Rei",
            HokutoCharacter::Toki => "Toki",
            HokutoCharacter::Mamiya => "Mamiya",
            HokutoCharacter::Shin => "Shin",
            HokutoCharacter::Jagi => "Jagi",
            HokutoCharacter::Raoh => "Raoh",
            HokutoCharacter::Souther => "Souther",
            HokutoCharacter::Thouzer => "Thouzer",
            HokutoCharacter::Ryuga => "Ryuga",
            HokutoCharacter::Juda => "Juda",
            HokutoCharacter::Kaioh => "Kaioh",
            HokutoCharacter::HyohHokuto => "Hyoh",
        }
    }

    pub fn fighting_style(&self) -> &'static str {
        match self {
            HokutoCharacter::Kenshiro => "Hokuto Shinken",
            HokutoCharacter::Rei => "Nanto Suicho Ken",
            HokutoCharacter::Toki => "Hokuto Shinken (Gentle)",
            HokutoCharacter::Mamiya => "Nanto Seiken",
            HokutoCharacter::Shin => "Nanto Koshu Ken",
            HokutoCharacter::Jagi => "Hokuto Shinken (Corrupted)",
            HokutoCharacter::Raoh => "Hokuto Shinken (Gōken)",
            HokutoCharacter::Souther => "Nanto Hōō Ken",
            HokutoCharacter::Thouzer => "Nanto Hōō Ken",
            HokutoCharacter::Ryuga => "Taizan Tenrō Ken",
            HokutoCharacter::Juda => "Nanto Kōkaku Ken",
            HokutoCharacter::Kaioh => "Hokuto Ryū Ken",
            HokutoCharacter::HyohHokuto => "Hokuto Ryū Ken",
        }
    }
}
