//! SMS PSG (Programmable Sound Generator) - SN76489
//!
//! 4-channel sound chip:
//! - 3 square wave channels
//! - 1 noise channel

const SAMPLE_RATE: u32 = 44100;
const CPU_FREQ: f64 = 3579545.0;
const SAMPLES_PER_CYCLE: f64 = SAMPLE_RATE as f64 / CPU_FREQ;

/// Volume table (logarithmic attenuation)
const VOLUME_TABLE: [f32; 16] = [
    1.0, 0.794, 0.631, 0.501, 0.398, 0.316, 0.251, 0.200,
    0.158, 0.126, 0.100, 0.079, 0.063, 0.050, 0.040, 0.0,
];

pub struct Psg {
    /// Tone registers (10-bit period)
    tone_period: [u16; 3],
    /// Tone counters
    tone_counter: [u16; 3],
    /// Tone output (high/low)
    tone_output: [bool; 3],
    /// Noise register
    noise_mode: u8,
    /// Noise shift register
    noise_shift: u16,
    /// Noise counter
    noise_counter: u16,
    /// Volume registers (4-bit attenuation)
    volume: [u8; 4],
    /// Latched channel
    latched_channel: u8,
    /// Latched type (0=tone, 1=volume)
    latched_type: bool,
    /// Sample accumulator
    sample_acc: f64,
    /// Output samples
    samples: Vec<f32>,
}

impl Psg {
    pub fn new() -> Self {
        Self {
            tone_period: [0; 3],
            tone_counter: [0; 3],
            tone_output: [false; 3],
            noise_mode: 0,
            noise_shift: 0x8000,
            noise_counter: 0,
            volume: [0x0F; 4], // All muted
            latched_channel: 0,
            latched_type: false,
            sample_acc: 0.0,
            samples: Vec::with_capacity(1024),
        }
    }

    pub fn reset(&mut self) {
        self.tone_period.fill(0);
        self.tone_counter.fill(0);
        self.tone_output.fill(false);
        self.noise_mode = 0;
        self.noise_shift = 0x8000;
        self.noise_counter = 0;
        self.volume.fill(0x0F);
        self.samples.clear();
    }

    pub fn write(&mut self, val: u8) {
        if val & 0x80 != 0 {
            // Latch/data byte
            self.latched_channel = (val >> 5) & 3;
            self.latched_type = val & 0x10 != 0;

            if self.latched_type {
                // Volume
                self.volume[self.latched_channel as usize] = val & 0x0F;
            } else if self.latched_channel < 3 {
                // Tone low bits
                self.tone_period[self.latched_channel as usize] =
                    (self.tone_period[self.latched_channel as usize] & 0x3F0) | (val as u16 & 0x0F);
            } else {
                // Noise
                self.noise_mode = val & 0x07;
                self.noise_shift = 0x8000;
            }
        } else {
            // Data byte
            if self.latched_type {
                self.volume[self.latched_channel as usize] = val & 0x0F;
            } else if self.latched_channel < 3 {
                // Tone high bits
                self.tone_period[self.latched_channel as usize] =
                    (self.tone_period[self.latched_channel as usize] & 0x00F) | ((val as u16 & 0x3F) << 4);
            }
        }
    }

    pub fn step(&mut self) {
        // Clock tone channels
        for i in 0..3 {
            if self.tone_counter[i] > 0 {
                self.tone_counter[i] -= 1;
            } else {
                self.tone_counter[i] = self.tone_period[i];
                self.tone_output[i] = !self.tone_output[i];
            }
        }

        // Clock noise channel
        let noise_period = match self.noise_mode & 3 {
            0 => 0x10,
            1 => 0x20,
            2 => 0x40,
            3 => self.tone_period[2],
            _ => 0x10,
        };

        if self.noise_counter > 0 {
            self.noise_counter -= 1;
        } else {
            self.noise_counter = noise_period;

            // Shift noise register
            let feedback = if self.noise_mode & 0x04 != 0 {
                // White noise
                (self.noise_shift & 1) ^ ((self.noise_shift >> 3) & 1)
            } else {
                // Periodic noise
                self.noise_shift & 1
            };

            self.noise_shift = (self.noise_shift >> 1) | (feedback << 15);
        }

        // Generate sample
        self.sample_acc += SAMPLES_PER_CYCLE;
        if self.sample_acc >= 1.0 {
            self.sample_acc -= 1.0;
            self.samples.push(self.mix());
        }
    }

    fn mix(&self) -> f32 {
        let mut output = 0.0f32;

        // Tone channels
        for i in 0..3 {
            if self.tone_output[i] && self.tone_period[i] > 1 {
                output += VOLUME_TABLE[self.volume[i] as usize];
            }
        }

        // Noise channel
        if self.noise_shift & 1 != 0 {
            output += VOLUME_TABLE[self.volume[3] as usize];
        }

        // Normalize to -1.0 to 1.0
        (output / 4.0) * 2.0 - 1.0
    }

    pub fn get_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.samples)
    }
}

impl Default for Psg {
    fn default() -> Self {
        Self::new()
    }
}
