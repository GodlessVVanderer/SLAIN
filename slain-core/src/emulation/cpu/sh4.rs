//! Hitachi SH-4 CPU Emulator
//!
//! The SH-4 is a 32-bit RISC CPU used in the Sega Dreamcast and Atomiswave.
//! - 200 MHz clock speed
//! - 16 general-purpose registers (R0-R15)
//! - Banked registers for fast interrupt handling
//! - FPU with 32 single-precision registers
//! - 8KB instruction cache, 16KB data cache

use super::Cpu;

/// Memory bus trait for SH-4
pub trait BusSh4 {
    fn read8(&mut self, addr: u32) -> u8;
    fn read16(&mut self, addr: u32) -> u16;
    fn read32(&mut self, addr: u32) -> u32;
    fn read64(&mut self, addr: u32) -> u64;
    fn write8(&mut self, addr: u32, val: u8);
    fn write16(&mut self, addr: u32, val: u16);
    fn write32(&mut self, addr: u32, val: u32);
    fn write64(&mut self, addr: u32, val: u64);
}

/// SH-4 Status Register bits
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusRegister {
    /// T bit - used for conditionals
    pub t: bool,
    /// S bit - saturation flag
    pub s: bool,
    /// Interrupt mask bits (4 bits)
    pub imask: u8,
    /// Q bit - divide step quotient
    pub q: bool,
    /// M bit - divide step
    pub m: bool,
    /// FD bit - FPU disable
    pub fd: bool,
    /// BL bit - block exceptions
    pub bl: bool,
    /// RB bit - register bank
    pub rb: bool,
    /// MD bit - processor mode (0=user, 1=privileged)
    pub md: bool,
}

impl StatusRegister {
    pub fn to_u32(&self) -> u32 {
        let mut val = 0u32;
        if self.t { val |= 1 << 0; }
        if self.s { val |= 1 << 1; }
        val |= (self.imask as u32 & 0xF) << 4;
        if self.q { val |= 1 << 8; }
        if self.m { val |= 1 << 9; }
        if self.fd { val |= 1 << 15; }
        if self.bl { val |= 1 << 28; }
        if self.rb { val |= 1 << 29; }
        if self.md { val |= 1 << 30; }
        val
    }

    pub fn from_u32(val: u32) -> Self {
        Self {
            t: val & (1 << 0) != 0,
            s: val & (1 << 1) != 0,
            imask: ((val >> 4) & 0xF) as u8,
            q: val & (1 << 8) != 0,
            m: val & (1 << 9) != 0,
            fd: val & (1 << 15) != 0,
            bl: val & (1 << 28) != 0,
            rb: val & (1 << 29) != 0,
            md: val & (1 << 30) != 0,
        }
    }
}

/// FPSCR - Floating Point Status/Control Register
#[derive(Debug, Clone, Copy, Default)]
pub struct Fpscr {
    /// Rounding mode (0=nearest, 1=zero)
    pub rm: u8,
    /// Inexact exception flag
    pub flag_inexact: bool,
    /// Underflow exception flag
    pub flag_underflow: bool,
    /// Overflow exception flag
    pub flag_overflow: bool,
    /// Division by zero flag
    pub flag_divzero: bool,
    /// Invalid operation flag
    pub flag_invalid: bool,
    /// Inexact exception enable
    pub enable_inexact: bool,
    /// Underflow exception enable
    pub enable_underflow: bool,
    /// Overflow exception enable
    pub enable_overflow: bool,
    /// Division by zero enable
    pub enable_divzero: bool,
    /// Invalid operation enable
    pub enable_invalid: bool,
    /// Cause inexact
    pub cause_inexact: bool,
    /// Cause underflow
    pub cause_underflow: bool,
    /// Cause overflow
    pub cause_overflow: bool,
    /// Cause divzero
    pub cause_divzero: bool,
    /// Cause invalid
    pub cause_invalid: bool,
    /// Cause FPU error
    pub cause_fpu_error: bool,
    /// Denormalization mode
    pub dn: bool,
    /// Precision mode (0=single, 1=double)
    pub pr: bool,
    /// SZ - transfer size (0=32bit, 1=64bit)
    pub sz: bool,
    /// FR - FPU register bank
    pub fr: bool,
}

impl Fpscr {
    pub fn to_u32(&self) -> u32 {
        let mut val = 0u32;
        val |= (self.rm as u32) & 0x3;
        if self.flag_inexact { val |= 1 << 2; }
        if self.flag_underflow { val |= 1 << 3; }
        if self.flag_overflow { val |= 1 << 4; }
        if self.flag_divzero { val |= 1 << 5; }
        if self.flag_invalid { val |= 1 << 6; }
        if self.enable_inexact { val |= 1 << 7; }
        if self.enable_underflow { val |= 1 << 8; }
        if self.enable_overflow { val |= 1 << 9; }
        if self.enable_divzero { val |= 1 << 10; }
        if self.enable_invalid { val |= 1 << 11; }
        if self.cause_inexact { val |= 1 << 12; }
        if self.cause_underflow { val |= 1 << 13; }
        if self.cause_overflow { val |= 1 << 14; }
        if self.cause_divzero { val |= 1 << 15; }
        if self.cause_invalid { val |= 1 << 16; }
        if self.cause_fpu_error { val |= 1 << 17; }
        if self.dn { val |= 1 << 18; }
        if self.pr { val |= 1 << 19; }
        if self.sz { val |= 1 << 20; }
        if self.fr { val |= 1 << 21; }
        val
    }

    pub fn from_u32(val: u32) -> Self {
        Self {
            rm: (val & 0x3) as u8,
            flag_inexact: val & (1 << 2) != 0,
            flag_underflow: val & (1 << 3) != 0,
            flag_overflow: val & (1 << 4) != 0,
            flag_divzero: val & (1 << 5) != 0,
            flag_invalid: val & (1 << 6) != 0,
            enable_inexact: val & (1 << 7) != 0,
            enable_underflow: val & (1 << 8) != 0,
            enable_overflow: val & (1 << 9) != 0,
            enable_divzero: val & (1 << 10) != 0,
            enable_invalid: val & (1 << 11) != 0,
            cause_inexact: val & (1 << 12) != 0,
            cause_underflow: val & (1 << 13) != 0,
            cause_overflow: val & (1 << 14) != 0,
            cause_divzero: val & (1 << 15) != 0,
            cause_invalid: val & (1 << 16) != 0,
            cause_fpu_error: val & (1 << 17) != 0,
            dn: val & (1 << 18) != 0,
            pr: val & (1 << 19) != 0,
            sz: val & (1 << 20) != 0,
            fr: val & (1 << 21) != 0,
        }
    }
}

/// SH-4 CPU State
pub struct Sh4<B: BusSh4> {
    /// General purpose registers (R0-R15)
    pub r: [u32; 16],
    /// Banked registers (R0_BANK-R7_BANK)
    pub r_bank: [u32; 8],
    /// Status register
    pub sr: StatusRegister,
    /// Saved status register
    pub ssr: u32,
    /// Saved program counter
    pub spc: u32,
    /// Global base register
    pub gbr: u32,
    /// Vector base register
    pub vbr: u32,
    /// Debug base register
    pub dbr: u32,
    /// Procedure register (return address)
    pub pr: u32,
    /// MAC registers (multiply-accumulate)
    pub mach: u32,
    pub macl: u32,
    /// Program counter
    pub pc: u32,
    /// Floating point registers (32 single-precision)
    pub fr: [f32; 16],
    /// Floating point registers bank 1
    pub xf: [f32; 16],
    /// Floating point status/control register
    pub fpscr: Fpscr,
    /// Floating point communication register
    pub fpul: u32,
    /// Memory bus
    pub bus: B,
    /// Total cycles
    cycles: u64,
    /// Delay slot PC (for delayed branches)
    delay_slot: Option<u32>,
    /// Pending interrupt
    irq_pending: bool,
    irq_level: u8,
}

impl<B: BusSh4> Sh4<B> {
    pub fn new(bus: B) -> Self {
        Self {
            r: [0; 16],
            r_bank: [0; 8],
            sr: StatusRegister::default(),
            ssr: 0,
            spc: 0,
            gbr: 0,
            vbr: 0,
            dbr: 0,
            pr: 0,
            mach: 0,
            macl: 0,
            pc: 0xA0000000, // Reset vector
            fr: [0.0; 16],
            xf: [0.0; 16],
            fpscr: Fpscr::default(),
            fpul: 0,
            bus,
            cycles: 0,
            delay_slot: None,
            irq_pending: false,
            irq_level: 0,
        }
    }

    /// Fetch 16-bit instruction
    fn fetch(&mut self) -> u16 {
        let instr = self.bus.read16(self.pc);
        self.pc = self.pc.wrapping_add(2);
        instr
    }

    /// Get register value (handles banking)
    fn get_reg(&self, n: usize) -> u32 {
        if n < 8 && self.sr.rb {
            self.r_bank[n]
        } else {
            self.r[n]
        }
    }

    /// Set register value (handles banking)
    fn set_reg(&mut self, n: usize, val: u32) {
        if n < 8 && self.sr.rb {
            self.r_bank[n] = val;
        } else {
            self.r[n] = val;
        }
    }

    /// Get FR register (handles FR bit)
    fn get_fr(&self, n: usize) -> f32 {
        if self.fpscr.fr {
            self.xf[n]
        } else {
            self.fr[n]
        }
    }

    /// Set FR register (handles FR bit)
    fn set_fr(&mut self, n: usize, val: f32) {
        if self.fpscr.fr {
            self.xf[n] = val;
        } else {
            self.fr[n] = val;
        }
    }

    /// Get DR register (double precision)
    fn get_dr(&self, n: usize) -> f64 {
        let idx = n * 2;
        let hi = if self.fpscr.fr { self.xf[idx] } else { self.fr[idx] };
        let lo = if self.fpscr.fr { self.xf[idx + 1] } else { self.fr[idx + 1] };
        let bits = ((hi.to_bits() as u64) << 32) | (lo.to_bits() as u64);
        f64::from_bits(bits)
    }

    /// Set DR register (double precision)
    fn set_dr(&mut self, n: usize, val: f64) {
        let bits = val.to_bits();
        let hi = f32::from_bits((bits >> 32) as u32);
        let lo = f32::from_bits(bits as u32);
        let idx = n * 2;
        if self.fpscr.fr {
            self.xf[idx] = hi;
            self.xf[idx + 1] = lo;
        } else {
            self.fr[idx] = hi;
            self.fr[idx + 1] = lo;
        }
    }

    /// Sign extend 8-bit to 32-bit
    fn sign_extend8(val: u8) -> u32 {
        val as i8 as i32 as u32
    }

    /// Sign extend 12-bit to 32-bit
    fn sign_extend12(val: u16) -> u32 {
        if val & 0x800 != 0 {
            (val as u32) | 0xFFFFF000
        } else {
            val as u32
        }
    }

    /// Sign extend 16-bit to 32-bit
    fn sign_extend16(val: u16) -> u32 {
        val as i16 as i32 as u32
    }

    /// Execute delayed branch
    fn delay_branch(&mut self, target: u32) {
        self.delay_slot = Some(target);
    }

    /// Execute one instruction
    pub fn execute(&mut self) -> u32 {
        // Handle delayed branch
        if let Some(target) = self.delay_slot.take() {
            let instr = self.fetch();
            let cycles = self.execute_instruction(instr);
            self.pc = target;
            return cycles;
        }

        let instr = self.fetch();
        self.execute_instruction(instr)
    }

    fn execute_instruction(&mut self, instr: u16) -> u32 {
        let op = (instr >> 12) & 0xF;

        match op {
            0x0 => self.execute_0xxx(instr),
            0x1 => self.execute_1xxx(instr),
            0x2 => self.execute_2xxx(instr),
            0x3 => self.execute_3xxx(instr),
            0x4 => self.execute_4xxx(instr),
            0x5 => self.execute_5xxx(instr),
            0x6 => self.execute_6xxx(instr),
            0x7 => self.execute_7xxx(instr),
            0x8 => self.execute_8xxx(instr),
            0x9 => self.execute_9xxx(instr),
            0xA => self.execute_axxx(instr),
            0xB => self.execute_bxxx(instr),
            0xC => self.execute_cxxx(instr),
            0xD => self.execute_dxxx(instr),
            0xE => self.execute_exxx(instr),
            0xF => self.execute_fxxx(instr),
            _ => unreachable!(),
        }
    }

    fn execute_0xxx(&mut self, instr: u16) -> u32 {
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let op = instr & 0xF;

        match (instr >> 4) & 0xFF {
            0x00 => match op {
                0x2 => { // STC SR,Rn
                    self.set_reg(n, self.sr.to_u32());
                    1
                }
                0x3 => { // BSRF Rm
                    self.pr = self.pc.wrapping_add(2);
                    let target = self.pc.wrapping_add(self.get_reg(n));
                    self.delay_branch(target);
                    2
                }
                0x8 => { // CLRT
                    self.sr.t = false;
                    1
                }
                0x9 => { // NOP
                    1
                }
                0xA => { // STS MACH,Rn
                    self.set_reg(n, self.mach);
                    1
                }
                0xB => { // RTS
                    self.delay_branch(self.pr);
                    2
                }
                _ => 1,
            },
            0x01 => match op {
                0x2 => { // STC GBR,Rn
                    self.set_reg(n, self.gbr);
                    1
                }
                0x8 => { // SETT
                    self.sr.t = true;
                    1
                }
                0x9 => { // DIV0U
                    self.sr.m = false;
                    self.sr.q = false;
                    self.sr.t = false;
                    1
                }
                0xA => { // STS MACL,Rn
                    self.set_reg(n, self.macl);
                    1
                }
                _ => 1,
            },
            0x02 => match op {
                0x2 => { // STC VBR,Rn
                    self.set_reg(n, self.vbr);
                    1
                }
                0x3 => { // BRAF Rm
                    let target = self.pc.wrapping_add(self.get_reg(n));
                    self.delay_branch(target);
                    2
                }
                0x8 => { // CLRMAC
                    self.mach = 0;
                    self.macl = 0;
                    1
                }
                0xA => { // STS PR,Rn
                    self.set_reg(n, self.pr);
                    1
                }
                0xB => { // RTE
                    self.pc = self.spc;
                    self.sr = StatusRegister::from_u32(self.ssr);
                    4
                }
                _ => 1,
            },
            0x03 => match op {
                0x2 => { // STC SSR,Rn
                    self.set_reg(n, self.ssr);
                    1
                }
                0x8 => { // LDTLB
                    1 // TLB not implemented
                }
                _ => 1,
            },
            0x04 => match op {
                0x2 => { // STC SPC,Rn
                    self.set_reg(n, self.spc);
                    1
                }
                _ => 1,
            },
            0x05 => match op {
                0xA => { // STS FPUL,Rn
                    self.set_reg(n, self.fpul);
                    1
                }
                _ => 1,
            },
            0x06 => match op {
                0xA => { // STS FPSCR,Rn
                    self.set_reg(n, self.fpscr.to_u32());
                    1
                }
                _ => 1,
            },
            0x07 => match op {
                0xC => { // MOV.B @(R0,Rm),Rn
                    let addr = self.r[0].wrapping_add(self.get_reg(m));
                    let val = self.bus.read8(addr);
                    self.set_reg(n, Self::sign_extend8(val));
                    1
                }
                0xD => { // MOV.W @(R0,Rm),Rn
                    let addr = self.r[0].wrapping_add(self.get_reg(m));
                    let val = self.bus.read16(addr);
                    self.set_reg(n, Self::sign_extend16(val));
                    1
                }
                0xE => { // MOV.L @(R0,Rm),Rn
                    let addr = self.r[0].wrapping_add(self.get_reg(m));
                    let val = self.bus.read32(addr);
                    self.set_reg(n, val);
                    1
                }
                _ => 1,
            },
            _ => {
                match op {
                    0x4 => { // MOV.B Rm,@(R0,Rn)
                        let addr = self.r[0].wrapping_add(self.get_reg(n));
                        self.bus.write8(addr, self.get_reg(m) as u8);
                        1
                    }
                    0x5 => { // MOV.W Rm,@(R0,Rn)
                        let addr = self.r[0].wrapping_add(self.get_reg(n));
                        self.bus.write16(addr, self.get_reg(m) as u16);
                        1
                    }
                    0x6 => { // MOV.L Rm,@(R0,Rn)
                        let addr = self.r[0].wrapping_add(self.get_reg(n));
                        self.bus.write32(addr, self.get_reg(m));
                        1
                    }
                    0x7 => { // MUL.L Rm,Rn
                        let result = (self.get_reg(n) as i32).wrapping_mul(self.get_reg(m) as i32);
                        self.macl = result as u32;
                        2
                    }
                    0xC => { // MOV.B @(R0,Rm),Rn
                        let addr = self.r[0].wrapping_add(self.get_reg(m));
                        let val = self.bus.read8(addr);
                        self.set_reg(n, Self::sign_extend8(val));
                        1
                    }
                    0xD => { // MOV.W @(R0,Rm),Rn
                        let addr = self.r[0].wrapping_add(self.get_reg(m));
                        let val = self.bus.read16(addr);
                        self.set_reg(n, Self::sign_extend16(val));
                        1
                    }
                    0xE => { // MOV.L @(R0,Rm),Rn
                        let addr = self.r[0].wrapping_add(self.get_reg(m));
                        let val = self.bus.read32(addr);
                        self.set_reg(n, val);
                        1
                    }
                    0xF => { // MAC.L @Rm+,@Rn+
                        let val1 = self.bus.read32(self.get_reg(n)) as i32 as i64;
                        let val2 = self.bus.read32(self.get_reg(m)) as i32 as i64;
                        self.set_reg(n, self.get_reg(n).wrapping_add(4));
                        self.set_reg(m, self.get_reg(m).wrapping_add(4));
                        let mac = ((self.mach as i64) << 32) | (self.macl as i64);
                        let result = mac.wrapping_add(val1.wrapping_mul(val2));
                        self.mach = (result >> 32) as u32;
                        self.macl = result as u32;
                        3
                    }
                    _ => 1,
                }
            }
        }
    }

    fn execute_1xxx(&mut self, instr: u16) -> u32 {
        // MOV.L Rm,@(disp,Rn)
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let disp = (instr & 0xF) as u32 * 4;
        let addr = self.get_reg(n).wrapping_add(disp);
        self.bus.write32(addr, self.get_reg(m));
        1
    }

    fn execute_2xxx(&mut self, instr: u16) -> u32 {
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let op = instr & 0xF;

        match op {
            0x0 => { // MOV.B Rm,@Rn
                self.bus.write8(self.get_reg(n), self.get_reg(m) as u8);
                1
            }
            0x1 => { // MOV.W Rm,@Rn
                self.bus.write16(self.get_reg(n), self.get_reg(m) as u16);
                1
            }
            0x2 => { // MOV.L Rm,@Rn
                self.bus.write32(self.get_reg(n), self.get_reg(m));
                1
            }
            0x4 => { // MOV.B Rm,@-Rn
                let addr = self.get_reg(n).wrapping_sub(1);
                self.set_reg(n, addr);
                self.bus.write8(addr, self.get_reg(m) as u8);
                1
            }
            0x5 => { // MOV.W Rm,@-Rn
                let addr = self.get_reg(n).wrapping_sub(2);
                self.set_reg(n, addr);
                self.bus.write16(addr, self.get_reg(m) as u16);
                1
            }
            0x6 => { // MOV.L Rm,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.get_reg(m));
                1
            }
            0x7 => { // DIV0S Rm,Rn
                self.sr.q = (self.get_reg(n) >> 31) != 0;
                self.sr.m = (self.get_reg(m) >> 31) != 0;
                self.sr.t = self.sr.q != self.sr.m;
                1
            }
            0x8 => { // TST Rm,Rn
                self.sr.t = (self.get_reg(n) & self.get_reg(m)) == 0;
                1
            }
            0x9 => { // AND Rm,Rn
                self.set_reg(n, self.get_reg(n) & self.get_reg(m));
                1
            }
            0xA => { // XOR Rm,Rn
                self.set_reg(n, self.get_reg(n) ^ self.get_reg(m));
                1
            }
            0xB => { // OR Rm,Rn
                self.set_reg(n, self.get_reg(n) | self.get_reg(m));
                1
            }
            0xC => { // CMP/STR Rm,Rn
                let temp = self.get_reg(n) ^ self.get_reg(m);
                let hh = (temp >> 24) & 0xFF;
                let hl = (temp >> 16) & 0xFF;
                let lh = (temp >> 8) & 0xFF;
                let ll = temp & 0xFF;
                self.sr.t = hh == 0 || hl == 0 || lh == 0 || ll == 0;
                1
            }
            0xD => { // XTRCT Rm,Rn
                let result = ((self.get_reg(m) << 16) & 0xFFFF0000) |
                             ((self.get_reg(n) >> 16) & 0x0000FFFF);
                self.set_reg(n, result);
                1
            }
            0xE => { // MULU.W Rm,Rn
                let result = (self.get_reg(n) as u16 as u32) * (self.get_reg(m) as u16 as u32);
                self.macl = result;
                1
            }
            0xF => { // MULS.W Rm,Rn
                let result = (self.get_reg(n) as i16 as i32) * (self.get_reg(m) as i16 as i32);
                self.macl = result as u32;
                1
            }
            _ => 1,
        }
    }

    fn execute_3xxx(&mut self, instr: u16) -> u32 {
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let op = instr & 0xF;

        match op {
            0x0 => { // CMP/EQ Rm,Rn
                self.sr.t = self.get_reg(n) == self.get_reg(m);
                1
            }
            0x2 => { // CMP/HS Rm,Rn (unsigned >=)
                self.sr.t = self.get_reg(n) >= self.get_reg(m);
                1
            }
            0x3 => { // CMP/GE Rm,Rn (signed >=)
                self.sr.t = (self.get_reg(n) as i32) >= (self.get_reg(m) as i32);
                1
            }
            0x4 => { // DIV1 Rm,Rn
                let old_q = self.sr.q;
                self.sr.q = (self.get_reg(n) >> 31) != 0;
                let mut rn = (self.get_reg(n) << 1) | (if self.sr.t { 1 } else { 0 });

                if old_q == self.sr.m {
                    rn = rn.wrapping_sub(self.get_reg(m));
                } else {
                    rn = rn.wrapping_add(self.get_reg(m));
                }

                self.sr.q = ((rn >> 31) != 0) != self.sr.m;
                self.sr.t = self.sr.q == self.sr.m;
                self.set_reg(n, rn);
                1
            }
            0x5 => { // DMULU.L Rm,Rn
                let result = (self.get_reg(n) as u64) * (self.get_reg(m) as u64);
                self.mach = (result >> 32) as u32;
                self.macl = result as u32;
                2
            }
            0x6 => { // CMP/HI Rm,Rn (unsigned >)
                self.sr.t = self.get_reg(n) > self.get_reg(m);
                1
            }
            0x7 => { // CMP/GT Rm,Rn (signed >)
                self.sr.t = (self.get_reg(n) as i32) > (self.get_reg(m) as i32);
                1
            }
            0x8 => { // SUB Rm,Rn
                self.set_reg(n, self.get_reg(n).wrapping_sub(self.get_reg(m)));
                1
            }
            0xA => { // SUBC Rm,Rn
                let rn = self.get_reg(n);
                let rm = self.get_reg(m);
                let t = if self.sr.t { 1u32 } else { 0 };
                let result = rn.wrapping_sub(rm).wrapping_sub(t);
                self.sr.t = (rn < rm) || (rn == rm && t == 1);
                self.set_reg(n, result);
                1
            }
            0xB => { // SUBV Rm,Rn
                let rn = self.get_reg(n) as i32;
                let rm = self.get_reg(m) as i32;
                let (result, overflow) = rn.overflowing_sub(rm);
                self.sr.t = overflow;
                self.set_reg(n, result as u32);
                1
            }
            0xC => { // ADD Rm,Rn
                self.set_reg(n, self.get_reg(n).wrapping_add(self.get_reg(m)));
                1
            }
            0xD => { // DMULS.L Rm,Rn
                let result = (self.get_reg(n) as i32 as i64) * (self.get_reg(m) as i32 as i64);
                self.mach = (result >> 32) as u32;
                self.macl = result as u32;
                2
            }
            0xE => { // ADDC Rm,Rn
                let rn = self.get_reg(n);
                let rm = self.get_reg(m);
                let t = if self.sr.t { 1u32 } else { 0 };
                let result = rn.wrapping_add(rm).wrapping_add(t);
                self.sr.t = result < rn || (result == rn && t == 1);
                self.set_reg(n, result);
                1
            }
            0xF => { // ADDV Rm,Rn
                let rn = self.get_reg(n) as i32;
                let rm = self.get_reg(m) as i32;
                let (result, overflow) = rn.overflowing_add(rm);
                self.sr.t = overflow;
                self.set_reg(n, result as u32);
                1
            }
            _ => 1,
        }
    }

    fn execute_4xxx(&mut self, instr: u16) -> u32 {
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let op = instr & 0xFF;

        match op {
            0x00 => { // SHLL Rn
                self.sr.t = (self.get_reg(n) >> 31) != 0;
                self.set_reg(n, self.get_reg(n) << 1);
                1
            }
            0x01 => { // SHLR Rn
                self.sr.t = (self.get_reg(n) & 1) != 0;
                self.set_reg(n, self.get_reg(n) >> 1);
                1
            }
            0x02 => { // STS.L MACH,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.mach);
                1
            }
            0x03 => { // STC.L SR,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.sr.to_u32());
                1
            }
            0x04 => { // ROTL Rn
                self.sr.t = (self.get_reg(n) >> 31) != 0;
                let result = (self.get_reg(n) << 1) | (if self.sr.t { 1 } else { 0 });
                self.set_reg(n, result);
                1
            }
            0x05 => { // ROTR Rn
                self.sr.t = (self.get_reg(n) & 1) != 0;
                let result = (self.get_reg(n) >> 1) | (if self.sr.t { 0x80000000 } else { 0 });
                self.set_reg(n, result);
                1
            }
            0x06 => { // LDS.L @Rm+,MACH
                self.mach = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x07 => { // LDC.L @Rm+,SR
                let val = self.bus.read32(self.get_reg(n));
                self.sr = StatusRegister::from_u32(val);
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                3
            }
            0x08 => { // SHLL2 Rn
                self.set_reg(n, self.get_reg(n) << 2);
                1
            }
            0x09 => { // SHLR2 Rn
                self.set_reg(n, self.get_reg(n) >> 2);
                1
            }
            0x0A => { // LDS Rm,MACH
                self.mach = self.get_reg(n);
                1
            }
            0x0B => { // JSR @Rn
                self.pr = self.pc.wrapping_add(2);
                self.delay_branch(self.get_reg(n));
                2
            }
            0x0E => { // LDC Rm,SR
                self.sr = StatusRegister::from_u32(self.get_reg(n));
                1
            }
            0x10 => { // DT Rn
                let result = self.get_reg(n).wrapping_sub(1);
                self.set_reg(n, result);
                self.sr.t = result == 0;
                1
            }
            0x11 => { // CMP/PZ Rn
                self.sr.t = (self.get_reg(n) as i32) >= 0;
                1
            }
            0x12 => { // STS.L MACL,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.macl);
                1
            }
            0x13 => { // STC.L GBR,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.gbr);
                1
            }
            0x15 => { // CMP/PL Rn
                self.sr.t = (self.get_reg(n) as i32) > 0;
                1
            }
            0x16 => { // LDS.L @Rm+,MACL
                self.macl = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x17 => { // LDC.L @Rm+,GBR
                self.gbr = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x18 => { // SHLL8 Rn
                self.set_reg(n, self.get_reg(n) << 8);
                1
            }
            0x19 => { // SHLR8 Rn
                self.set_reg(n, self.get_reg(n) >> 8);
                1
            }
            0x1A => { // LDS Rm,MACL
                self.macl = self.get_reg(n);
                1
            }
            0x1B => { // TAS.B @Rn
                let addr = self.get_reg(n);
                let val = self.bus.read8(addr);
                self.sr.t = val == 0;
                self.bus.write8(addr, val | 0x80);
                4
            }
            0x1E => { // LDC Rm,GBR
                self.gbr = self.get_reg(n);
                1
            }
            0x20 => { // SHAL Rn
                self.sr.t = (self.get_reg(n) >> 31) != 0;
                self.set_reg(n, self.get_reg(n) << 1);
                1
            }
            0x21 => { // SHAR Rn
                self.sr.t = (self.get_reg(n) & 1) != 0;
                self.set_reg(n, ((self.get_reg(n) as i32) >> 1) as u32);
                1
            }
            0x22 => { // STS.L PR,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.pr);
                1
            }
            0x23 => { // STC.L VBR,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.vbr);
                1
            }
            0x24 => { // ROTCL Rn
                let old_t = self.sr.t;
                self.sr.t = (self.get_reg(n) >> 31) != 0;
                let result = (self.get_reg(n) << 1) | (if old_t { 1 } else { 0 });
                self.set_reg(n, result);
                1
            }
            0x25 => { // ROTCR Rn
                let old_t = self.sr.t;
                self.sr.t = (self.get_reg(n) & 1) != 0;
                let result = (self.get_reg(n) >> 1) | (if old_t { 0x80000000 } else { 0 });
                self.set_reg(n, result);
                1
            }
            0x26 => { // LDS.L @Rm+,PR
                self.pr = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x27 => { // LDC.L @Rm+,VBR
                self.vbr = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x28 => { // SHLL16 Rn
                self.set_reg(n, self.get_reg(n) << 16);
                1
            }
            0x29 => { // SHLR16 Rn
                self.set_reg(n, self.get_reg(n) >> 16);
                1
            }
            0x2A => { // LDS Rm,PR
                self.pr = self.get_reg(n);
                1
            }
            0x2B => { // JMP @Rn
                self.delay_branch(self.get_reg(n));
                2
            }
            0x2E => { // LDC Rm,VBR
                self.vbr = self.get_reg(n);
                1
            }
            0x33 => { // STC.L SSR,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.ssr);
                1
            }
            0x37 => { // LDC.L @Rm+,SSR
                self.ssr = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x3E => { // LDC Rm,SSR
                self.ssr = self.get_reg(n);
                1
            }
            0x43 => { // STC.L SPC,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.spc);
                1
            }
            0x47 => { // LDC.L @Rm+,SPC
                self.spc = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x4E => { // LDC Rm,SPC
                self.spc = self.get_reg(n);
                1
            }
            0x52 => { // STS.L FPUL,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.fpul);
                1
            }
            0x56 => { // LDS.L @Rm+,FPUL
                self.fpul = self.bus.read32(self.get_reg(n));
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x5A => { // LDS Rm,FPUL
                self.fpul = self.get_reg(n);
                1
            }
            0x62 => { // STS.L FPSCR,@-Rn
                let addr = self.get_reg(n).wrapping_sub(4);
                self.set_reg(n, addr);
                self.bus.write32(addr, self.fpscr.to_u32());
                1
            }
            0x66 => { // LDS.L @Rm+,FPSCR
                let val = self.bus.read32(self.get_reg(n));
                self.fpscr = Fpscr::from_u32(val);
                self.set_reg(n, self.get_reg(n).wrapping_add(4));
                1
            }
            0x6A => { // LDS Rm,FPSCR
                self.fpscr = Fpscr::from_u32(self.get_reg(n));
                1
            }
            0xFA => { // LDC Rm,DBR
                self.dbr = self.get_reg(n);
                1
            }
            _ => {
                // Handle STC.L Rm_BANK,@-Rn (0x8n where n=3)
                if (op & 0x8F) == 0x83 {
                    let bank = (op >> 4) & 0x7;
                    let addr = self.get_reg(n).wrapping_sub(4);
                    self.set_reg(n, addr);
                    self.bus.write32(addr, self.r_bank[bank as usize]);
                    return 1;
                }
                // Handle LDC.L @Rm+,Rn_BANK (0x8n where n=7)
                if (op & 0x8F) == 0x87 {
                    let bank = (op >> 4) & 0x7;
                    self.r_bank[bank as usize] = self.bus.read32(self.get_reg(n));
                    self.set_reg(n, self.get_reg(n).wrapping_add(4));
                    return 1;
                }
                // Handle LDC Rm,Rn_BANK (0x8n where n=E)
                if (op & 0x8F) == 0x8E {
                    let bank = (op >> 4) & 0x7;
                    self.r_bank[bank as usize] = self.get_reg(n);
                    return 1;
                }
                // SHAD/SHLD
                if (op & 0x0F) == 0x0C {
                    // SHAD Rm,Rn
                    let shift = self.get_reg(m) as i32;
                    let result = if shift >= 0 {
                        (self.get_reg(n) as i32) << (shift & 0x1F)
                    } else if (shift & 0x1F) == 0 {
                        if (self.get_reg(n) as i32) < 0 { -1 } else { 0 }
                    } else {
                        (self.get_reg(n) as i32) >> ((-shift) & 0x1F)
                    };
                    self.set_reg(n, result as u32);
                    return 1;
                }
                if (op & 0x0F) == 0x0D {
                    // SHLD Rm,Rn
                    let shift = self.get_reg(m) as i32;
                    let result = if shift >= 0 {
                        self.get_reg(n) << (shift & 0x1F)
                    } else if (shift & 0x1F) == 0 {
                        0
                    } else {
                        self.get_reg(n) >> ((-shift) & 0x1F)
                    };
                    self.set_reg(n, result);
                    return 1;
                }
                // MAC.W @Rm+,@Rn+
                if (op & 0x0F) == 0x0F {
                    let val1 = self.bus.read16(self.get_reg(n)) as i16 as i32;
                    let val2 = self.bus.read16(self.get_reg(m)) as i16 as i32;
                    self.set_reg(n, self.get_reg(n).wrapping_add(2));
                    self.set_reg(m, self.get_reg(m).wrapping_add(2));
                    let result = val1 * val2;
                    if self.sr.s {
                        // Saturation
                        let mac = self.macl as i32;
                        let (new_mac, overflow) = mac.overflowing_add(result);
                        if overflow {
                            self.mach = 1;
                            self.macl = if result < 0 { 0x80000000 } else { 0x7FFFFFFF };
                        } else {
                            self.macl = new_mac as u32;
                        }
                    } else {
                        let mac = ((self.mach as i64) << 32) | (self.macl as u64 as i64);
                        let new_mac = mac + result as i64;
                        self.mach = (new_mac >> 32) as u32;
                        self.macl = new_mac as u32;
                    }
                    return 3;
                }
                1
            }
        }
    }

    fn execute_5xxx(&mut self, instr: u16) -> u32 {
        // MOV.L @(disp,Rm),Rn
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let disp = (instr & 0xF) as u32 * 4;
        let addr = self.get_reg(m).wrapping_add(disp);
        let val = self.bus.read32(addr);
        self.set_reg(n, val);
        1
    }

    fn execute_6xxx(&mut self, instr: u16) -> u32 {
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let op = instr & 0xF;

        match op {
            0x0 => { // MOV.B @Rm,Rn
                let val = self.bus.read8(self.get_reg(m));
                self.set_reg(n, Self::sign_extend8(val));
                1
            }
            0x1 => { // MOV.W @Rm,Rn
                let val = self.bus.read16(self.get_reg(m));
                self.set_reg(n, Self::sign_extend16(val));
                1
            }
            0x2 => { // MOV.L @Rm,Rn
                let addr = self.get_reg(m);
                let val = self.bus.read32(addr);
                self.set_reg(n, val);
                1
            }
            0x3 => { // MOV Rm,Rn
                self.set_reg(n, self.get_reg(m));
                1
            }
            0x4 => { // MOV.B @Rm+,Rn
                let val = self.bus.read8(self.get_reg(m));
                self.set_reg(n, Self::sign_extend8(val));
                if n != m {
                    self.set_reg(m, self.get_reg(m).wrapping_add(1));
                }
                1
            }
            0x5 => { // MOV.W @Rm+,Rn
                let val = self.bus.read16(self.get_reg(m));
                self.set_reg(n, Self::sign_extend16(val));
                if n != m {
                    self.set_reg(m, self.get_reg(m).wrapping_add(2));
                }
                1
            }
            0x6 => { // MOV.L @Rm+,Rn
                let addr = self.get_reg(m);
                let val = self.bus.read32(addr);
                self.set_reg(n, val);
                if n != m {
                    self.set_reg(m, self.get_reg(m).wrapping_add(4));
                }
                1
            }
            0x7 => { // NOT Rm,Rn
                self.set_reg(n, !self.get_reg(m));
                1
            }
            0x8 => { // SWAP.B Rm,Rn
                let rm = self.get_reg(m);
                let result = (rm & 0xFFFF0000) | ((rm & 0xFF) << 8) | ((rm >> 8) & 0xFF);
                self.set_reg(n, result);
                1
            }
            0x9 => { // SWAP.W Rm,Rn
                let rm = self.get_reg(m);
                let result = (rm << 16) | (rm >> 16);
                self.set_reg(n, result);
                1
            }
            0xA => { // NEGC Rm,Rn
                let t = if self.sr.t { 1u32 } else { 0 };
                let result = 0u32.wrapping_sub(self.get_reg(m)).wrapping_sub(t);
                self.sr.t = self.get_reg(m) != 0 || t != 0;
                self.set_reg(n, result);
                1
            }
            0xB => { // NEG Rm,Rn
                self.set_reg(n, 0u32.wrapping_sub(self.get_reg(m)));
                1
            }
            0xC => { // EXTU.B Rm,Rn
                self.set_reg(n, self.get_reg(m) & 0xFF);
                1
            }
            0xD => { // EXTU.W Rm,Rn
                self.set_reg(n, self.get_reg(m) & 0xFFFF);
                1
            }
            0xE => { // EXTS.B Rm,Rn
                self.set_reg(n, Self::sign_extend8(self.get_reg(m) as u8));
                1
            }
            0xF => { // EXTS.W Rm,Rn
                self.set_reg(n, Self::sign_extend16(self.get_reg(m) as u16));
                1
            }
            _ => 1,
        }
    }

    fn execute_7xxx(&mut self, instr: u16) -> u32 {
        // ADD #imm,Rn
        let n = ((instr >> 8) & 0xF) as usize;
        let imm = (instr & 0xFF) as i8 as i32 as u32;
        self.set_reg(n, self.get_reg(n).wrapping_add(imm));
        1
    }

    fn execute_8xxx(&mut self, instr: u16) -> u32 {
        let op = (instr >> 8) & 0xF;
        let m_or_n = ((instr >> 4) & 0xF) as usize;
        let disp = instr & 0xF;

        match op {
            0x0 => { // MOV.B R0,@(disp,Rn)
                let addr = self.get_reg(m_or_n).wrapping_add(disp as u32);
                self.bus.write8(addr, self.r[0] as u8);
                1
            }
            0x1 => { // MOV.W R0,@(disp,Rn)
                let addr = self.get_reg(m_or_n).wrapping_add((disp as u32) * 2);
                self.bus.write16(addr, self.r[0] as u16);
                1
            }
            0x4 => { // MOV.B @(disp,Rm),R0
                let addr = self.get_reg(m_or_n).wrapping_add(disp as u32);
                let val = self.bus.read8(addr);
                self.r[0] = Self::sign_extend8(val);
                1
            }
            0x5 => { // MOV.W @(disp,Rm),R0
                let addr = self.get_reg(m_or_n).wrapping_add((disp as u32) * 2);
                let val = self.bus.read16(addr);
                self.r[0] = Self::sign_extend16(val);
                1
            }
            0x8 => { // CMP/EQ #imm,R0
                let imm = (instr & 0xFF) as i8 as i32 as u32;
                self.sr.t = self.r[0] == imm;
                1
            }
            0x9 => { // BT label
                if self.sr.t {
                    let disp = Self::sign_extend8((instr & 0xFF) as u8);
                    self.pc = self.pc.wrapping_add(disp.wrapping_mul(2)).wrapping_add(2);
                }
                1
            }
            0xB => { // BF label
                if !self.sr.t {
                    let disp = Self::sign_extend8((instr & 0xFF) as u8);
                    self.pc = self.pc.wrapping_add(disp.wrapping_mul(2)).wrapping_add(2);
                }
                1
            }
            0xD => { // BT/S label
                if self.sr.t {
                    let disp = Self::sign_extend8((instr & 0xFF) as u8);
                    let target = self.pc.wrapping_add(disp.wrapping_mul(2)).wrapping_add(2);
                    self.delay_branch(target);
                }
                1
            }
            0xF => { // BF/S label
                if !self.sr.t {
                    let disp = Self::sign_extend8((instr & 0xFF) as u8);
                    let target = self.pc.wrapping_add(disp.wrapping_mul(2)).wrapping_add(2);
                    self.delay_branch(target);
                }
                1
            }
            _ => 1,
        }
    }

    fn execute_9xxx(&mut self, instr: u16) -> u32 {
        // MOV.W @(disp,PC),Rn
        let n = ((instr >> 8) & 0xF) as usize;
        let disp = (instr & 0xFF) as u32 * 2;
        let addr = (self.pc & !1).wrapping_add(disp);
        let val = self.bus.read16(addr);
        self.set_reg(n, Self::sign_extend16(val));
        1
    }

    fn execute_axxx(&mut self, instr: u16) -> u32 {
        // BRA label
        let disp = Self::sign_extend12(instr & 0xFFF);
        let target = self.pc.wrapping_add(disp.wrapping_mul(2)).wrapping_add(2);
        self.delay_branch(target);
        1
    }

    fn execute_bxxx(&mut self, instr: u16) -> u32 {
        // BSR label
        let disp = Self::sign_extend12(instr & 0xFFF);
        self.pr = self.pc.wrapping_add(2);
        let target = self.pc.wrapping_add(disp.wrapping_mul(2)).wrapping_add(2);
        self.delay_branch(target);
        1
    }

    fn execute_cxxx(&mut self, instr: u16) -> u32 {
        let op = (instr >> 8) & 0xF;
        let imm = (instr & 0xFF) as u32;

        match op {
            0x0 => { // MOV.B R0,@(disp,GBR)
                let addr = self.gbr.wrapping_add(imm);
                self.bus.write8(addr, self.r[0] as u8);
                1
            }
            0x1 => { // MOV.W R0,@(disp,GBR)
                let addr = self.gbr.wrapping_add(imm * 2);
                self.bus.write16(addr, self.r[0] as u16);
                1
            }
            0x2 => { // MOV.L R0,@(disp,GBR)
                let addr = self.gbr.wrapping_add(imm * 4);
                self.bus.write32(addr, self.r[0]);
                1
            }
            0x3 => { // TRAPA #imm
                self.ssr = self.sr.to_u32();
                self.spc = self.pc;
                self.sr.bl = true;
                self.sr.md = true;
                self.sr.rb = true;
                self.pc = self.vbr.wrapping_add(0x100);
                7
            }
            0x4 => { // MOV.B @(disp,GBR),R0
                let addr = self.gbr.wrapping_add(imm);
                let val = self.bus.read8(addr);
                self.r[0] = Self::sign_extend8(val);
                1
            }
            0x5 => { // MOV.W @(disp,GBR),R0
                let addr = self.gbr.wrapping_add(imm * 2);
                let val = self.bus.read16(addr);
                self.r[0] = Self::sign_extend16(val);
                1
            }
            0x6 => { // MOV.L @(disp,GBR),R0
                let addr = self.gbr.wrapping_add(imm * 4);
                self.r[0] = self.bus.read32(addr);
                1
            }
            0x7 => { // MOVA @(disp,PC),R0
                let disp = (instr & 0xFF) as u32 * 4;
                self.r[0] = (self.pc & !3).wrapping_add(disp);
                1
            }
            0x8 => { // TST #imm,R0
                self.sr.t = (self.r[0] & imm) == 0;
                1
            }
            0x9 => { // AND #imm,R0
                self.r[0] &= imm;
                1
            }
            0xA => { // XOR #imm,R0
                self.r[0] ^= imm;
                1
            }
            0xB => { // OR #imm,R0
                self.r[0] |= imm;
                1
            }
            0xC => { // TST.B #imm,@(R0,GBR)
                let addr = self.r[0].wrapping_add(self.gbr);
                let val = self.bus.read8(addr);
                self.sr.t = (val as u32 & imm) == 0;
                3
            }
            0xD => { // AND.B #imm,@(R0,GBR)
                let addr = self.r[0].wrapping_add(self.gbr);
                let val = self.bus.read8(addr);
                self.bus.write8(addr, val & imm as u8);
                3
            }
            0xE => { // XOR.B #imm,@(R0,GBR)
                let addr = self.r[0].wrapping_add(self.gbr);
                let val = self.bus.read8(addr);
                self.bus.write8(addr, val ^ imm as u8);
                3
            }
            0xF => { // OR.B #imm,@(R0,GBR)
                let addr = self.r[0].wrapping_add(self.gbr);
                let val = self.bus.read8(addr);
                self.bus.write8(addr, val | imm as u8);
                3
            }
            _ => 1,
        }
    }

    fn execute_dxxx(&mut self, instr: u16) -> u32 {
        // MOV.L @(disp,PC),Rn
        let n = ((instr >> 8) & 0xF) as usize;
        let disp = (instr & 0xFF) as u32 * 4;
        let addr = (self.pc & !3).wrapping_add(disp);
        let val = self.bus.read32(addr);
        self.set_reg(n, val);
        1
    }

    fn execute_exxx(&mut self, instr: u16) -> u32 {
        // MOV #imm,Rn
        let n = ((instr >> 8) & 0xF) as usize;
        let imm = (instr & 0xFF) as i8 as i32 as u32;
        self.set_reg(n, imm);
        1
    }

    fn execute_fxxx(&mut self, instr: u16) -> u32 {
        let n = ((instr >> 8) & 0xF) as usize;
        let m = ((instr >> 4) & 0xF) as usize;
        let op = instr & 0xF;

        match op {
            0x0 => { // FADD FRm,FRn
                if self.fpscr.pr {
                    let dn = n >> 1;
                    let dm = m >> 1;
                    let result = self.get_dr(dn) + self.get_dr(dm);
                    self.set_dr(dn, result);
                } else {
                    let result = self.get_fr(n) + self.get_fr(m);
                    self.set_fr(n, result);
                }
                1
            }
            0x1 => { // FSUB FRm,FRn
                if self.fpscr.pr {
                    let dn = n >> 1;
                    let dm = m >> 1;
                    let result = self.get_dr(dn) - self.get_dr(dm);
                    self.set_dr(dn, result);
                } else {
                    let result = self.get_fr(n) - self.get_fr(m);
                    self.set_fr(n, result);
                }
                1
            }
            0x2 => { // FMUL FRm,FRn
                if self.fpscr.pr {
                    let dn = n >> 1;
                    let dm = m >> 1;
                    let result = self.get_dr(dn) * self.get_dr(dm);
                    self.set_dr(dn, result);
                } else {
                    let result = self.get_fr(n) * self.get_fr(m);
                    self.set_fr(n, result);
                }
                1
            }
            0x3 => { // FDIV FRm,FRn
                if self.fpscr.pr {
                    let dn = n >> 1;
                    let dm = m >> 1;
                    let result = self.get_dr(dn) / self.get_dr(dm);
                    self.set_dr(dn, result);
                } else {
                    let result = self.get_fr(n) / self.get_fr(m);
                    self.set_fr(n, result);
                }
                10
            }
            0x4 => { // FCMP/EQ FRm,FRn
                if self.fpscr.pr {
                    let dn = n >> 1;
                    let dm = m >> 1;
                    self.sr.t = self.get_dr(dn) == self.get_dr(dm);
                } else {
                    self.sr.t = self.get_fr(n) == self.get_fr(m);
                }
                1
            }
            0x5 => { // FCMP/GT FRm,FRn
                if self.fpscr.pr {
                    let dn = n >> 1;
                    let dm = m >> 1;
                    self.sr.t = self.get_dr(dn) > self.get_dr(dm);
                } else {
                    self.sr.t = self.get_fr(n) > self.get_fr(m);
                }
                1
            }
            0x6 => { // FMOV.S @(R0,Rm),FRn / FMOV @(R0,Rm),DRn
                let addr = self.r[0].wrapping_add(self.get_reg(m));
                if self.fpscr.sz {
                    let val = self.bus.read64(addr);
                    let dn = n >> 1;
                    self.set_dr(dn, f64::from_bits(val));
                } else {
                    let val = self.bus.read32(addr);
                    self.set_fr(n, f32::from_bits(val));
                }
                1
            }
            0x7 => { // FMOV.S FRm,@(R0,Rn) / FMOV DRm,@(R0,Rn)
                let addr = self.r[0].wrapping_add(self.get_reg(n));
                if self.fpscr.sz {
                    let dm = m >> 1;
                    self.bus.write64(addr, self.get_dr(dm).to_bits());
                } else {
                    self.bus.write32(addr, self.get_fr(m).to_bits());
                }
                1
            }
            0x8 => { // FMOV.S @Rm,FRn / FMOV @Rm,DRn
                let addr = self.get_reg(m);
                if self.fpscr.sz {
                    let val = self.bus.read64(addr);
                    let dn = n >> 1;
                    self.set_dr(dn, f64::from_bits(val));
                } else {
                    let val = self.bus.read32(addr);
                    self.set_fr(n, f32::from_bits(val));
                }
                1
            }
            0x9 => { // FMOV.S @Rm+,FRn / FMOV @Rm+,DRn
                let addr = self.get_reg(m);
                if self.fpscr.sz {
                    let val = self.bus.read64(addr);
                    let dn = n >> 1;
                    self.set_dr(dn, f64::from_bits(val));
                    self.set_reg(m, addr.wrapping_add(8));
                } else {
                    let val = self.bus.read32(addr);
                    self.set_fr(n, f32::from_bits(val));
                    self.set_reg(m, addr.wrapping_add(4));
                }
                1
            }
            0xA => { // FMOV.S FRm,@Rn / FMOV DRm,@Rn
                let addr = self.get_reg(n);
                if self.fpscr.sz {
                    let dm = m >> 1;
                    self.bus.write64(addr, self.get_dr(dm).to_bits());
                } else {
                    self.bus.write32(addr, self.get_fr(m).to_bits());
                }
                1
            }
            0xB => { // FMOV.S FRm,@-Rn / FMOV DRm,@-Rn
                if self.fpscr.sz {
                    let addr = self.get_reg(n).wrapping_sub(8);
                    self.set_reg(n, addr);
                    let dm = m >> 1;
                    self.bus.write64(addr, self.get_dr(dm).to_bits());
                } else {
                    let addr = self.get_reg(n).wrapping_sub(4);
                    self.set_reg(n, addr);
                    self.bus.write32(addr, self.get_fr(m).to_bits());
                }
                1
            }
            0xC => { // FMOV FRm,FRn / FMOV DRm,DRn
                if self.fpscr.sz {
                    let dn = n >> 1;
                    let dm = m >> 1;
                    let val = self.get_dr(dm);
                    self.set_dr(dn, val);
                } else {
                    let val = self.get_fr(m);
                    self.set_fr(n, val);
                }
                1
            }
            0xD => {
                match (instr >> 4) & 0xFF {
                    0x0 => { // FSTS FPUL,FRn
                        self.set_fr(n, f32::from_bits(self.fpul));
                        1
                    }
                    0x1 => { // FLDS FRm,FPUL
                        self.fpul = self.get_fr(n).to_bits();
                        1
                    }
                    0x2 => { // FLOAT FPUL,FRn / FLOAT FPUL,DRn
                        if self.fpscr.pr {
                            let dn = n >> 1;
                            self.set_dr(dn, self.fpul as i32 as f64);
                        } else {
                            self.set_fr(n, self.fpul as i32 as f32);
                        }
                        1
                    }
                    0x3 => { // FTRC FRm,FPUL / FTRC DRm,FPUL
                        if self.fpscr.pr {
                            let dm = n >> 1;
                            self.fpul = self.get_dr(dm) as i32 as u32;
                        } else {
                            self.fpul = self.get_fr(n) as i32 as u32;
                        }
                        1
                    }
                    0x4 => { // FNEG FRn / FNEG DRn
                        if self.fpscr.pr {
                            let dn = n >> 1;
                            self.set_dr(dn, -self.get_dr(dn));
                        } else {
                            self.set_fr(n, -self.get_fr(n));
                        }
                        1
                    }
                    0x5 => { // FABS FRn / FABS DRn
                        if self.fpscr.pr {
                            let dn = n >> 1;
                            self.set_dr(dn, self.get_dr(dn).abs());
                        } else {
                            self.set_fr(n, self.get_fr(n).abs());
                        }
                        1
                    }
                    0x6 => { // FSQRT FRn / FSQRT DRn
                        if self.fpscr.pr {
                            let dn = n >> 1;
                            self.set_dr(dn, self.get_dr(dn).sqrt());
                        } else {
                            self.set_fr(n, self.get_fr(n).sqrt());
                        }
                        9
                    }
                    0x8 => { // FLDI0 FRn
                        self.set_fr(n, 0.0);
                        1
                    }
                    0x9 => { // FLDI1 FRn
                        self.set_fr(n, 1.0);
                        1
                    }
                    0xA => { // FCNVSD FPUL,DRn
                        let dn = n >> 1;
                        self.set_dr(dn, f32::from_bits(self.fpul) as f64);
                        1
                    }
                    0xB => { // FCNVDS DRm,FPUL
                        let dm = n >> 1;
                        self.fpul = (self.get_dr(dm) as f32).to_bits();
                        1
                    }
                    0xE => { // FIPR FVm,FVn
                        let vn = (n & 0xC) as usize;
                        let vm = ((n & 0x3) << 2) as usize;
                        let result = self.get_fr(vn) * self.get_fr(vm)
                            + self.get_fr(vn + 1) * self.get_fr(vm + 1)
                            + self.get_fr(vn + 2) * self.get_fr(vm + 2)
                            + self.get_fr(vn + 3) * self.get_fr(vm + 3);
                        self.set_fr(vn + 3, result);
                        1
                    }
                    0xF => {
                        if (n & 0x1) == 0 {
                            // FTRV XMTRX,FVn
                            let vn = (n & 0xC) as usize;
                            let fr0 = self.get_fr(vn);
                            let fr1 = self.get_fr(vn + 1);
                            let fr2 = self.get_fr(vn + 2);
                            let fr3 = self.get_fr(vn + 3);

                            // Get XF matrix (4x4)
                            let xf = |i: usize| -> f32 {
                                if self.fpscr.fr { self.fr[i] } else { self.xf[i] }
                            };

                            let r0 = xf(0) * fr0 + xf(4) * fr1 + xf(8) * fr2 + xf(12) * fr3;
                            let r1 = xf(1) * fr0 + xf(5) * fr1 + xf(9) * fr2 + xf(13) * fr3;
                            let r2 = xf(2) * fr0 + xf(6) * fr1 + xf(10) * fr2 + xf(14) * fr3;
                            let r3 = xf(3) * fr0 + xf(7) * fr1 + xf(11) * fr2 + xf(15) * fr3;

                            self.set_fr(vn, r0);
                            self.set_fr(vn + 1, r1);
                            self.set_fr(vn + 2, r2);
                            self.set_fr(vn + 3, r3);
                            4
                        } else {
                            // FSCHG / FRCHG
                            if n == 0xF {
                                self.fpscr.fr = !self.fpscr.fr;
                            } else {
                                self.fpscr.sz = !self.fpscr.sz;
                            }
                            1
                        }
                    }
                    _ => 1,
                }
            }
            0xE => { // FMAC FR0,FRm,FRn
                let result = self.get_fr(0) * self.get_fr(m) + self.get_fr(n);
                self.set_fr(n, result);
                1
            }
            _ => 1,
        }
    }
}

impl<B: BusSh4> Cpu for Sh4<B> {
    fn step(&mut self) -> u32 {
        let cycles = self.execute();
        self.cycles += cycles as u64;
        cycles
    }

    fn reset(&mut self) {
        self.r = [0; 16];
        self.r_bank = [0; 8];
        self.sr = StatusRegister {
            md: true,
            rb: true,
            bl: true,
            imask: 0xF,
            ..Default::default()
        };
        self.vbr = 0;
        self.pc = 0xA0000000;
        self.fpscr = Fpscr::default();
    }

    fn irq(&mut self) {
        self.irq_pending = true;
    }

    fn nmi(&mut self) {
        // Handle NMI
        self.ssr = self.sr.to_u32();
        self.spc = self.pc;
        self.sr.bl = true;
        self.sr.md = true;
        self.sr.rb = true;
        self.pc = self.vbr.wrapping_add(0x600);
    }

    fn pc(&self) -> u16 {
        self.pc as u16
    }

    fn set_pc(&mut self, pc: u16) {
        self.pc = pc as u32;
    }

    fn cycles(&self) -> u64 {
        self.cycles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus {
        ram: Vec<u8>,
    }

    impl TestBus {
        fn new() -> Self {
            Self { ram: vec![0; 0x1000000] }
        }
    }

    impl BusSh4 for TestBus {
        fn read8(&mut self, addr: u32) -> u8 {
            self.ram.get(addr as usize).copied().unwrap_or(0)
        }
        fn read16(&mut self, addr: u32) -> u16 {
            let lo = self.read8(addr) as u16;
            let hi = self.read8(addr + 1) as u16;
            lo | (hi << 8)
        }
        fn read32(&mut self, addr: u32) -> u32 {
            let lo = self.read16(addr) as u32;
            let hi = self.read16(addr + 2) as u32;
            lo | (hi << 16)
        }
        fn read64(&mut self, addr: u32) -> u64 {
            let lo = self.read32(addr) as u64;
            let hi = self.read32(addr + 4) as u64;
            lo | (hi << 32)
        }
        fn write8(&mut self, addr: u32, val: u8) {
            if let Some(byte) = self.ram.get_mut(addr as usize) {
                *byte = val;
            }
        }
        fn write16(&mut self, addr: u32, val: u16) {
            self.write8(addr, val as u8);
            self.write8(addr + 1, (val >> 8) as u8);
        }
        fn write32(&mut self, addr: u32, val: u32) {
            self.write16(addr, val as u16);
            self.write16(addr + 2, (val >> 16) as u16);
        }
        fn write64(&mut self, addr: u32, val: u64) {
            self.write32(addr, val as u32);
            self.write32(addr + 4, (val >> 32) as u32);
        }
    }

    #[test]
    fn test_mov_immediate() {
        let mut bus = TestBus::new();
        // MOV #42,R0 (0xE042)
        bus.write16(0, 0xE042);

        let mut cpu = Sh4::new(bus);
        cpu.pc = 0;
        cpu.step();

        assert_eq!(cpu.r[0], 0x42);
    }

    #[test]
    fn test_add() {
        let mut bus = TestBus::new();
        // MOV #10,R0
        bus.write16(0, 0xE00A);
        // MOV #20,R1
        bus.write16(2, 0xE114);
        // ADD R1,R0
        bus.write16(4, 0x301C);

        let mut cpu = Sh4::new(bus);
        cpu.pc = 0;
        cpu.step();
        cpu.step();
        cpu.step();

        assert_eq!(cpu.r[0], 30);
    }
}
