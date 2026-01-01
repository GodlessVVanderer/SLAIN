//! Zilog Z80 CPU Emulator
//!
//! The Z80 is an 8-bit microprocessor used in the Sega Master System.
//! This is a cycle-accurate implementation.

use super::Cpu;

/// Memory access trait for the Z80
pub trait BusZ80 {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, data: u8);
    fn io_read(&mut self, port: u16) -> u8;
    fn io_write(&mut self, port: u16, data: u8);
}

/// Z80 CPU flags
#[derive(Debug, Clone, Copy, Default)]
pub struct Z80Flags {
    pub s: bool,  // Sign
    pub z: bool,  // Zero
    pub h: bool,  // Half carry
    pub pv: bool, // Parity/Overflow
    pub n: bool,  // Add/Subtract
    pub c: bool,  // Carry
}

impl Z80Flags {
    pub fn to_byte(&self) -> u8 {
        let mut f = 0u8;
        if self.s { f |= 0x80; }
        if self.z { f |= 0x40; }
        if self.h { f |= 0x10; }
        if self.pv { f |= 0x04; }
        if self.n { f |= 0x02; }
        if self.c { f |= 0x01; }
        f
    }

    pub fn from_byte(b: u8) -> Self {
        Self {
            s: b & 0x80 != 0,
            z: b & 0x40 != 0,
            h: b & 0x10 != 0,
            pv: b & 0x04 != 0,
            n: b & 0x02 != 0,
            c: b & 0x01 != 0,
        }
    }
}

/// Z80 CPU state
pub struct Z80<B: BusZ80> {
    // Main registers
    pub a: u8,
    pub f: Z80Flags,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,

    // Alternate registers
    pub a_alt: u8,
    pub f_alt: Z80Flags,
    pub b_alt: u8,
    pub c_alt: u8,
    pub d_alt: u8,
    pub e_alt: u8,
    pub h_alt: u8,
    pub l_alt: u8,

    // Index registers
    pub ix: u16,
    pub iy: u16,

    // Stack pointer and program counter
    pub sp: u16,
    pub pc: u16,

    // Interrupt registers
    pub i: u8,
    pub r: u8,

    // Interrupt flip-flops
    pub iff1: bool,
    pub iff2: bool,

    // Interrupt mode (0, 1, or 2)
    pub im: u8,

    // Halted state
    pub halted: bool,

    // Bus
    pub bus: B,

    // Cycle counter
    pub cycles: u64,

    // Pending interrupt
    irq_pending: bool,
    nmi_pending: bool,
}

impl<B: BusZ80> Z80<B> {
    pub fn new(bus: B) -> Self {
        Self {
            a: 0xFF,
            f: Z80Flags::default(),
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            h: 0,
            l: 0,
            a_alt: 0,
            f_alt: Z80Flags::default(),
            b_alt: 0,
            c_alt: 0,
            d_alt: 0,
            e_alt: 0,
            h_alt: 0,
            l_alt: 0,
            ix: 0,
            iy: 0,
            sp: 0xFFFF,
            pc: 0,
            i: 0,
            r: 0,
            iff1: false,
            iff2: false,
            im: 0,
            halted: false,
            bus,
            cycles: 0,
            irq_pending: false,
            nmi_pending: false,
        }
    }

    // Register pair helpers
    fn af(&self) -> u16 { ((self.a as u16) << 8) | (self.f.to_byte() as u16) }
    fn bc(&self) -> u16 { ((self.b as u16) << 8) | (self.c as u16) }
    fn de(&self) -> u16 { ((self.d as u16) << 8) | (self.e as u16) }
    fn hl(&self) -> u16 { ((self.h as u16) << 8) | (self.l as u16) }

    fn set_af(&mut self, val: u16) { self.a = (val >> 8) as u8; self.f = Z80Flags::from_byte(val as u8); }
    fn set_bc(&mut self, val: u16) { self.b = (val >> 8) as u8; self.c = val as u8; }
    fn set_de(&mut self, val: u16) { self.d = (val >> 8) as u8; self.e = val as u8; }
    fn set_hl(&mut self, val: u16) { self.h = (val >> 8) as u8; self.l = val as u8; }

    fn read16(&mut self, addr: u16) -> u16 {
        let lo = self.bus.read(addr) as u16;
        let hi = self.bus.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    fn write16(&mut self, addr: u16, val: u16) {
        self.bus.write(addr, val as u8);
        self.bus.write(addr.wrapping_add(1), (val >> 8) as u8);
    }

    fn push(&mut self, val: u16) {
        self.sp = self.sp.wrapping_sub(2);
        self.write16(self.sp, val);
    }

    fn pop(&mut self) -> u16 {
        let val = self.read16(self.sp);
        self.sp = self.sp.wrapping_add(2);
        val
    }

    fn fetch(&mut self) -> u8 {
        let val = self.bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        self.r = (self.r & 0x80) | ((self.r.wrapping_add(1)) & 0x7F);
        val
    }

    fn fetch16(&mut self) -> u16 {
        let lo = self.fetch() as u16;
        let hi = self.fetch() as u16;
        (hi << 8) | lo
    }

    // Flag helpers
    fn set_sz(&mut self, val: u8) {
        self.f.s = val & 0x80 != 0;
        self.f.z = val == 0;
    }

    fn parity(val: u8) -> bool {
        val.count_ones() % 2 == 0
    }

    // ALU operations
    fn add_a(&mut self, val: u8) {
        let a = self.a as u16;
        let v = val as u16;
        let result = a + v;

        self.f.s = (result as u8) & 0x80 != 0;
        self.f.z = (result as u8) == 0;
        self.f.h = ((a & 0x0F) + (v & 0x0F)) > 0x0F;
        self.f.pv = ((a ^ result) & (v ^ result) & 0x80) != 0;
        self.f.n = false;
        self.f.c = result > 0xFF;
        self.a = result as u8;
    }

    fn adc_a(&mut self, val: u8) {
        let a = self.a as u16;
        let v = val as u16;
        let c = if self.f.c { 1u16 } else { 0 };
        let result = a + v + c;

        self.f.s = (result as u8) & 0x80 != 0;
        self.f.z = (result as u8) == 0;
        self.f.h = ((a & 0x0F) + (v & 0x0F) + c) > 0x0F;
        self.f.pv = ((a ^ result) & (v ^ result) & 0x80) != 0;
        self.f.n = false;
        self.f.c = result > 0xFF;
        self.a = result as u8;
    }

    fn sub_a(&mut self, val: u8) {
        let a = self.a as i16;
        let v = val as i16;
        let result = a - v;

        self.f.s = (result as u8) & 0x80 != 0;
        self.f.z = (result as u8) == 0;
        self.f.h = (a & 0x0F) < (v & 0x0F);
        self.f.pv = ((a ^ v) & (a ^ result) & 0x80) != 0;
        self.f.n = true;
        self.f.c = (result as i16) < 0;
        self.a = result as u8;
    }

    fn sbc_a(&mut self, val: u8) {
        let a = self.a as i16;
        let v = val as i16;
        let c = if self.f.c { 1i16 } else { 0 };
        let result = a - v - c;

        self.f.s = (result as u8) & 0x80 != 0;
        self.f.z = (result as u8) == 0;
        self.f.h = (a & 0x0F) < ((v & 0x0F) + c);
        self.f.pv = ((a ^ v) & (a ^ result) & 0x80) != 0;
        self.f.n = true;
        self.f.c = result < 0;
        self.a = result as u8;
    }

    fn and_a(&mut self, val: u8) {
        self.a &= val;
        self.f.s = self.a & 0x80 != 0;
        self.f.z = self.a == 0;
        self.f.h = true;
        self.f.pv = Self::parity(self.a);
        self.f.n = false;
        self.f.c = false;
    }

    fn xor_a(&mut self, val: u8) {
        self.a ^= val;
        self.f.s = self.a & 0x80 != 0;
        self.f.z = self.a == 0;
        self.f.h = false;
        self.f.pv = Self::parity(self.a);
        self.f.n = false;
        self.f.c = false;
    }

    fn or_a(&mut self, val: u8) {
        self.a |= val;
        self.f.s = self.a & 0x80 != 0;
        self.f.z = self.a == 0;
        self.f.h = false;
        self.f.pv = Self::parity(self.a);
        self.f.n = false;
        self.f.c = false;
    }

    fn cp_a(&mut self, val: u8) {
        let a = self.a as i16;
        let v = val as i16;
        let result = a - v;

        self.f.s = (result as u8) & 0x80 != 0;
        self.f.z = (result as u8) == 0;
        self.f.h = (a & 0x0F) < (v & 0x0F);
        self.f.pv = ((a ^ v) & (a ^ result) & 0x80) != 0;
        self.f.n = true;
        self.f.c = result < 0;
    }

    fn inc(&mut self, val: u8) -> u8 {
        let result = val.wrapping_add(1);
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = (val & 0x0F) == 0x0F;
        self.f.pv = val == 0x7F;
        self.f.n = false;
        result
    }

    fn dec(&mut self, val: u8) -> u8 {
        let result = val.wrapping_sub(1);
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = (val & 0x0F) == 0x00;
        self.f.pv = val == 0x80;
        self.f.n = true;
        result
    }

    fn add_hl(&mut self, val: u16) {
        let hl = self.hl() as u32;
        let v = val as u32;
        let result = hl + v;

        self.f.h = ((hl & 0x0FFF) + (v & 0x0FFF)) > 0x0FFF;
        self.f.n = false;
        self.f.c = result > 0xFFFF;
        self.set_hl(result as u16);
    }

    fn adc_hl(&mut self, val: u16) {
        let hl = self.hl() as u32;
        let v = val as u32;
        let c = if self.f.c { 1u32 } else { 0 };
        let result = hl + v + c;

        self.f.s = (result as u16) & 0x8000 != 0;
        self.f.z = (result as u16) == 0;
        self.f.h = ((hl & 0x0FFF) + (v & 0x0FFF) + c) > 0x0FFF;
        self.f.pv = ((hl ^ result) & (v ^ result) & 0x8000) != 0;
        self.f.n = false;
        self.f.c = result > 0xFFFF;
        self.set_hl(result as u16);
    }

    fn sbc_hl(&mut self, val: u16) {
        let hl = self.hl() as i32;
        let v = val as i32;
        let c = if self.f.c { 1i32 } else { 0 };
        let result = hl - v - c;

        self.f.s = (result as u16) & 0x8000 != 0;
        self.f.z = (result as u16) == 0;
        self.f.h = (hl & 0x0FFF) < ((v & 0x0FFF) + c);
        self.f.pv = ((hl ^ v) & (hl ^ result) & 0x8000) != 0;
        self.f.n = true;
        self.f.c = result < 0;
        self.set_hl(result as u16);
    }

    // Rotate and shift
    fn rlca(&mut self) {
        let c = (self.a >> 7) & 1;
        self.a = (self.a << 1) | c;
        self.f.h = false;
        self.f.n = false;
        self.f.c = c != 0;
    }

    fn rrca(&mut self) {
        let c = self.a & 1;
        self.a = (self.a >> 1) | (c << 7);
        self.f.h = false;
        self.f.n = false;
        self.f.c = c != 0;
    }

    fn rla(&mut self) {
        let c = self.a >> 7;
        self.a = (self.a << 1) | (if self.f.c { 1 } else { 0 });
        self.f.h = false;
        self.f.n = false;
        self.f.c = c != 0;
    }

    fn rra(&mut self) {
        let c = self.a & 1;
        self.a = (self.a >> 1) | (if self.f.c { 0x80 } else { 0 });
        self.f.h = false;
        self.f.n = false;
        self.f.c = c != 0;
    }

    fn rlc(&mut self, val: u8) -> u8 {
        let c = (val >> 7) & 1;
        let result = (val << 1) | c;
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = false;
        self.f.pv = Self::parity(result);
        self.f.n = false;
        self.f.c = c != 0;
        result
    }

    fn rrc(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let result = (val >> 1) | (c << 7);
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = false;
        self.f.pv = Self::parity(result);
        self.f.n = false;
        self.f.c = c != 0;
        result
    }

    fn rl(&mut self, val: u8) -> u8 {
        let c = val >> 7;
        let result = (val << 1) | (if self.f.c { 1 } else { 0 });
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = false;
        self.f.pv = Self::parity(result);
        self.f.n = false;
        self.f.c = c != 0;
        result
    }

    fn rr(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let result = (val >> 1) | (if self.f.c { 0x80 } else { 0 });
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = false;
        self.f.pv = Self::parity(result);
        self.f.n = false;
        self.f.c = c != 0;
        result
    }

    fn sla(&mut self, val: u8) -> u8 {
        let c = val >> 7;
        let result = val << 1;
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = false;
        self.f.pv = Self::parity(result);
        self.f.n = false;
        self.f.c = c != 0;
        result
    }

    fn sra(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let result = ((val as i8) >> 1) as u8;
        self.f.s = result & 0x80 != 0;
        self.f.z = result == 0;
        self.f.h = false;
        self.f.pv = Self::parity(result);
        self.f.n = false;
        self.f.c = c != 0;
        result
    }

    fn srl(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let result = val >> 1;
        self.f.s = false;
        self.f.z = result == 0;
        self.f.h = false;
        self.f.pv = Self::parity(result);
        self.f.n = false;
        self.f.c = c != 0;
        result
    }

    fn bit(&mut self, bit: u8, val: u8) {
        let result = val & (1 << bit);
        self.f.s = bit == 7 && result != 0;
        self.f.z = result == 0;
        self.f.h = true;
        self.f.pv = result == 0;
        self.f.n = false;
    }

    fn res(&self, bit: u8, val: u8) -> u8 {
        val & !(1 << bit)
    }

    fn set(&self, bit: u8, val: u8) -> u8 {
        val | (1 << bit)
    }

    fn ex_af(&mut self) {
        std::mem::swap(&mut self.a, &mut self.a_alt);
        std::mem::swap(&mut self.f, &mut self.f_alt);
    }

    fn exx(&mut self) {
        std::mem::swap(&mut self.b, &mut self.b_alt);
        std::mem::swap(&mut self.c, &mut self.c_alt);
        std::mem::swap(&mut self.d, &mut self.d_alt);
        std::mem::swap(&mut self.e, &mut self.e_alt);
        std::mem::swap(&mut self.h, &mut self.h_alt);
        std::mem::swap(&mut self.l, &mut self.l_alt);
    }

    fn daa(&mut self) {
        let mut a = self.a;
        let mut correction = 0u8;

        if self.f.h || (!self.f.n && (a & 0x0F) > 9) {
            correction |= 0x06;
        }

        if self.f.c || (!self.f.n && a > 0x99) {
            correction |= 0x60;
            self.f.c = true;
        }

        if self.f.n {
            a = a.wrapping_sub(correction);
        } else {
            a = a.wrapping_add(correction);
        }

        self.f.h = false;
        self.f.s = a & 0x80 != 0;
        self.f.z = a == 0;
        self.f.pv = Self::parity(a);
        self.a = a;
    }

    fn jr(&mut self, condition: bool) -> u32 {
        let offset = self.fetch() as i8;
        if condition {
            self.pc = self.pc.wrapping_add(offset as u16);
            12
        } else {
            7
        }
    }

    fn jp(&mut self, condition: bool) -> u32 {
        let addr = self.fetch16();
        if condition {
            self.pc = addr;
        }
        10
    }

    fn call(&mut self, condition: bool) -> u32 {
        let addr = self.fetch16();
        if condition {
            self.push(self.pc);
            self.pc = addr;
            17
        } else {
            10
        }
    }

    fn ret(&mut self, condition: bool) -> u32 {
        if condition {
            self.pc = self.pop();
            11
        } else {
            5
        }
    }

    fn rst(&mut self, addr: u16) {
        self.push(self.pc);
        self.pc = addr;
    }

    pub fn execute(&mut self) -> u32 {
        // Check for NMI
        if self.nmi_pending {
            self.nmi_pending = false;
            self.halted = false;
            self.iff2 = self.iff1;
            self.iff1 = false;
            self.push(self.pc);
            self.pc = 0x0066;
            return 11;
        }

        // Check for IRQ
        if self.irq_pending && self.iff1 {
            self.irq_pending = false;
            self.halted = false;
            self.iff1 = false;
            self.iff2 = false;

            return match self.im {
                0 | 1 => {
                    self.push(self.pc);
                    self.pc = 0x0038;
                    13
                }
                2 => {
                    self.push(self.pc);
                    let addr = ((self.i as u16) << 8) | 0xFF;
                    self.pc = self.read16(addr);
                    19
                }
                _ => 13,
            };
        }

        if self.halted {
            self.cycles += 4;
            return 4;
        }

        let opcode = self.fetch();
        let cycles = self.execute_opcode(opcode);
        self.cycles += cycles as u64;
        cycles
    }

    fn execute_opcode(&mut self, opcode: u8) -> u32 {
        match opcode {
            // NOP
            0x00 => 4,

            // LD BC,nn
            0x01 => { let val = self.fetch16(); self.set_bc(val); 10 }

            // LD (BC),A
            0x02 => { self.bus.write(self.bc(), self.a); 7 }

            // INC BC
            0x03 => { self.set_bc(self.bc().wrapping_add(1)); 6 }

            // INC B
            0x04 => { self.b = self.inc(self.b); 4 }

            // DEC B
            0x05 => { self.b = self.dec(self.b); 4 }

            // LD B,n
            0x06 => { self.b = self.fetch(); 7 }

            // RLCA
            0x07 => { self.rlca(); 4 }

            // EX AF,AF'
            0x08 => { self.ex_af(); 4 }

            // ADD HL,BC
            0x09 => { self.add_hl(self.bc()); 11 }

            // LD A,(BC)
            0x0A => { self.a = self.bus.read(self.bc()); 7 }

            // DEC BC
            0x0B => { self.set_bc(self.bc().wrapping_sub(1)); 6 }

            // INC C
            0x0C => { self.c = self.inc(self.c); 4 }

            // DEC C
            0x0D => { self.c = self.dec(self.c); 4 }

            // LD C,n
            0x0E => { self.c = self.fetch(); 7 }

            // RRCA
            0x0F => { self.rrca(); 4 }

            // DJNZ
            0x10 => {
                self.b = self.b.wrapping_sub(1);
                if self.b != 0 {
                    let offset = self.fetch() as i8;
                    self.pc = self.pc.wrapping_add(offset as u16);
                    13
                } else {
                    self.pc = self.pc.wrapping_add(1);
                    8
                }
            }

            // LD DE,nn
            0x11 => { let val = self.fetch16(); self.set_de(val); 10 }

            // LD (DE),A
            0x12 => { self.bus.write(self.de(), self.a); 7 }

            // INC DE
            0x13 => { self.set_de(self.de().wrapping_add(1)); 6 }

            // INC D
            0x14 => { self.d = self.inc(self.d); 4 }

            // DEC D
            0x15 => { self.d = self.dec(self.d); 4 }

            // LD D,n
            0x16 => { self.d = self.fetch(); 7 }

            // RLA
            0x17 => { self.rla(); 4 }

            // JR e
            0x18 => self.jr(true),

            // ADD HL,DE
            0x19 => { self.add_hl(self.de()); 11 }

            // LD A,(DE)
            0x1A => { self.a = self.bus.read(self.de()); 7 }

            // DEC DE
            0x1B => { self.set_de(self.de().wrapping_sub(1)); 6 }

            // INC E
            0x1C => { self.e = self.inc(self.e); 4 }

            // DEC E
            0x1D => { self.e = self.dec(self.e); 4 }

            // LD E,n
            0x1E => { self.e = self.fetch(); 7 }

            // RRA
            0x1F => { self.rra(); 4 }

            // JR NZ,e
            0x20 => self.jr(!self.f.z),

            // LD HL,nn
            0x21 => { let val = self.fetch16(); self.set_hl(val); 10 }

            // LD (nn),HL
            0x22 => { let addr = self.fetch16(); self.write16(addr, self.hl()); 16 }

            // INC HL
            0x23 => { self.set_hl(self.hl().wrapping_add(1)); 6 }

            // INC H
            0x24 => { self.h = self.inc(self.h); 4 }

            // DEC H
            0x25 => { self.h = self.dec(self.h); 4 }

            // LD H,n
            0x26 => { self.h = self.fetch(); 7 }

            // DAA
            0x27 => { self.daa(); 4 }

            // JR Z,e
            0x28 => self.jr(self.f.z),

            // ADD HL,HL
            0x29 => { let hl = self.hl(); self.add_hl(hl); 11 }

            // LD HL,(nn)
            0x2A => { let addr = self.fetch16(); let val = self.read16(addr); self.set_hl(val); 16 }

            // DEC HL
            0x2B => { self.set_hl(self.hl().wrapping_sub(1)); 6 }

            // INC L
            0x2C => { self.l = self.inc(self.l); 4 }

            // DEC L
            0x2D => { self.l = self.dec(self.l); 4 }

            // LD L,n
            0x2E => { self.l = self.fetch(); 7 }

            // CPL
            0x2F => { self.a = !self.a; self.f.h = true; self.f.n = true; 4 }

            // JR NC,e
            0x30 => self.jr(!self.f.c),

            // LD SP,nn
            0x31 => { self.sp = self.fetch16(); 10 }

            // LD (nn),A
            0x32 => { let addr = self.fetch16(); self.bus.write(addr, self.a); 13 }

            // INC SP
            0x33 => { self.sp = self.sp.wrapping_add(1); 6 }

            // INC (HL)
            0x34 => { let hl = self.hl(); let v = self.bus.read(hl); let r = self.inc(v); self.bus.write(hl, r); 11 }

            // DEC (HL)
            0x35 => { let hl = self.hl(); let v = self.bus.read(hl); let r = self.dec(v); self.bus.write(hl, r); 11 }

            // LD (HL),n
            0x36 => { let n = self.fetch(); self.bus.write(self.hl(), n); 10 }

            // SCF
            0x37 => { self.f.c = true; self.f.h = false; self.f.n = false; 4 }

            // JR C,e
            0x38 => self.jr(self.f.c),

            // ADD HL,SP
            0x39 => { self.add_hl(self.sp); 11 }

            // LD A,(nn)
            0x3A => { let addr = self.fetch16(); self.a = self.bus.read(addr); 13 }

            // DEC SP
            0x3B => { self.sp = self.sp.wrapping_sub(1); 6 }

            // INC A
            0x3C => { self.a = self.inc(self.a); 4 }

            // DEC A
            0x3D => { self.a = self.dec(self.a); 4 }

            // LD A,n
            0x3E => { self.a = self.fetch(); 7 }

            // CCF
            0x3F => { self.f.h = self.f.c; self.f.c = !self.f.c; self.f.n = false; 4 }

            // LD r,r' instructions (0x40-0x7F, excluding HALT at 0x76)
            0x40 => 4, // LD B,B
            0x41 => { self.b = self.c; 4 }
            0x42 => { self.b = self.d; 4 }
            0x43 => { self.b = self.e; 4 }
            0x44 => { self.b = self.h; 4 }
            0x45 => { self.b = self.l; 4 }
            0x46 => { self.b = self.bus.read(self.hl()); 7 }
            0x47 => { self.b = self.a; 4 }
            0x48 => { self.c = self.b; 4 }
            0x49 => 4, // LD C,C
            0x4A => { self.c = self.d; 4 }
            0x4B => { self.c = self.e; 4 }
            0x4C => { self.c = self.h; 4 }
            0x4D => { self.c = self.l; 4 }
            0x4E => { self.c = self.bus.read(self.hl()); 7 }
            0x4F => { self.c = self.a; 4 }
            0x50 => { self.d = self.b; 4 }
            0x51 => { self.d = self.c; 4 }
            0x52 => 4, // LD D,D
            0x53 => { self.d = self.e; 4 }
            0x54 => { self.d = self.h; 4 }
            0x55 => { self.d = self.l; 4 }
            0x56 => { self.d = self.bus.read(self.hl()); 7 }
            0x57 => { self.d = self.a; 4 }
            0x58 => { self.e = self.b; 4 }
            0x59 => { self.e = self.c; 4 }
            0x5A => { self.e = self.d; 4 }
            0x5B => 4, // LD E,E
            0x5C => { self.e = self.h; 4 }
            0x5D => { self.e = self.l; 4 }
            0x5E => { self.e = self.bus.read(self.hl()); 7 }
            0x5F => { self.e = self.a; 4 }
            0x60 => { self.h = self.b; 4 }
            0x61 => { self.h = self.c; 4 }
            0x62 => { self.h = self.d; 4 }
            0x63 => { self.h = self.e; 4 }
            0x64 => 4, // LD H,H
            0x65 => { self.h = self.l; 4 }
            0x66 => { self.h = self.bus.read(self.hl()); 7 }
            0x67 => { self.h = self.a; 4 }
            0x68 => { self.l = self.b; 4 }
            0x69 => { self.l = self.c; 4 }
            0x6A => { self.l = self.d; 4 }
            0x6B => { self.l = self.e; 4 }
            0x6C => { self.l = self.h; 4 }
            0x6D => 4, // LD L,L
            0x6E => { self.l = self.bus.read(self.hl()); 7 }
            0x6F => { self.l = self.a; 4 }
            0x70 => { self.bus.write(self.hl(), self.b); 7 }
            0x71 => { self.bus.write(self.hl(), self.c); 7 }
            0x72 => { self.bus.write(self.hl(), self.d); 7 }
            0x73 => { self.bus.write(self.hl(), self.e); 7 }
            0x74 => { self.bus.write(self.hl(), self.h); 7 }
            0x75 => { self.bus.write(self.hl(), self.l); 7 }

            // HALT
            0x76 => { self.halted = true; 4 }

            0x77 => { self.bus.write(self.hl(), self.a); 7 }
            0x78 => { self.a = self.b; 4 }
            0x79 => { self.a = self.c; 4 }
            0x7A => { self.a = self.d; 4 }
            0x7B => { self.a = self.e; 4 }
            0x7C => { self.a = self.h; 4 }
            0x7D => { self.a = self.l; 4 }
            0x7E => { self.a = self.bus.read(self.hl()); 7 }
            0x7F => 4, // LD A,A

            // ADD A,r
            0x80 => { self.add_a(self.b); 4 }
            0x81 => { self.add_a(self.c); 4 }
            0x82 => { self.add_a(self.d); 4 }
            0x83 => { self.add_a(self.e); 4 }
            0x84 => { self.add_a(self.h); 4 }
            0x85 => { self.add_a(self.l); 4 }
            0x86 => { let v = self.bus.read(self.hl()); self.add_a(v); 7 }
            0x87 => { self.add_a(self.a); 4 }

            // ADC A,r
            0x88 => { self.adc_a(self.b); 4 }
            0x89 => { self.adc_a(self.c); 4 }
            0x8A => { self.adc_a(self.d); 4 }
            0x8B => { self.adc_a(self.e); 4 }
            0x8C => { self.adc_a(self.h); 4 }
            0x8D => { self.adc_a(self.l); 4 }
            0x8E => { let v = self.bus.read(self.hl()); self.adc_a(v); 7 }
            0x8F => { self.adc_a(self.a); 4 }

            // SUB r
            0x90 => { self.sub_a(self.b); 4 }
            0x91 => { self.sub_a(self.c); 4 }
            0x92 => { self.sub_a(self.d); 4 }
            0x93 => { self.sub_a(self.e); 4 }
            0x94 => { self.sub_a(self.h); 4 }
            0x95 => { self.sub_a(self.l); 4 }
            0x96 => { let v = self.bus.read(self.hl()); self.sub_a(v); 7 }
            0x97 => { self.sub_a(self.a); 4 }

            // SBC A,r
            0x98 => { self.sbc_a(self.b); 4 }
            0x99 => { self.sbc_a(self.c); 4 }
            0x9A => { self.sbc_a(self.d); 4 }
            0x9B => { self.sbc_a(self.e); 4 }
            0x9C => { self.sbc_a(self.h); 4 }
            0x9D => { self.sbc_a(self.l); 4 }
            0x9E => { let v = self.bus.read(self.hl()); self.sbc_a(v); 7 }
            0x9F => { self.sbc_a(self.a); 4 }

            // AND r
            0xA0 => { self.and_a(self.b); 4 }
            0xA1 => { self.and_a(self.c); 4 }
            0xA2 => { self.and_a(self.d); 4 }
            0xA3 => { self.and_a(self.e); 4 }
            0xA4 => { self.and_a(self.h); 4 }
            0xA5 => { self.and_a(self.l); 4 }
            0xA6 => { let v = self.bus.read(self.hl()); self.and_a(v); 7 }
            0xA7 => { self.and_a(self.a); 4 }

            // XOR r
            0xA8 => { self.xor_a(self.b); 4 }
            0xA9 => { self.xor_a(self.c); 4 }
            0xAA => { self.xor_a(self.d); 4 }
            0xAB => { self.xor_a(self.e); 4 }
            0xAC => { self.xor_a(self.h); 4 }
            0xAD => { self.xor_a(self.l); 4 }
            0xAE => { let v = self.bus.read(self.hl()); self.xor_a(v); 7 }
            0xAF => { self.xor_a(self.a); 4 }

            // OR r
            0xB0 => { self.or_a(self.b); 4 }
            0xB1 => { self.or_a(self.c); 4 }
            0xB2 => { self.or_a(self.d); 4 }
            0xB3 => { self.or_a(self.e); 4 }
            0xB4 => { self.or_a(self.h); 4 }
            0xB5 => { self.or_a(self.l); 4 }
            0xB6 => { let v = self.bus.read(self.hl()); self.or_a(v); 7 }
            0xB7 => { self.or_a(self.a); 4 }

            // CP r
            0xB8 => { self.cp_a(self.b); 4 }
            0xB9 => { self.cp_a(self.c); 4 }
            0xBA => { self.cp_a(self.d); 4 }
            0xBB => { self.cp_a(self.e); 4 }
            0xBC => { self.cp_a(self.h); 4 }
            0xBD => { self.cp_a(self.l); 4 }
            0xBE => { let v = self.bus.read(self.hl()); self.cp_a(v); 7 }
            0xBF => { self.cp_a(self.a); 4 }

            // RET NZ
            0xC0 => self.ret(!self.f.z),

            // POP BC
            0xC1 => { let val = self.pop(); self.set_bc(val); 10 }

            // JP NZ,nn
            0xC2 => self.jp(!self.f.z),

            // JP nn
            0xC3 => { self.pc = self.fetch16(); 10 }

            // CALL NZ,nn
            0xC4 => self.call(!self.f.z),

            // PUSH BC
            0xC5 => { self.push(self.bc()); 11 }

            // ADD A,n
            0xC6 => { let n = self.fetch(); self.add_a(n); 7 }

            // RST 00H
            0xC7 => { self.rst(0x00); 11 }

            // RET Z
            0xC8 => self.ret(self.f.z),

            // RET
            0xC9 => { self.pc = self.pop(); 10 }

            // JP Z,nn
            0xCA => self.jp(self.f.z),

            // CB prefix (bit operations)
            0xCB => self.execute_cb(),

            // CALL Z,nn
            0xCC => self.call(self.f.z),

            // CALL nn
            0xCD => { let addr = self.fetch16(); self.push(self.pc); self.pc = addr; 17 }

            // ADC A,n
            0xCE => { let n = self.fetch(); self.adc_a(n); 7 }

            // RST 08H
            0xCF => { self.rst(0x08); 11 }

            // RET NC
            0xD0 => self.ret(!self.f.c),

            // POP DE
            0xD1 => { let val = self.pop(); self.set_de(val); 10 }

            // JP NC,nn
            0xD2 => self.jp(!self.f.c),

            // OUT (n),A
            0xD3 => { let port = self.fetch(); self.bus.io_write((self.a as u16) << 8 | port as u16, self.a); 11 }

            // CALL NC,nn
            0xD4 => self.call(!self.f.c),

            // PUSH DE
            0xD5 => { self.push(self.de()); 11 }

            // SUB n
            0xD6 => { let n = self.fetch(); self.sub_a(n); 7 }

            // RST 10H
            0xD7 => { self.rst(0x10); 11 }

            // RET C
            0xD8 => self.ret(self.f.c),

            // EXX
            0xD9 => { self.exx(); 4 }

            // JP C,nn
            0xDA => self.jp(self.f.c),

            // IN A,(n)
            0xDB => { let port = self.fetch(); self.a = self.bus.io_read((self.a as u16) << 8 | port as u16); 11 }

            // CALL C,nn
            0xDC => self.call(self.f.c),

            // DD prefix (IX instructions)
            0xDD => self.execute_dd(),

            // SBC A,n
            0xDE => { let n = self.fetch(); self.sbc_a(n); 7 }

            // RST 18H
            0xDF => { self.rst(0x18); 11 }

            // RET PO
            0xE0 => self.ret(!self.f.pv),

            // POP HL
            0xE1 => { let val = self.pop(); self.set_hl(val); 10 }

            // JP PO,nn
            0xE2 => self.jp(!self.f.pv),

            // EX (SP),HL
            0xE3 => {
                let val = self.read16(self.sp);
                self.write16(self.sp, self.hl());
                self.set_hl(val);
                19
            }

            // CALL PO,nn
            0xE4 => self.call(!self.f.pv),

            // PUSH HL
            0xE5 => { self.push(self.hl()); 11 }

            // AND n
            0xE6 => { let n = self.fetch(); self.and_a(n); 7 }

            // RST 20H
            0xE7 => { self.rst(0x20); 11 }

            // RET PE
            0xE8 => self.ret(self.f.pv),

            // JP (HL)
            0xE9 => { self.pc = self.hl(); 4 }

            // JP PE,nn
            0xEA => self.jp(self.f.pv),

            // EX DE,HL
            0xEB => {
                let de = self.de();
                let hl = self.hl();
                self.set_de(hl);
                self.set_hl(de);
                4
            }

            // CALL PE,nn
            0xEC => self.call(self.f.pv),

            // ED prefix (extended instructions)
            0xED => self.execute_ed(),

            // XOR n
            0xEE => { let n = self.fetch(); self.xor_a(n); 7 }

            // RST 28H
            0xEF => { self.rst(0x28); 11 }

            // RET P
            0xF0 => self.ret(!self.f.s),

            // POP AF
            0xF1 => { let val = self.pop(); self.set_af(val); 10 }

            // JP P,nn
            0xF2 => self.jp(!self.f.s),

            // DI
            0xF3 => { self.iff1 = false; self.iff2 = false; 4 }

            // CALL P,nn
            0xF4 => self.call(!self.f.s),

            // PUSH AF
            0xF5 => { self.push(self.af()); 11 }

            // OR n
            0xF6 => { let n = self.fetch(); self.or_a(n); 7 }

            // RST 30H
            0xF7 => { self.rst(0x30); 11 }

            // RET M
            0xF8 => self.ret(self.f.s),

            // LD SP,HL
            0xF9 => { self.sp = self.hl(); 6 }

            // JP M,nn
            0xFA => self.jp(self.f.s),

            // EI
            0xFB => { self.iff1 = true; self.iff2 = true; 4 }

            // CALL M,nn
            0xFC => self.call(self.f.s),

            // FD prefix (IY instructions)
            0xFD => self.execute_fd(),

            // CP n
            0xFE => { let n = self.fetch(); self.cp_a(n); 7 }

            // RST 38H
            0xFF => { self.rst(0x38); 11 }
        }
    }

    fn execute_cb(&mut self) -> u32 {
        let opcode = self.fetch();
        let reg_idx = opcode & 0x07;
        let op = (opcode >> 3) & 0x1F;

        let val = match reg_idx {
            0 => self.b,
            1 => self.c,
            2 => self.d,
            3 => self.e,
            4 => self.h,
            5 => self.l,
            6 => self.bus.read(self.hl()),
            7 => self.a,
            _ => unreachable!(),
        };

        let result = match op {
            0 => self.rlc(val),
            1 => self.rrc(val),
            2 => self.rl(val),
            3 => self.rr(val),
            4 => self.sla(val),
            5 => self.sra(val),
            6 => { // SLL (undocumented)
                let c = val >> 7;
                let result = (val << 1) | 1;
                self.f.s = result & 0x80 != 0;
                self.f.z = result == 0;
                self.f.h = false;
                self.f.pv = Self::parity(result);
                self.f.n = false;
                self.f.c = c != 0;
                result
            }
            7 => self.srl(val),
            8..=15 => { self.bit(op - 8, val); val }
            16..=23 => self.res(op - 16, val),
            24..=31 => self.set(op - 24, val),
            _ => unreachable!(),
        };

        // BIT operations don't write back
        if op < 8 || op >= 16 {
            match reg_idx {
                0 => self.b = result,
                1 => self.c = result,
                2 => self.d = result,
                3 => self.e = result,
                4 => self.h = result,
                5 => self.l = result,
                6 => self.bus.write(self.hl(), result),
                7 => self.a = result,
                _ => unreachable!(),
            }
        }

        if reg_idx == 6 {
            if op >= 8 && op < 16 { 12 } else { 15 }
        } else {
            8
        }
    }

    fn execute_dd(&mut self) -> u32 {
        let opcode = self.fetch();
        self.execute_index(opcode, true)
    }

    fn execute_fd(&mut self) -> u32 {
        let opcode = self.fetch();
        self.execute_index(opcode, false)
    }

    fn execute_index(&mut self, opcode: u8, use_ix: bool) -> u32 {
        let index = if use_ix { self.ix } else { self.iy };

        match opcode {
            // ADD IX/IY,BC
            0x09 => {
                let result = index.wrapping_add(self.bc());
                self.f.h = ((index & 0x0FFF) + (self.bc() & 0x0FFF)) > 0x0FFF;
                self.f.n = false;
                self.f.c = result < index;
                if use_ix { self.ix = result; } else { self.iy = result; }
                15
            }

            // ADD IX/IY,DE
            0x19 => {
                let result = index.wrapping_add(self.de());
                self.f.h = ((index & 0x0FFF) + (self.de() & 0x0FFF)) > 0x0FFF;
                self.f.n = false;
                self.f.c = result < index;
                if use_ix { self.ix = result; } else { self.iy = result; }
                15
            }

            // LD IX/IY,nn
            0x21 => {
                let val = self.fetch16();
                if use_ix { self.ix = val; } else { self.iy = val; }
                14
            }

            // LD (nn),IX/IY
            0x22 => {
                let addr = self.fetch16();
                self.write16(addr, index);
                20
            }

            // INC IX/IY
            0x23 => {
                if use_ix { self.ix = self.ix.wrapping_add(1); }
                else { self.iy = self.iy.wrapping_add(1); }
                10
            }

            // ADD IX/IY,IX/IY
            0x29 => {
                let result = index.wrapping_add(index);
                self.f.h = ((index & 0x0FFF) + (index & 0x0FFF)) > 0x0FFF;
                self.f.n = false;
                self.f.c = result < index;
                if use_ix { self.ix = result; } else { self.iy = result; }
                15
            }

            // LD IX/IY,(nn)
            0x2A => {
                let addr = self.fetch16();
                let val = self.read16(addr);
                if use_ix { self.ix = val; } else { self.iy = val; }
                20
            }

            // DEC IX/IY
            0x2B => {
                if use_ix { self.ix = self.ix.wrapping_sub(1); }
                else { self.iy = self.iy.wrapping_sub(1); }
                10
            }

            // INC (IX/IY+d)
            0x34 => {
                let d = self.fetch() as i8 as i16;
                let addr = (index as i16).wrapping_add(d) as u16;
                let val = self.bus.read(addr);
                let result = self.inc(val);
                self.bus.write(addr, result);
                23
            }

            // DEC (IX/IY+d)
            0x35 => {
                let d = self.fetch() as i8 as i16;
                let addr = (index as i16).wrapping_add(d) as u16;
                let val = self.bus.read(addr);
                let result = self.dec(val);
                self.bus.write(addr, result);
                23
            }

            // LD (IX/IY+d),n
            0x36 => {
                let d = self.fetch() as i8 as i16;
                let n = self.fetch();
                let addr = (index as i16).wrapping_add(d) as u16;
                self.bus.write(addr, n);
                19
            }

            // ADD IX/IY,SP
            0x39 => {
                let result = index.wrapping_add(self.sp);
                self.f.h = ((index & 0x0FFF) + (self.sp & 0x0FFF)) > 0x0FFF;
                self.f.n = false;
                self.f.c = result < index;
                if use_ix { self.ix = result; } else { self.iy = result; }
                15
            }

            // LD r,(IX/IY+d)
            0x46 | 0x4E | 0x56 | 0x5E | 0x66 | 0x6E | 0x7E => {
                let d = self.fetch() as i8 as i16;
                let addr = (index as i16).wrapping_add(d) as u16;
                let val = self.bus.read(addr);
                match opcode {
                    0x46 => self.b = val,
                    0x4E => self.c = val,
                    0x56 => self.d = val,
                    0x5E => self.e = val,
                    0x66 => self.h = val,
                    0x6E => self.l = val,
                    0x7E => self.a = val,
                    _ => {}
                }
                19
            }

            // LD (IX/IY+d),r
            0x70 | 0x71 | 0x72 | 0x73 | 0x74 | 0x75 | 0x77 => {
                let d = self.fetch() as i8 as i16;
                let addr = (index as i16).wrapping_add(d) as u16;
                let val = match opcode {
                    0x70 => self.b,
                    0x71 => self.c,
                    0x72 => self.d,
                    0x73 => self.e,
                    0x74 => self.h,
                    0x75 => self.l,
                    0x77 => self.a,
                    _ => 0,
                };
                self.bus.write(addr, val);
                19
            }

            // ADD/ADC/SUB/SBC/AND/XOR/OR/CP A,(IX/IY+d)
            0x86 | 0x8E | 0x96 | 0x9E | 0xA6 | 0xAE | 0xB6 | 0xBE => {
                let d = self.fetch() as i8 as i16;
                let addr = (index as i16).wrapping_add(d) as u16;
                let val = self.bus.read(addr);
                match opcode {
                    0x86 => self.add_a(val),
                    0x8E => self.adc_a(val),
                    0x96 => self.sub_a(val),
                    0x9E => self.sbc_a(val),
                    0xA6 => self.and_a(val),
                    0xAE => self.xor_a(val),
                    0xB6 => self.or_a(val),
                    0xBE => self.cp_a(val),
                    _ => {}
                }
                19
            }

            // CB prefix with IX/IY
            0xCB => {
                let d = self.fetch() as i8 as i16;
                let op = self.fetch();
                let addr = (index as i16).wrapping_add(d) as u16;
                let val = self.bus.read(addr);
                let bit = (op >> 3) & 0x07;
                let result = match op >> 6 {
                    0 => match (op >> 3) & 0x07 {
                        0 => self.rlc(val),
                        1 => self.rrc(val),
                        2 => self.rl(val),
                        3 => self.rr(val),
                        4 => self.sla(val),
                        5 => self.sra(val),
                        6 => (val << 1) | 1, // SLL
                        7 => self.srl(val),
                        _ => val,
                    },
                    1 => { self.bit(bit, val); val }
                    2 => self.res(bit, val),
                    3 => self.set(bit, val),
                    _ => val,
                };
                if (op >> 6) != 1 {
                    self.bus.write(addr, result);
                }
                23
            }

            // POP IX/IY
            0xE1 => {
                let val = self.pop();
                if use_ix { self.ix = val; } else { self.iy = val; }
                14
            }

            // EX (SP),IX/IY
            0xE3 => {
                let val = self.read16(self.sp);
                self.write16(self.sp, index);
                if use_ix { self.ix = val; } else { self.iy = val; }
                23
            }

            // PUSH IX/IY
            0xE5 => {
                self.push(index);
                15
            }

            // JP (IX/IY)
            0xE9 => {
                self.pc = index;
                8
            }

            // LD SP,IX/IY
            0xF9 => {
                self.sp = index;
                10
            }

            // Default: treat as NOP for unknown
            _ => 8,
        }
    }

    fn execute_ed(&mut self) -> u32 {
        let opcode = self.fetch();

        match opcode {
            // IN B,(C)
            0x40 => { self.b = self.bus.io_read(self.bc()); self.set_sz(self.b); self.f.h = false; self.f.pv = Self::parity(self.b); self.f.n = false; 12 }
            // OUT (C),B
            0x41 => { self.bus.io_write(self.bc(), self.b); 12 }
            // SBC HL,BC
            0x42 => { self.sbc_hl(self.bc()); 15 }
            // LD (nn),BC
            0x43 => { let addr = self.fetch16(); self.write16(addr, self.bc()); 20 }
            // NEG
            0x44 => { let a = self.a; self.a = 0; self.sub_a(a); 8 }
            // RETN
            0x45 => { self.iff1 = self.iff2; self.pc = self.pop(); 14 }
            // IM 0
            0x46 => { self.im = 0; 8 }
            // LD I,A
            0x47 => { self.i = self.a; 9 }
            // IN C,(C)
            0x48 => { self.c = self.bus.io_read(self.bc()); self.set_sz(self.c); self.f.h = false; self.f.pv = Self::parity(self.c); self.f.n = false; 12 }
            // OUT (C),C
            0x49 => { self.bus.io_write(self.bc(), self.c); 12 }
            // ADC HL,BC
            0x4A => { self.adc_hl(self.bc()); 15 }
            // LD BC,(nn)
            0x4B => { let addr = self.fetch16(); let val = self.read16(addr); self.set_bc(val); 20 }
            // RETI
            0x4D => { self.iff1 = self.iff2; self.pc = self.pop(); 14 }
            // LD R,A
            0x4F => { self.r = self.a; 9 }
            // IN D,(C)
            0x50 => { self.d = self.bus.io_read(self.bc()); self.set_sz(self.d); self.f.h = false; self.f.pv = Self::parity(self.d); self.f.n = false; 12 }
            // OUT (C),D
            0x51 => { self.bus.io_write(self.bc(), self.d); 12 }
            // SBC HL,DE
            0x52 => { self.sbc_hl(self.de()); 15 }
            // LD (nn),DE
            0x53 => { let addr = self.fetch16(); self.write16(addr, self.de()); 20 }
            // IM 1
            0x56 => { self.im = 1; 8 }
            // LD A,I
            0x57 => { self.a = self.i; self.set_sz(self.a); self.f.h = false; self.f.pv = self.iff2; self.f.n = false; 9 }
            // IN E,(C)
            0x58 => { self.e = self.bus.io_read(self.bc()); self.set_sz(self.e); self.f.h = false; self.f.pv = Self::parity(self.e); self.f.n = false; 12 }
            // OUT (C),E
            0x59 => { self.bus.io_write(self.bc(), self.e); 12 }
            // ADC HL,DE
            0x5A => { self.adc_hl(self.de()); 15 }
            // LD DE,(nn)
            0x5B => { let addr = self.fetch16(); let val = self.read16(addr); self.set_de(val); 20 }
            // IM 2
            0x5E => { self.im = 2; 8 }
            // LD A,R
            0x5F => { self.a = self.r; self.set_sz(self.a); self.f.h = false; self.f.pv = self.iff2; self.f.n = false; 9 }
            // IN H,(C)
            0x60 => { self.h = self.bus.io_read(self.bc()); self.set_sz(self.h); self.f.h = false; self.f.pv = Self::parity(self.h); self.f.n = false; 12 }
            // OUT (C),H
            0x61 => { self.bus.io_write(self.bc(), self.h); 12 }
            // SBC HL,HL
            0x62 => { let hl = self.hl(); self.sbc_hl(hl); 15 }
            // LD (nn),HL (ED version)
            0x63 => { let addr = self.fetch16(); self.write16(addr, self.hl()); 20 }
            // RRD
            0x67 => {
                let hl = self.hl();
                let mem = self.bus.read(hl);
                let result = (self.a << 4) | (mem >> 4);
                self.a = (self.a & 0xF0) | (mem & 0x0F);
                self.bus.write(hl, result);
                self.set_sz(self.a);
                self.f.h = false;
                self.f.pv = Self::parity(self.a);
                self.f.n = false;
                18
            }
            // IN L,(C)
            0x68 => { self.l = self.bus.io_read(self.bc()); self.set_sz(self.l); self.f.h = false; self.f.pv = Self::parity(self.l); self.f.n = false; 12 }
            // OUT (C),L
            0x69 => { self.bus.io_write(self.bc(), self.l); 12 }
            // ADC HL,HL
            0x6A => { let hl = self.hl(); self.adc_hl(hl); 15 }
            // LD HL,(nn) (ED version)
            0x6B => { let addr = self.fetch16(); let val = self.read16(addr); self.set_hl(val); 20 }
            // RLD
            0x6F => {
                let hl = self.hl();
                let mem = self.bus.read(hl);
                let result = (mem << 4) | (self.a & 0x0F);
                self.a = (self.a & 0xF0) | (mem >> 4);
                self.bus.write(hl, result);
                self.set_sz(self.a);
                self.f.h = false;
                self.f.pv = Self::parity(self.a);
                self.f.n = false;
                18
            }
            // IN (C) / SBC HL,SP
            0x70 => { let _ = self.bus.io_read(self.bc()); 12 }
            // OUT (C),0
            0x71 => { self.bus.io_write(self.bc(), 0); 12 }
            // SBC HL,SP
            0x72 => { self.sbc_hl(self.sp); 15 }
            // LD (nn),SP
            0x73 => { let addr = self.fetch16(); self.write16(addr, self.sp); 20 }
            // IN A,(C)
            0x78 => { self.a = self.bus.io_read(self.bc()); self.set_sz(self.a); self.f.h = false; self.f.pv = Self::parity(self.a); self.f.n = false; 12 }
            // OUT (C),A
            0x79 => { self.bus.io_write(self.bc(), self.a); 12 }
            // ADC HL,SP
            0x7A => { self.adc_hl(self.sp); 15 }
            // LD SP,(nn)
            0x7B => { let addr = self.fetch16(); self.sp = self.read16(addr); 20 }

            // LDI
            0xA0 => {
                let val = self.bus.read(self.hl());
                self.bus.write(self.de(), val);
                self.set_hl(self.hl().wrapping_add(1));
                self.set_de(self.de().wrapping_add(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.h = false;
                self.f.pv = self.bc() != 0;
                self.f.n = false;
                16
            }

            // CPI
            0xA1 => {
                let val = self.bus.read(self.hl());
                let result = self.a.wrapping_sub(val);
                self.set_hl(self.hl().wrapping_add(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.s = result & 0x80 != 0;
                self.f.z = result == 0;
                self.f.h = (self.a & 0x0F) < (val & 0x0F);
                self.f.pv = self.bc() != 0;
                self.f.n = true;
                16
            }

            // INI
            0xA2 => {
                let val = self.bus.io_read(self.bc());
                self.bus.write(self.hl(), val);
                self.set_hl(self.hl().wrapping_add(1));
                self.b = self.b.wrapping_sub(1);
                self.f.z = self.b == 0;
                self.f.n = true;
                16
            }

            // OUTI
            0xA3 => {
                let val = self.bus.read(self.hl());
                self.b = self.b.wrapping_sub(1);
                self.bus.io_write(self.bc(), val);
                self.set_hl(self.hl().wrapping_add(1));
                self.f.z = self.b == 0;
                self.f.n = true;
                16
            }

            // LDD
            0xA8 => {
                let val = self.bus.read(self.hl());
                self.bus.write(self.de(), val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.set_de(self.de().wrapping_sub(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.h = false;
                self.f.pv = self.bc() != 0;
                self.f.n = false;
                16
            }

            // CPD
            0xA9 => {
                let val = self.bus.read(self.hl());
                let result = self.a.wrapping_sub(val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.s = result & 0x80 != 0;
                self.f.z = result == 0;
                self.f.h = (self.a & 0x0F) < (val & 0x0F);
                self.f.pv = self.bc() != 0;
                self.f.n = true;
                16
            }

            // IND
            0xAA => {
                let val = self.bus.io_read(self.bc());
                self.bus.write(self.hl(), val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.b = self.b.wrapping_sub(1);
                self.f.z = self.b == 0;
                self.f.n = true;
                16
            }

            // OUTD
            0xAB => {
                let val = self.bus.read(self.hl());
                self.b = self.b.wrapping_sub(1);
                self.bus.io_write(self.bc(), val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.f.z = self.b == 0;
                self.f.n = true;
                16
            }

            // LDIR
            0xB0 => {
                let val = self.bus.read(self.hl());
                self.bus.write(self.de(), val);
                self.set_hl(self.hl().wrapping_add(1));
                self.set_de(self.de().wrapping_add(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.h = false;
                self.f.pv = false;
                self.f.n = false;
                if self.bc() != 0 {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // CPIR
            0xB1 => {
                let val = self.bus.read(self.hl());
                let result = self.a.wrapping_sub(val);
                self.set_hl(self.hl().wrapping_add(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.s = result & 0x80 != 0;
                self.f.z = result == 0;
                self.f.h = (self.a & 0x0F) < (val & 0x0F);
                self.f.pv = self.bc() != 0;
                self.f.n = true;
                if self.bc() != 0 && !self.f.z {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // INIR
            0xB2 => {
                let val = self.bus.io_read(self.bc());
                self.bus.write(self.hl(), val);
                self.set_hl(self.hl().wrapping_add(1));
                self.b = self.b.wrapping_sub(1);
                self.f.z = true;
                self.f.n = true;
                if self.b != 0 {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // OTIR
            0xB3 => {
                let val = self.bus.read(self.hl());
                self.b = self.b.wrapping_sub(1);
                self.bus.io_write(self.bc(), val);
                self.set_hl(self.hl().wrapping_add(1));
                self.f.z = true;
                self.f.n = true;
                if self.b != 0 {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // LDDR
            0xB8 => {
                let val = self.bus.read(self.hl());
                self.bus.write(self.de(), val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.set_de(self.de().wrapping_sub(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.h = false;
                self.f.pv = false;
                self.f.n = false;
                if self.bc() != 0 {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // CPDR
            0xB9 => {
                let val = self.bus.read(self.hl());
                let result = self.a.wrapping_sub(val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.set_bc(self.bc().wrapping_sub(1));
                self.f.s = result & 0x80 != 0;
                self.f.z = result == 0;
                self.f.h = (self.a & 0x0F) < (val & 0x0F);
                self.f.pv = self.bc() != 0;
                self.f.n = true;
                if self.bc() != 0 && !self.f.z {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // INDR
            0xBA => {
                let val = self.bus.io_read(self.bc());
                self.bus.write(self.hl(), val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.b = self.b.wrapping_sub(1);
                self.f.z = true;
                self.f.n = true;
                if self.b != 0 {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // OTDR
            0xBB => {
                let val = self.bus.read(self.hl());
                self.b = self.b.wrapping_sub(1);
                self.bus.io_write(self.bc(), val);
                self.set_hl(self.hl().wrapping_sub(1));
                self.f.z = true;
                self.f.n = true;
                if self.b != 0 {
                    self.pc = self.pc.wrapping_sub(2);
                    21
                } else {
                    16
                }
            }

            // Unknown ED opcodes act as NOPs
            _ => 8,
        }
    }
}

impl<B: BusZ80> Cpu for Z80<B> {
    fn step(&mut self) -> u32 {
        self.execute()
    }

    fn reset(&mut self) {
        self.a = 0xFF;
        self.f = Z80Flags::default();
        self.b = 0;
        self.c = 0;
        self.d = 0;
        self.e = 0;
        self.h = 0;
        self.l = 0;
        self.ix = 0;
        self.iy = 0;
        self.sp = 0xFFFF;
        self.pc = 0;
        self.i = 0;
        self.r = 0;
        self.iff1 = false;
        self.iff2 = false;
        self.im = 0;
        self.halted = false;
        self.cycles = 0;
    }

    fn irq(&mut self) {
        self.irq_pending = true;
    }

    fn nmi(&mut self) {
        self.nmi_pending = true;
    }

    fn pc(&self) -> u16 {
        self.pc
    }

    fn set_pc(&mut self, pc: u16) {
        self.pc = pc;
    }

    fn cycles(&self) -> u64 {
        self.cycles
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestBus {
        ram: [u8; 0x10000],
    }

    impl TestBus {
        fn new() -> Self {
            Self { ram: [0; 0x10000] }
        }
    }

    impl BusZ80 for TestBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.ram[addr as usize]
        }

        fn write(&mut self, addr: u16, data: u8) {
            self.ram[addr as usize] = data;
        }

        fn io_read(&mut self, _port: u16) -> u8 {
            0xFF
        }

        fn io_write(&mut self, _port: u16, _data: u8) {}
    }

    #[test]
    fn test_ld_immediate() {
        let mut bus = TestBus::new();
        bus.ram[0] = 0x3E; // LD A,n
        bus.ram[1] = 0x42;

        let mut cpu = Z80::new(bus);
        cpu.step();

        assert_eq!(cpu.a, 0x42);
    }

    #[test]
    fn test_add() {
        let mut bus = TestBus::new();
        bus.ram[0] = 0x3E; // LD A,50
        bus.ram[1] = 0x50;
        bus.ram[2] = 0xC6; // ADD A,30
        bus.ram[3] = 0x30;

        let mut cpu = Z80::new(bus);
        cpu.step();
        cpu.step();

        assert_eq!(cpu.a, 0x80);
        assert!(cpu.f.s);
        assert!(cpu.f.pv); // Overflow
    }
}
