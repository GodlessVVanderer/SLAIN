//! NES APU (Audio Processing Unit) - 2A03
//!
//! The APU generates audio output for the NES:
//! - 2 pulse wave channels
//! - 1 triangle wave channel
//! - 1 noise channel
//! - 1 DMC (delta modulation) channel

/// APU sample rate
pub const SAMPLE_RATE: u32 = 44100;
/// CPU frequency
const CPU_FREQ: f64 = 1789773.0;
/// Samples per CPU cycle
const SAMPLES_PER_CYCLE: f64 = SAMPLE_RATE as f64 / CPU_FREQ;

/// Length counter lookup table
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14,
    12, 16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30,
];

/// Pulse duty cycle waveforms
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
    [0, 1, 1, 0, 0, 0, 0, 0], // 25%
    [0, 1, 1, 1, 1, 0, 0, 0], // 50%
    [1, 0, 0, 1, 1, 1, 1, 1], // 75% (inverted 25%)
];

/// Triangle waveform
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
];

/// Noise period lookup table (NTSC)
const NOISE_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

/// DMC rate lookup table (NTSC)
const DMC_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

/// Pulse channel
#[derive(Default, Clone)]
struct Pulse {
    enabled: bool,
    duty: u8,
    length_halt: bool,
    constant_volume: bool,
    volume: u8,
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    timer_period: u16,
    timer: u16,
    length_counter: u8,
    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    sweep_reload: bool,
    sweep_divider: u8,
    sequence_pos: u8,
    channel: u8, // 0 or 1 (for sweep negate difference)
}

impl Pulse {
    fn new(channel: u8) -> Self {
        Self { channel, ..Default::default() }
    }

    fn write_register(&mut self, addr: u16, val: u8) {
        match addr & 3 {
            0 => {
                self.duty = (val >> 6) & 3;
                self.length_halt = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.volume = val & 0x0F;
            }
            1 => {
                self.sweep_enabled = val & 0x80 != 0;
                self.sweep_period = (val >> 4) & 7;
                self.sweep_negate = val & 0x08 != 0;
                self.sweep_shift = val & 0x07;
                self.sweep_reload = true;
            }
            2 => {
                self.timer_period = (self.timer_period & 0x700) | val as u16;
            }
            3 => {
                self.timer_period = (self.timer_period & 0x0FF) | ((val as u16 & 7) << 8);
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.sequence_pos = 0;
                self.envelope_start = true;
            }
            _ => {}
        }
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            self.sequence_pos = (self.sequence_pos + 1) % 8;
        } else {
            self.timer -= 1;
        }
    }

    fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.volume;
        } else if self.envelope_divider > 0 {
            self.envelope_divider -= 1;
        } else {
            self.envelope_divider = self.volume;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.length_halt {
                self.envelope_decay = 15;
            }
        }
    }

    fn clock_length(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn clock_sweep(&mut self) {
        let change = self.timer_period >> self.sweep_shift;
        let target = if self.sweep_negate {
            self.timer_period.wrapping_sub(change).wrapping_sub(if self.channel == 0 { 1 } else { 0 })
        } else {
            self.timer_period.wrapping_add(change)
        };

        let muting = self.timer_period < 8 || target > 0x7FF;

        if self.sweep_divider == 0 && self.sweep_enabled && !muting {
            self.timer_period = target;
        }

        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.timer_period < 8 {
            return 0;
        }

        if DUTY_TABLE[self.duty as usize][self.sequence_pos as usize] == 0 {
            return 0;
        }

        if self.constant_volume {
            self.volume
        } else {
            self.envelope_decay
        }
    }
}

/// Triangle channel
#[derive(Default, Clone)]
struct Triangle {
    enabled: bool,
    control: bool,
    linear_load: u8,
    timer_period: u16,
    timer: u16,
    length_counter: u8,
    linear_counter: u8,
    linear_reload: bool,
    sequence_pos: u8,
}

impl Triangle {
    fn write_register(&mut self, addr: u16, val: u8) {
        match addr & 3 {
            0 => {
                self.control = val & 0x80 != 0;
                self.linear_load = val & 0x7F;
            }
            2 => {
                self.timer_period = (self.timer_period & 0x700) | val as u16;
            }
            3 => {
                self.timer_period = (self.timer_period & 0x0FF) | ((val as u16 & 7) << 8);
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.linear_reload = true;
            }
            _ => {}
        }
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.sequence_pos = (self.sequence_pos + 1) % 32;
            }
        } else {
            self.timer -= 1;
        }
    }

    fn clock_linear(&mut self) {
        if self.linear_reload {
            self.linear_counter = self.linear_load;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control {
            self.linear_reload = false;
        }
    }

    fn clock_length(&mut self) {
        if !self.control && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.linear_counter == 0 {
            return 0;
        }
        TRIANGLE_TABLE[self.sequence_pos as usize]
    }
}

/// Noise channel
#[derive(Clone)]
struct Noise {
    enabled: bool,
    length_halt: bool,
    constant_volume: bool,
    volume: u8,
    mode: bool,
    timer_period: u16,
    timer: u16,
    length_counter: u8,
    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    shift: u16,
}

impl Default for Noise {
    fn default() -> Self {
        Self {
            shift: 1,
            enabled: false,
            length_halt: false,
            constant_volume: false,
            volume: 0,
            mode: false,
            timer_period: 0,
            timer: 0,
            length_counter: 0,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
        }
    }
}

impl Noise {
    fn write_register(&mut self, addr: u16, val: u8) {
        match addr & 3 {
            0 => {
                self.length_halt = val & 0x20 != 0;
                self.constant_volume = val & 0x10 != 0;
                self.volume = val & 0x0F;
            }
            2 => {
                self.mode = val & 0x80 != 0;
                self.timer_period = NOISE_TABLE[(val & 0x0F) as usize];
            }
            3 => {
                if self.enabled {
                    self.length_counter = LENGTH_TABLE[(val >> 3) as usize];
                }
                self.envelope_start = true;
            }
            _ => {}
        }
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.timer_period;
            let bit = if self.mode { 6 } else { 1 };
            let feedback = (self.shift & 1) ^ ((self.shift >> bit) & 1);
            self.shift = (self.shift >> 1) | (feedback << 14);
        } else {
            self.timer -= 1;
        }
    }

    fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.volume;
        } else if self.envelope_divider > 0 {
            self.envelope_divider -= 1;
        } else {
            self.envelope_divider = self.volume;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.length_halt {
                self.envelope_decay = 15;
            }
        }
    }

    fn clock_length(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || self.shift & 1 != 0 {
            return 0;
        }

        if self.constant_volume {
            self.volume
        } else {
            self.envelope_decay
        }
    }
}

/// DMC (Delta Modulation Channel)
#[derive(Default, Clone)]
struct Dmc {
    enabled: bool,
    irq_enabled: bool,
    loop_flag: bool,
    rate: u16,
    timer: u16,
    output_level: u8,
    sample_address: u16,
    sample_length: u16,
    current_address: u16,
    bytes_remaining: u16,
    sample_buffer: u8,
    sample_buffer_empty: bool,
    shift: u8,
    bits_remaining: u8,
    irq_flag: bool,
}

impl Dmc {
    fn write_register(&mut self, addr: u16, val: u8) {
        match addr & 3 {
            0 => {
                self.irq_enabled = val & 0x80 != 0;
                self.loop_flag = val & 0x40 != 0;
                self.rate = DMC_TABLE[(val & 0x0F) as usize];
            }
            1 => {
                self.output_level = val & 0x7F;
            }
            2 => {
                self.sample_address = 0xC000 | ((val as u16) << 6);
            }
            3 => {
                self.sample_length = ((val as u16) << 4) + 1;
            }
            _ => {}
        }
    }

    fn restart(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    fn clock_timer(&mut self) {
        if self.timer == 0 {
            self.timer = self.rate;

            if !self.sample_buffer_empty {
                if self.shift & 1 != 0 {
                    if self.output_level < 126 {
                        self.output_level += 2;
                    }
                } else if self.output_level > 1 {
                    self.output_level -= 2;
                }
                self.shift >>= 1;
                self.bits_remaining -= 1;

                if self.bits_remaining == 0 {
                    self.bits_remaining = 8;
                    self.sample_buffer_empty = true;
                }
            }
        } else {
            self.timer -= 1;
        }
    }

    fn output(&self) -> u8 {
        self.output_level
    }
}

/// APU state
pub struct Apu {
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,
    dmc: Dmc,

    /// Frame counter
    frame_counter: u8,
    frame_mode: bool, // false = 4-step, true = 5-step
    frame_irq_inhibit: bool,
    frame_irq: bool,

    /// Cycle counter
    cycles: u64,

    /// Sample accumulator
    sample_acc: f64,
    samples: Vec<f32>,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            pulse1: Pulse::new(0),
            pulse2: Pulse::new(1),
            triangle: Triangle::default(),
            noise: Noise::default(),
            dmc: Dmc::default(),
            frame_counter: 0,
            frame_mode: false,
            frame_irq_inhibit: false,
            frame_irq: false,
            cycles: 0,
            sample_acc: 0.0,
            samples: Vec::with_capacity(1024),
        }
    }

    pub fn reset(&mut self) {
        self.pulse1 = Pulse::new(0);
        self.pulse2 = Pulse::new(1);
        self.triangle = Triangle::default();
        self.noise = Noise::default();
        self.dmc = Dmc::default();
        self.frame_counter = 0;
        self.cycles = 0;
        self.samples.clear();
    }

    /// Execute one APU cycle
    pub fn step(&mut self) {
        // Triangle clocks every CPU cycle
        self.triangle.clock_timer();

        // Other channels clock every other cycle
        if self.cycles % 2 == 0 {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
            self.dmc.clock_timer();
        }

        // Frame counter
        self.step_frame_counter();

        // Generate sample
        self.sample_acc += SAMPLES_PER_CYCLE;
        if self.sample_acc >= 1.0 {
            self.sample_acc -= 1.0;
            self.samples.push(self.mix());
        }

        self.cycles += 1;
    }

    fn step_frame_counter(&mut self) {
        // Frame counter runs at ~240 Hz
        let frame_period = if self.frame_mode { 18641 } else { 14915 };

        if self.cycles % frame_period == 0 {
            self.frame_counter = (self.frame_counter + 1) % if self.frame_mode { 5 } else { 4 };

            match self.frame_counter {
                0 | 2 => {
                    // Quarter frame - clock envelopes and triangle linear
                    self.clock_envelopes();
                }
                1 | 3 => {
                    // Half frame - clock length counters and sweep
                    self.clock_envelopes();
                    self.clock_lengths();
                    self.clock_sweeps();
                }
                4 => {
                    // 5-step mode only
                    if self.frame_mode {
                        self.clock_envelopes();
                        self.clock_lengths();
                        self.clock_sweeps();
                    }
                }
                _ => {}
            }

            // Frame IRQ (4-step mode only)
            if !self.frame_mode && self.frame_counter == 3 && !self.frame_irq_inhibit {
                self.frame_irq = true;
            }
        }
    }

    fn clock_envelopes(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.triangle.clock_linear();
        self.noise.clock_envelope();
    }

    fn clock_lengths(&mut self) {
        self.pulse1.clock_length();
        self.pulse2.clock_length();
        self.triangle.clock_length();
        self.noise.clock_length();
    }

    fn clock_sweeps(&mut self) {
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
    }

    /// Mix all channels to output sample
    fn mix(&self) -> f32 {
        let pulse1 = self.pulse1.output() as f32;
        let pulse2 = self.pulse2.output() as f32;
        let triangle = self.triangle.output() as f32;
        let noise = self.noise.output() as f32;
        let dmc = self.dmc.output() as f32;

        // Approximate mixer formula from NES APU Mixer wiki
        let pulse_out = if pulse1 + pulse2 > 0.0 {
            95.88 / ((8128.0 / (pulse1 + pulse2)) + 100.0)
        } else {
            0.0
        };

        let tnd_out = if triangle + noise + dmc > 0.0 {
            159.79 / ((1.0 / ((triangle / 8227.0) + (noise / 12241.0) + (dmc / 22638.0))) + 100.0)
        } else {
            0.0
        };

        (pulse_out + tnd_out) * 2.0 - 1.0
    }

    /// Read APU register
    pub fn read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x4015 => {
                let mut val = 0u8;
                if self.pulse1.length_counter > 0 { val |= 0x01; }
                if self.pulse2.length_counter > 0 { val |= 0x02; }
                if self.triangle.length_counter > 0 { val |= 0x04; }
                if self.noise.length_counter > 0 { val |= 0x08; }
                if self.dmc.bytes_remaining > 0 { val |= 0x10; }
                if self.frame_irq { val |= 0x40; self.frame_irq = false; }
                if self.dmc.irq_flag { val |= 0x80; }
                val
            }
            _ => 0,
        }
    }

    /// Write APU register
    pub fn write_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x4000..=0x4003 => self.pulse1.write_register(addr, val),
            0x4004..=0x4007 => self.pulse2.write_register(addr, val),
            0x4008..=0x400B => self.triangle.write_register(addr, val),
            0x400C..=0x400F => self.noise.write_register(addr, val),
            0x4010..=0x4013 => self.dmc.write_register(addr, val),
            0x4015 => {
                self.pulse1.enabled = val & 0x01 != 0;
                self.pulse2.enabled = val & 0x02 != 0;
                self.triangle.enabled = val & 0x04 != 0;
                self.noise.enabled = val & 0x08 != 0;
                self.dmc.enabled = val & 0x10 != 0;

                if !self.pulse1.enabled { self.pulse1.length_counter = 0; }
                if !self.pulse2.enabled { self.pulse2.length_counter = 0; }
                if !self.triangle.enabled { self.triangle.length_counter = 0; }
                if !self.noise.enabled { self.noise.length_counter = 0; }
                if self.dmc.enabled && self.dmc.bytes_remaining == 0 {
                    self.dmc.restart();
                }

                self.dmc.irq_flag = false;
            }
            0x4017 => {
                self.frame_mode = val & 0x80 != 0;
                self.frame_irq_inhibit = val & 0x40 != 0;
                if self.frame_irq_inhibit {
                    self.frame_irq = false;
                }
                self.frame_counter = 0;

                if self.frame_mode {
                    self.clock_envelopes();
                    self.clock_lengths();
                    self.clock_sweeps();
                }
            }
            _ => {}
        }
    }

    /// Get accumulated audio samples and clear buffer
    pub fn get_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }

    /// Check for IRQ
    pub fn irq(&self) -> bool {
        self.frame_irq || self.dmc.irq_flag
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}
