//! MOS 6502 CPU Emulator
//!
//! The 6502 is an 8-bit microprocessor used in the NES/Famicom.
//! This is a cycle-accurate implementation.

use super::{Cpu, StatusFlags};

/// Memory access trait for the 6502
pub trait Bus6502 {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, data: u8);
}

/// 6502 CPU state
pub struct Mos6502<B: Bus6502> {
    /// Accumulator
    pub a: u8,
    /// X index register
    pub x: u8,
    /// Y index register
    pub y: u8,
    /// Stack pointer
    pub sp: u8,
    /// Program counter
    pub pc: u16,
    /// Status flags
    pub status: StatusFlags,
    /// Memory bus
    pub bus: B,
    /// Total cycles executed
    pub cycles: u64,
    /// Interrupt pending
    irq_pending: bool,
    /// NMI pending
    nmi_pending: bool,
    /// Stall cycles (for DMA)
    stall: u32,
}

impl<B: Bus6502> Mos6502<B> {
    pub fn new(bus: B) -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            status: StatusFlags::default(),
            bus,
            cycles: 0,
            irq_pending: false,
            nmi_pending: false,
            stall: 0,
        }
    }

    /// Get status register as byte
    pub fn get_status(&self) -> u8 {
        let mut status = 0x20; // Bit 5 always set
        if self.status.carry { status |= 0x01; }
        if self.status.zero { status |= 0x02; }
        if self.status.interrupt_disable { status |= 0x04; }
        if self.status.decimal { status |= 0x08; }
        if self.status.break_flag { status |= 0x10; }
        if self.status.overflow { status |= 0x40; }
        if self.status.negative { status |= 0x80; }
        status
    }

    /// Set status register from byte
    pub fn set_status(&mut self, value: u8) {
        self.status.carry = value & 0x01 != 0;
        self.status.zero = value & 0x02 != 0;
        self.status.interrupt_disable = value & 0x04 != 0;
        self.status.decimal = value & 0x08 != 0;
        self.status.break_flag = value & 0x10 != 0;
        self.status.overflow = value & 0x40 != 0;
        self.status.negative = value & 0x80 != 0;
    }

    /// Push byte to stack
    fn push(&mut self, value: u8) {
        self.bus.write(0x0100 | self.sp as u16, value);
        self.sp = self.sp.wrapping_sub(1);
    }

    /// Pop byte from stack
    fn pop(&mut self) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        self.bus.read(0x0100 | self.sp as u16)
    }

    /// Push 16-bit value to stack
    fn push16(&mut self, value: u16) {
        self.push((value >> 8) as u8);
        self.push(value as u8);
    }

    /// Pop 16-bit value from stack
    fn pop16(&mut self) -> u16 {
        let lo = self.pop() as u16;
        let hi = self.pop() as u16;
        (hi << 8) | lo
    }

    /// Read 16-bit value from memory
    fn read16(&mut self, addr: u16) -> u16 {
        let lo = self.bus.read(addr) as u16;
        let hi = self.bus.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    /// Read 16-bit with page wrap bug (for indirect addressing)
    fn read16_bug(&mut self, addr: u16) -> u16 {
        let lo = self.bus.read(addr) as u16;
        let hi_addr = (addr & 0xFF00) | ((addr + 1) & 0x00FF);
        let hi = self.bus.read(hi_addr) as u16;
        (hi << 8) | lo
    }

    /// Set zero and negative flags based on value
    fn set_zn(&mut self, value: u8) {
        self.status.zero = value == 0;
        self.status.negative = value & 0x80 != 0;
    }

    /// Check if page crossed (for extra cycle)
    fn pages_differ(a: u16, b: u16) -> bool {
        (a & 0xFF00) != (b & 0xFF00)
    }

    /// Add stall cycles (for DMA transfers)
    pub fn add_stall(&mut self, cycles: u32) {
        self.stall += cycles;
    }

    /// Execute one instruction
    pub fn execute(&mut self) -> u32 {
        // Handle stall cycles first
        if self.stall > 0 {
            self.stall -= 1;
            self.cycles += 1;
            return 1;
        }

        // Check for NMI
        if self.nmi_pending {
            self.nmi_pending = false;
            self.handle_nmi();
        }
        // Check for IRQ
        else if self.irq_pending && !self.status.interrupt_disable {
            self.irq_pending = false;
            self.handle_irq();
        }

        let opcode = self.bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        let cycles = self.execute_opcode(opcode);
        self.cycles += cycles as u64;
        cycles
    }

    fn handle_nmi(&mut self) {
        self.push16(self.pc);
        self.push(self.get_status() & !0x10); // Clear break flag
        self.status.interrupt_disable = true;
        self.pc = self.read16(0xFFFA);
        self.cycles += 7;
    }

    fn handle_irq(&mut self) {
        self.push16(self.pc);
        self.push(self.get_status() & !0x10);
        self.status.interrupt_disable = true;
        self.pc = self.read16(0xFFFE);
        self.cycles += 7;
    }

    /// Execute a single opcode
    fn execute_opcode(&mut self, opcode: u8) -> u32 {
        match opcode {
            // BRK - Force Interrupt
            0x00 => {
                self.pc = self.pc.wrapping_add(1);
                self.push16(self.pc);
                self.push(self.get_status() | 0x10);
                self.status.interrupt_disable = true;
                self.pc = self.read16(0xFFFE);
                7
            }

            // ORA - OR with Accumulator
            0x01 => { self.ora_indexed_indirect(); 6 }
            0x05 => { self.ora_zero_page(); 3 }
            0x09 => { self.ora_immediate(); 2 }
            0x0D => { self.ora_absolute(); 4 }
            0x11 => { self.ora_indirect_indexed() }
            0x15 => { self.ora_zero_page_x(); 4 }
            0x19 => { self.ora_absolute_y() }
            0x1D => { self.ora_absolute_x() }

            // ASL - Arithmetic Shift Left
            0x06 => { self.asl_zero_page(); 5 }
            0x0A => { self.asl_accumulator(); 2 }
            0x0E => { self.asl_absolute(); 6 }
            0x16 => { self.asl_zero_page_x(); 6 }
            0x1E => { self.asl_absolute_x(); 7 }

            // PHP - Push Processor Status
            0x08 => {
                self.push(self.get_status() | 0x10);
                3
            }

            // BPL - Branch if Positive
            0x10 => self.branch(!self.status.negative),

            // CLC - Clear Carry Flag
            0x18 => {
                self.status.carry = false;
                2
            }

            // JSR - Jump to Subroutine
            0x20 => {
                let addr = self.read16(self.pc);
                self.push16(self.pc.wrapping_add(1));
                self.pc = addr;
                6
            }

            // AND - Logical AND
            0x21 => { self.and_indexed_indirect(); 6 }
            0x25 => { self.and_zero_page(); 3 }
            0x29 => { self.and_immediate(); 2 }
            0x2D => { self.and_absolute(); 4 }
            0x31 => { self.and_indirect_indexed() }
            0x35 => { self.and_zero_page_x(); 4 }
            0x39 => { self.and_absolute_y() }
            0x3D => { self.and_absolute_x() }

            // BIT - Bit Test
            0x24 => { self.bit_zero_page(); 3 }
            0x2C => { self.bit_absolute(); 4 }

            // ROL - Rotate Left
            0x26 => { self.rol_zero_page(); 5 }
            0x2A => { self.rol_accumulator(); 2 }
            0x2E => { self.rol_absolute(); 6 }
            0x36 => { self.rol_zero_page_x(); 6 }
            0x3E => { self.rol_absolute_x(); 7 }

            // PLP - Pull Processor Status
            0x28 => {
                let status = self.pop();
                self.set_status(status);
                4
            }

            // BMI - Branch if Minus
            0x30 => self.branch(self.status.negative),

            // SEC - Set Carry Flag
            0x38 => {
                self.status.carry = true;
                2
            }

            // RTI - Return from Interrupt
            0x40 => {
                let status = self.pop();
                self.set_status(status);
                self.pc = self.pop16();
                6
            }

            // EOR - Exclusive OR
            0x41 => { self.eor_indexed_indirect(); 6 }
            0x45 => { self.eor_zero_page(); 3 }
            0x49 => { self.eor_immediate(); 2 }
            0x4D => { self.eor_absolute(); 4 }
            0x51 => { self.eor_indirect_indexed() }
            0x55 => { self.eor_zero_page_x(); 4 }
            0x59 => { self.eor_absolute_y() }
            0x5D => { self.eor_absolute_x() }

            // LSR - Logical Shift Right
            0x46 => { self.lsr_zero_page(); 5 }
            0x4A => { self.lsr_accumulator(); 2 }
            0x4E => { self.lsr_absolute(); 6 }
            0x56 => { self.lsr_zero_page_x(); 6 }
            0x5E => { self.lsr_absolute_x(); 7 }

            // PHA - Push Accumulator
            0x48 => {
                self.push(self.a);
                3
            }

            // JMP - Jump
            0x4C => {
                self.pc = self.read16(self.pc);
                3
            }
            0x6C => {
                let addr = self.read16(self.pc);
                self.pc = self.read16_bug(addr);
                5
            }

            // BVC - Branch if Overflow Clear
            0x50 => self.branch(!self.status.overflow),

            // CLI - Clear Interrupt Disable
            0x58 => {
                self.status.interrupt_disable = false;
                2
            }

            // RTS - Return from Subroutine
            0x60 => {
                self.pc = self.pop16().wrapping_add(1);
                6
            }

            // ADC - Add with Carry
            0x61 => { self.adc_indexed_indirect(); 6 }
            0x65 => { self.adc_zero_page(); 3 }
            0x69 => { self.adc_immediate(); 2 }
            0x6D => { self.adc_absolute(); 4 }
            0x71 => { self.adc_indirect_indexed() }
            0x75 => { self.adc_zero_page_x(); 4 }
            0x79 => { self.adc_absolute_y() }
            0x7D => { self.adc_absolute_x() }

            // ROR - Rotate Right
            0x66 => { self.ror_zero_page(); 5 }
            0x6A => { self.ror_accumulator(); 2 }
            0x6E => { self.ror_absolute(); 6 }
            0x76 => { self.ror_zero_page_x(); 6 }
            0x7E => { self.ror_absolute_x(); 7 }

            // PLA - Pull Accumulator
            0x68 => {
                self.a = self.pop();
                self.set_zn(self.a);
                4
            }

            // BVS - Branch if Overflow Set
            0x70 => self.branch(self.status.overflow),

            // SEI - Set Interrupt Disable
            0x78 => {
                self.status.interrupt_disable = true;
                2
            }

            // STA - Store Accumulator
            0x81 => { self.sta_indexed_indirect(); 6 }
            0x85 => { self.sta_zero_page(); 3 }
            0x8D => { self.sta_absolute(); 4 }
            0x91 => { self.sta_indirect_indexed(); 6 }
            0x95 => { self.sta_zero_page_x(); 4 }
            0x99 => { self.sta_absolute_y(); 5 }
            0x9D => { self.sta_absolute_x(); 5 }

            // STY - Store Y Register
            0x84 => { self.sty_zero_page(); 3 }
            0x8C => { self.sty_absolute(); 4 }
            0x94 => { self.sty_zero_page_x(); 4 }

            // STX - Store X Register
            0x86 => { self.stx_zero_page(); 3 }
            0x8E => { self.stx_absolute(); 4 }
            0x96 => { self.stx_zero_page_y(); 4 }

            // DEY - Decrement Y Register
            0x88 => {
                self.y = self.y.wrapping_sub(1);
                self.set_zn(self.y);
                2
            }

            // TXA - Transfer X to Accumulator
            0x8A => {
                self.a = self.x;
                self.set_zn(self.a);
                2
            }

            // BCC - Branch if Carry Clear
            0x90 => self.branch(!self.status.carry),

            // TYA - Transfer Y to Accumulator
            0x98 => {
                self.a = self.y;
                self.set_zn(self.a);
                2
            }

            // TXS - Transfer X to Stack Pointer
            0x9A => {
                self.sp = self.x;
                2
            }

            // LDY - Load Y Register
            0xA0 => { self.ldy_immediate(); 2 }
            0xA4 => { self.ldy_zero_page(); 3 }
            0xAC => { self.ldy_absolute(); 4 }
            0xB4 => { self.ldy_zero_page_x(); 4 }
            0xBC => { self.ldy_absolute_x() }

            // LDA - Load Accumulator
            0xA1 => { self.lda_indexed_indirect(); 6 }
            0xA5 => { self.lda_zero_page(); 3 }
            0xA9 => { self.lda_immediate(); 2 }
            0xAD => { self.lda_absolute(); 4 }
            0xB1 => { self.lda_indirect_indexed() }
            0xB5 => { self.lda_zero_page_x(); 4 }
            0xB9 => { self.lda_absolute_y() }
            0xBD => { self.lda_absolute_x() }

            // LDX - Load X Register
            0xA2 => { self.ldx_immediate(); 2 }
            0xA6 => { self.ldx_zero_page(); 3 }
            0xAE => { self.ldx_absolute(); 4 }
            0xB6 => { self.ldx_zero_page_y(); 4 }
            0xBE => { self.ldx_absolute_y() }

            // TAY - Transfer Accumulator to Y
            0xA8 => {
                self.y = self.a;
                self.set_zn(self.y);
                2
            }

            // TAX - Transfer Accumulator to X
            0xAA => {
                self.x = self.a;
                self.set_zn(self.x);
                2
            }

            // BCS - Branch if Carry Set
            0xB0 => self.branch(self.status.carry),

            // CLV - Clear Overflow Flag
            0xB8 => {
                self.status.overflow = false;
                2
            }

            // TSX - Transfer Stack Pointer to X
            0xBA => {
                self.x = self.sp;
                self.set_zn(self.x);
                2
            }

            // CPY - Compare Y Register
            0xC0 => { self.cpy_immediate(); 2 }
            0xC4 => { self.cpy_zero_page(); 3 }
            0xCC => { self.cpy_absolute(); 4 }

            // CMP - Compare
            0xC1 => { self.cmp_indexed_indirect(); 6 }
            0xC5 => { self.cmp_zero_page(); 3 }
            0xC9 => { self.cmp_immediate(); 2 }
            0xCD => { self.cmp_absolute(); 4 }
            0xD1 => { self.cmp_indirect_indexed() }
            0xD5 => { self.cmp_zero_page_x(); 4 }
            0xD9 => { self.cmp_absolute_y() }
            0xDD => { self.cmp_absolute_x() }

            // DEC - Decrement Memory
            0xC6 => { self.dec_zero_page(); 5 }
            0xCE => { self.dec_absolute(); 6 }
            0xD6 => { self.dec_zero_page_x(); 6 }
            0xDE => { self.dec_absolute_x(); 7 }

            // INY - Increment Y Register
            0xC8 => {
                self.y = self.y.wrapping_add(1);
                self.set_zn(self.y);
                2
            }

            // DEX - Decrement X Register
            0xCA => {
                self.x = self.x.wrapping_sub(1);
                self.set_zn(self.x);
                2
            }

            // BNE - Branch if Not Equal
            0xD0 => self.branch(!self.status.zero),

            // CLD - Clear Decimal Mode
            0xD8 => {
                self.status.decimal = false;
                2
            }

            // CPX - Compare X Register
            0xE0 => { self.cpx_immediate(); 2 }
            0xE4 => { self.cpx_zero_page(); 3 }
            0xEC => { self.cpx_absolute(); 4 }

            // SBC - Subtract with Carry
            0xE1 => { self.sbc_indexed_indirect(); 6 }
            0xE5 => { self.sbc_zero_page(); 3 }
            0xE9 => { self.sbc_immediate(); 2 }
            0xED => { self.sbc_absolute(); 4 }
            0xF1 => { self.sbc_indirect_indexed() }
            0xF5 => { self.sbc_zero_page_x(); 4 }
            0xF9 => { self.sbc_absolute_y() }
            0xFD => { self.sbc_absolute_x() }

            // INC - Increment Memory
            0xE6 => { self.inc_zero_page(); 5 }
            0xEE => { self.inc_absolute(); 6 }
            0xF6 => { self.inc_zero_page_x(); 6 }
            0xFE => { self.inc_absolute_x(); 7 }

            // INX - Increment X Register
            0xE8 => {
                self.x = self.x.wrapping_add(1);
                self.set_zn(self.x);
                2
            }

            // NOP - No Operation
            0xEA => 2,

            // BEQ - Branch if Equal
            0xF0 => self.branch(self.status.zero),

            // SED - Set Decimal Flag
            0xF8 => {
                self.status.decimal = true;
                2
            }

            // Unofficial opcodes (NOPs and common ones)
            0x04 | 0x44 | 0x64 => { self.pc = self.pc.wrapping_add(1); 3 } // DOP
            0x0C => { self.pc = self.pc.wrapping_add(2); 4 } // TOP
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => { self.pc = self.pc.wrapping_add(1); 4 }
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => 2, // NOP
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                let addr = self.read16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                let base = addr.wrapping_sub(self.x as u16);
                if Self::pages_differ(base, addr) { 5 } else { 4 }
            }
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => { self.pc = self.pc.wrapping_add(1); 2 }

            // LAX - LDA + LDX
            0xA3 => { self.lax_indexed_indirect(); 6 }
            0xA7 => { self.lax_zero_page(); 3 }
            0xAF => { self.lax_absolute(); 4 }
            0xB3 => { self.lax_indirect_indexed() }
            0xB7 => { self.lax_zero_page_y(); 4 }
            0xBF => { self.lax_absolute_y() }

            // SAX - Store A & X
            0x83 => { self.sax_indexed_indirect(); 6 }
            0x87 => { self.sax_zero_page(); 3 }
            0x8F => { self.sax_absolute(); 4 }
            0x97 => { self.sax_zero_page_y(); 4 }

            // DCP - DEC + CMP
            0xC3 => { self.dcp_indexed_indirect(); 8 }
            0xC7 => { self.dcp_zero_page(); 5 }
            0xCF => { self.dcp_absolute(); 6 }
            0xD3 => { self.dcp_indirect_indexed(); 8 }
            0xD7 => { self.dcp_zero_page_x(); 6 }
            0xDB => { self.dcp_absolute_y(); 7 }
            0xDF => { self.dcp_absolute_x(); 7 }

            // ISC/ISB - INC + SBC
            0xE3 => { self.isb_indexed_indirect(); 8 }
            0xE7 => { self.isb_zero_page(); 5 }
            0xEF => { self.isb_absolute(); 6 }
            0xF3 => { self.isb_indirect_indexed(); 8 }
            0xF7 => { self.isb_zero_page_x(); 6 }
            0xFB => { self.isb_absolute_y(); 7 }
            0xFF => { self.isb_absolute_x(); 7 }

            // SLO - ASL + ORA
            0x03 => { self.slo_indexed_indirect(); 8 }
            0x07 => { self.slo_zero_page(); 5 }
            0x0F => { self.slo_absolute(); 6 }
            0x13 => { self.slo_indirect_indexed(); 8 }
            0x17 => { self.slo_zero_page_x(); 6 }
            0x1B => { self.slo_absolute_y(); 7 }
            0x1F => { self.slo_absolute_x(); 7 }

            // RLA - ROL + AND
            0x23 => { self.rla_indexed_indirect(); 8 }
            0x27 => { self.rla_zero_page(); 5 }
            0x2F => { self.rla_absolute(); 6 }
            0x33 => { self.rla_indirect_indexed(); 8 }
            0x37 => { self.rla_zero_page_x(); 6 }
            0x3B => { self.rla_absolute_y(); 7 }
            0x3F => { self.rla_absolute_x(); 7 }

            // SRE - LSR + EOR
            0x43 => { self.sre_indexed_indirect(); 8 }
            0x47 => { self.sre_zero_page(); 5 }
            0x4F => { self.sre_absolute(); 6 }
            0x53 => { self.sre_indirect_indexed(); 8 }
            0x57 => { self.sre_zero_page_x(); 6 }
            0x5B => { self.sre_absolute_y(); 7 }
            0x5F => { self.sre_absolute_x(); 7 }

            // RRA - ROR + ADC
            0x63 => { self.rra_indexed_indirect(); 8 }
            0x67 => { self.rra_zero_page(); 5 }
            0x6F => { self.rra_absolute(); 6 }
            0x73 => { self.rra_indirect_indexed(); 8 }
            0x77 => { self.rra_zero_page_x(); 6 }
            0x7B => { self.rra_absolute_y(); 7 }
            0x7F => { self.rra_absolute_x(); 7 }

            // Unknown/KIL opcodes - treat as NOP
            _ => {
                tracing::warn!("Unknown opcode: 0x{:02X} at PC 0x{:04X}", opcode, self.pc.wrapping_sub(1));
                2
            }
        }
    }

    // ========================================================================
    // Addressing Modes
    // ========================================================================

    fn read_immediate(&mut self) -> u8 {
        let value = self.bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        value
    }

    fn read_zero_page(&mut self) -> (u16, u8) {
        let addr = self.bus.read(self.pc) as u16;
        self.pc = self.pc.wrapping_add(1);
        (addr, self.bus.read(addr))
    }

    fn read_zero_page_x(&mut self) -> (u16, u8) {
        let addr = self.bus.read(self.pc).wrapping_add(self.x) as u16;
        self.pc = self.pc.wrapping_add(1);
        (addr, self.bus.read(addr))
    }

    fn read_zero_page_y(&mut self) -> (u16, u8) {
        let addr = self.bus.read(self.pc).wrapping_add(self.y) as u16;
        self.pc = self.pc.wrapping_add(1);
        (addr, self.bus.read(addr))
    }

    fn read_absolute(&mut self) -> (u16, u8) {
        let addr = self.read16(self.pc);
        self.pc = self.pc.wrapping_add(2);
        (addr, self.bus.read(addr))
    }

    fn read_absolute_x(&mut self) -> (u16, u8, bool) {
        let base = self.read16(self.pc);
        self.pc = self.pc.wrapping_add(2);
        let addr = base.wrapping_add(self.x as u16);
        let crossed = Self::pages_differ(base, addr);
        (addr, self.bus.read(addr), crossed)
    }

    fn read_absolute_y(&mut self) -> (u16, u8, bool) {
        let base = self.read16(self.pc);
        self.pc = self.pc.wrapping_add(2);
        let addr = base.wrapping_add(self.y as u16);
        let crossed = Self::pages_differ(base, addr);
        (addr, self.bus.read(addr), crossed)
    }

    fn read_indexed_indirect(&mut self) -> (u16, u8) {
        let ptr = self.bus.read(self.pc).wrapping_add(self.x);
        self.pc = self.pc.wrapping_add(1);
        let lo = self.bus.read(ptr as u16) as u16;
        let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16;
        let addr = (hi << 8) | lo;
        (addr, self.bus.read(addr))
    }

    fn read_indirect_indexed(&mut self) -> (u16, u8, bool) {
        let ptr = self.bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);
        let lo = self.bus.read(ptr as u16) as u16;
        let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16;
        let base = (hi << 8) | lo;
        let addr = base.wrapping_add(self.y as u16);
        let crossed = Self::pages_differ(base, addr);
        (addr, self.bus.read(addr), crossed)
    }

    // ========================================================================
    // Branch instruction
    // ========================================================================

    fn branch(&mut self, condition: bool) -> u32 {
        let offset = self.bus.read(self.pc) as i8;
        self.pc = self.pc.wrapping_add(1);
        if condition {
            let old_pc = self.pc;
            self.pc = self.pc.wrapping_add(offset as u16);
            if Self::pages_differ(old_pc, self.pc) { 4 } else { 3 }
        } else {
            2
        }
    }

    // ========================================================================
    // ALU Operations
    // ========================================================================

    fn adc(&mut self, value: u8) {
        let a = self.a as u16;
        let v = value as u16;
        let c = if self.status.carry { 1u16 } else { 0 };
        let result = a + v + c;

        self.status.carry = result > 0xFF;
        self.status.overflow = ((a ^ result) & (v ^ result) & 0x80) != 0;
        self.a = result as u8;
        self.set_zn(self.a);
    }

    fn sbc(&mut self, value: u8) {
        self.adc(!value);
    }

    fn compare(&mut self, reg: u8, value: u8) {
        let result = reg.wrapping_sub(value);
        self.status.carry = reg >= value;
        self.set_zn(result);
    }

    // ========================================================================
    // Instruction Implementations
    // ========================================================================

    // ORA
    fn ora_immediate(&mut self) { let v = self.read_immediate(); self.a |= v; self.set_zn(self.a); }
    fn ora_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.a |= v; self.set_zn(self.a); }
    fn ora_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.a |= v; self.set_zn(self.a); }
    fn ora_absolute(&mut self) { let (_, v) = self.read_absolute(); self.a |= v; self.set_zn(self.a); }
    fn ora_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.a |= v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn ora_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.a |= v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn ora_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.a |= v; self.set_zn(self.a); }
    fn ora_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.a |= v; self.set_zn(self.a); if c { 6 } else { 5 } }

    // AND
    fn and_immediate(&mut self) { let v = self.read_immediate(); self.a &= v; self.set_zn(self.a); }
    fn and_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.a &= v; self.set_zn(self.a); }
    fn and_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.a &= v; self.set_zn(self.a); }
    fn and_absolute(&mut self) { let (_, v) = self.read_absolute(); self.a &= v; self.set_zn(self.a); }
    fn and_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.a &= v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn and_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.a &= v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn and_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.a &= v; self.set_zn(self.a); }
    fn and_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.a &= v; self.set_zn(self.a); if c { 6 } else { 5 } }

    // EOR
    fn eor_immediate(&mut self) { let v = self.read_immediate(); self.a ^= v; self.set_zn(self.a); }
    fn eor_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.a ^= v; self.set_zn(self.a); }
    fn eor_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.a ^= v; self.set_zn(self.a); }
    fn eor_absolute(&mut self) { let (_, v) = self.read_absolute(); self.a ^= v; self.set_zn(self.a); }
    fn eor_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.a ^= v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn eor_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.a ^= v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn eor_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.a ^= v; self.set_zn(self.a); }
    fn eor_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.a ^= v; self.set_zn(self.a); if c { 6 } else { 5 } }

    // ADC
    fn adc_immediate(&mut self) { let v = self.read_immediate(); self.adc(v); }
    fn adc_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.adc(v); }
    fn adc_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.adc(v); }
    fn adc_absolute(&mut self) { let (_, v) = self.read_absolute(); self.adc(v); }
    fn adc_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.adc(v); if c { 5 } else { 4 } }
    fn adc_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.adc(v); if c { 5 } else { 4 } }
    fn adc_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.adc(v); }
    fn adc_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.adc(v); if c { 6 } else { 5 } }

    // SBC
    fn sbc_immediate(&mut self) { let v = self.read_immediate(); self.sbc(v); }
    fn sbc_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.sbc(v); }
    fn sbc_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.sbc(v); }
    fn sbc_absolute(&mut self) { let (_, v) = self.read_absolute(); self.sbc(v); }
    fn sbc_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.sbc(v); if c { 5 } else { 4 } }
    fn sbc_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.sbc(v); if c { 5 } else { 4 } }
    fn sbc_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.sbc(v); }
    fn sbc_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.sbc(v); if c { 6 } else { 5 } }

    // CMP
    fn cmp_immediate(&mut self) { let v = self.read_immediate(); self.compare(self.a, v); }
    fn cmp_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.compare(self.a, v); }
    fn cmp_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.compare(self.a, v); }
    fn cmp_absolute(&mut self) { let (_, v) = self.read_absolute(); self.compare(self.a, v); }
    fn cmp_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.compare(self.a, v); if c { 5 } else { 4 } }
    fn cmp_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.compare(self.a, v); if c { 5 } else { 4 } }
    fn cmp_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.compare(self.a, v); }
    fn cmp_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.compare(self.a, v); if c { 6 } else { 5 } }

    // CPX
    fn cpx_immediate(&mut self) { let v = self.read_immediate(); self.compare(self.x, v); }
    fn cpx_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.compare(self.x, v); }
    fn cpx_absolute(&mut self) { let (_, v) = self.read_absolute(); self.compare(self.x, v); }

    // CPY
    fn cpy_immediate(&mut self) { let v = self.read_immediate(); self.compare(self.y, v); }
    fn cpy_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.compare(self.y, v); }
    fn cpy_absolute(&mut self) { let (_, v) = self.read_absolute(); self.compare(self.y, v); }

    // LDA
    fn lda_immediate(&mut self) { self.a = self.read_immediate(); self.set_zn(self.a); }
    fn lda_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.a = v; self.set_zn(self.a); }
    fn lda_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.a = v; self.set_zn(self.a); }
    fn lda_absolute(&mut self) { let (_, v) = self.read_absolute(); self.a = v; self.set_zn(self.a); }
    fn lda_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.a = v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn lda_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.a = v; self.set_zn(self.a); if c { 5 } else { 4 } }
    fn lda_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.a = v; self.set_zn(self.a); }
    fn lda_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.a = v; self.set_zn(self.a); if c { 6 } else { 5 } }

    // LDX
    fn ldx_immediate(&mut self) { self.x = self.read_immediate(); self.set_zn(self.x); }
    fn ldx_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.x = v; self.set_zn(self.x); }
    fn ldx_zero_page_y(&mut self) { let (_, v) = self.read_zero_page_y(); self.x = v; self.set_zn(self.x); }
    fn ldx_absolute(&mut self) { let (_, v) = self.read_absolute(); self.x = v; self.set_zn(self.x); }
    fn ldx_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.x = v; self.set_zn(self.x); if c { 5 } else { 4 } }

    // LDY
    fn ldy_immediate(&mut self) { self.y = self.read_immediate(); self.set_zn(self.y); }
    fn ldy_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.y = v; self.set_zn(self.y); }
    fn ldy_zero_page_x(&mut self) { let (_, v) = self.read_zero_page_x(); self.y = v; self.set_zn(self.y); }
    fn ldy_absolute(&mut self) { let (_, v) = self.read_absolute(); self.y = v; self.set_zn(self.y); }
    fn ldy_absolute_x(&mut self) -> u32 { let (_, v, c) = self.read_absolute_x(); self.y = v; self.set_zn(self.y); if c { 5 } else { 4 } }

    // STA
    fn sta_zero_page(&mut self) { let addr = self.bus.read(self.pc) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.a); }
    fn sta_zero_page_x(&mut self) { let addr = self.bus.read(self.pc).wrapping_add(self.x) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.a); }
    fn sta_absolute(&mut self) { let addr = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); self.bus.write(addr, self.a); }
    fn sta_absolute_x(&mut self) { let addr = self.read16(self.pc).wrapping_add(self.x as u16); self.pc = self.pc.wrapping_add(2); self.bus.write(addr, self.a); }
    fn sta_absolute_y(&mut self) { let addr = self.read16(self.pc).wrapping_add(self.y as u16); self.pc = self.pc.wrapping_add(2); self.bus.write(addr, self.a); }
    fn sta_indexed_indirect(&mut self) { let ptr = self.bus.read(self.pc).wrapping_add(self.x); self.pc = self.pc.wrapping_add(1); let lo = self.bus.read(ptr as u16) as u16; let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16; self.bus.write((hi << 8) | lo, self.a); }
    fn sta_indirect_indexed(&mut self) { let ptr = self.bus.read(self.pc); self.pc = self.pc.wrapping_add(1); let lo = self.bus.read(ptr as u16) as u16; let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16; let addr = ((hi << 8) | lo).wrapping_add(self.y as u16); self.bus.write(addr, self.a); }

    // STX
    fn stx_zero_page(&mut self) { let addr = self.bus.read(self.pc) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.x); }
    fn stx_zero_page_y(&mut self) { let addr = self.bus.read(self.pc).wrapping_add(self.y) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.x); }
    fn stx_absolute(&mut self) { let addr = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); self.bus.write(addr, self.x); }

    // STY
    fn sty_zero_page(&mut self) { let addr = self.bus.read(self.pc) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.y); }
    fn sty_zero_page_x(&mut self) { let addr = self.bus.read(self.pc).wrapping_add(self.x) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.y); }
    fn sty_absolute(&mut self) { let addr = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); self.bus.write(addr, self.y); }

    // ASL
    fn asl_accumulator(&mut self) { self.status.carry = self.a & 0x80 != 0; self.a <<= 1; self.set_zn(self.a); }
    fn asl_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.set_zn(r); }
    fn asl_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.set_zn(r); }
    fn asl_absolute(&mut self) { let (addr, v) = self.read_absolute(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.set_zn(r); }
    fn asl_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.set_zn(r); }

    // LSR
    fn lsr_accumulator(&mut self) { self.status.carry = self.a & 0x01 != 0; self.a >>= 1; self.set_zn(self.a); }
    fn lsr_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.set_zn(r); }
    fn lsr_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.set_zn(r); }
    fn lsr_absolute(&mut self) { let (addr, v) = self.read_absolute(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.set_zn(r); }
    fn lsr_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.set_zn(r); }

    // ROL
    fn rol_accumulator(&mut self) { let c = if self.status.carry { 1 } else { 0 }; self.status.carry = self.a & 0x80 != 0; self.a = (self.a << 1) | c; self.set_zn(self.a); }
    fn rol_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.set_zn(r); }
    fn rol_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.set_zn(r); }
    fn rol_absolute(&mut self) { let (addr, v) = self.read_absolute(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.set_zn(r); }
    fn rol_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.set_zn(r); }

    // ROR
    fn ror_accumulator(&mut self) { let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = self.a & 0x01 != 0; self.a = (self.a >> 1) | c; self.set_zn(self.a); }
    fn ror_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.set_zn(r); }
    fn ror_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.set_zn(r); }
    fn ror_absolute(&mut self) { let (addr, v) = self.read_absolute(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.set_zn(r); }
    fn ror_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.set_zn(r); }

    // INC
    fn inc_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.set_zn(r); }
    fn inc_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.set_zn(r); }
    fn inc_absolute(&mut self) { let (addr, v) = self.read_absolute(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.set_zn(r); }
    fn inc_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let r = v.wrapping_add(1); self.bus.write(addr, r); self.set_zn(r); }

    // DEC
    fn dec_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.set_zn(r); }
    fn dec_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.set_zn(r); }
    fn dec_absolute(&mut self) { let (addr, v) = self.read_absolute(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.set_zn(r); }
    fn dec_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.set_zn(r); }

    // BIT
    fn bit_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.status.zero = (self.a & v) == 0; self.status.overflow = v & 0x40 != 0; self.status.negative = v & 0x80 != 0; }
    fn bit_absolute(&mut self) { let (_, v) = self.read_absolute(); self.status.zero = (self.a & v) == 0; self.status.overflow = v & 0x40 != 0; self.status.negative = v & 0x80 != 0; }

    // ========================================================================
    // Unofficial opcodes
    // ========================================================================

    // LAX - LDA + LDX
    fn lax_zero_page(&mut self) { let (_, v) = self.read_zero_page(); self.a = v; self.x = v; self.set_zn(v); }
    fn lax_zero_page_y(&mut self) { let (_, v) = self.read_zero_page_y(); self.a = v; self.x = v; self.set_zn(v); }
    fn lax_absolute(&mut self) { let (_, v) = self.read_absolute(); self.a = v; self.x = v; self.set_zn(v); }
    fn lax_absolute_y(&mut self) -> u32 { let (_, v, c) = self.read_absolute_y(); self.a = v; self.x = v; self.set_zn(v); if c { 5 } else { 4 } }
    fn lax_indexed_indirect(&mut self) { let (_, v) = self.read_indexed_indirect(); self.a = v; self.x = v; self.set_zn(v); }
    fn lax_indirect_indexed(&mut self) -> u32 { let (_, v, c) = self.read_indirect_indexed(); self.a = v; self.x = v; self.set_zn(v); if c { 6 } else { 5 } }

    // SAX - Store A & X
    fn sax_zero_page(&mut self) { let addr = self.bus.read(self.pc) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.a & self.x); }
    fn sax_zero_page_y(&mut self) { let addr = self.bus.read(self.pc).wrapping_add(self.y) as u16; self.pc = self.pc.wrapping_add(1); self.bus.write(addr, self.a & self.x); }
    fn sax_absolute(&mut self) { let addr = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); self.bus.write(addr, self.a & self.x); }
    fn sax_indexed_indirect(&mut self) { let ptr = self.bus.read(self.pc).wrapping_add(self.x); self.pc = self.pc.wrapping_add(1); let lo = self.bus.read(ptr as u16) as u16; let hi = self.bus.read(ptr.wrapping_add(1) as u16) as u16; self.bus.write((hi << 8) | lo, self.a & self.x); }

    // DCP - DEC + CMP
    fn dcp_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.compare(self.a, r); }
    fn dcp_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.compare(self.a, r); }
    fn dcp_absolute(&mut self) { let (addr, v) = self.read_absolute(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.compare(self.a, r); }
    fn dcp_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.compare(self.a, r); }
    fn dcp_absolute_y(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.y as u16); let v = self.bus.read(addr); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.compare(self.a, r); }
    fn dcp_indexed_indirect(&mut self) { let (addr, v) = self.read_indexed_indirect(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.compare(self.a, r); }
    fn dcp_indirect_indexed(&mut self) { let (addr, v, _) = self.read_indirect_indexed(); let r = v.wrapping_sub(1); self.bus.write(addr, r); self.compare(self.a, r); }

    // ISB/ISC - INC + SBC
    fn isb_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.sbc(r); }
    fn isb_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.sbc(r); }
    fn isb_absolute(&mut self) { let (addr, v) = self.read_absolute(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.sbc(r); }
    fn isb_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let r = v.wrapping_add(1); self.bus.write(addr, r); self.sbc(r); }
    fn isb_absolute_y(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.y as u16); let v = self.bus.read(addr); let r = v.wrapping_add(1); self.bus.write(addr, r); self.sbc(r); }
    fn isb_indexed_indirect(&mut self) { let (addr, v) = self.read_indexed_indirect(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.sbc(r); }
    fn isb_indirect_indexed(&mut self) { let (addr, v, _) = self.read_indirect_indexed(); let r = v.wrapping_add(1); self.bus.write(addr, r); self.sbc(r); }

    // SLO - ASL + ORA
    fn slo_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.a |= r; self.set_zn(self.a); }
    fn slo_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.a |= r; self.set_zn(self.a); }
    fn slo_absolute(&mut self) { let (addr, v) = self.read_absolute(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.a |= r; self.set_zn(self.a); }
    fn slo_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.a |= r; self.set_zn(self.a); }
    fn slo_absolute_y(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.y as u16); let v = self.bus.read(addr); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.a |= r; self.set_zn(self.a); }
    fn slo_indexed_indirect(&mut self) { let (addr, v) = self.read_indexed_indirect(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.a |= r; self.set_zn(self.a); }
    fn slo_indirect_indexed(&mut self) { let (addr, v, _) = self.read_indirect_indexed(); self.status.carry = v & 0x80 != 0; let r = v << 1; self.bus.write(addr, r); self.a |= r; self.set_zn(self.a); }

    // RLA - ROL + AND
    fn rla_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.a &= r; self.set_zn(self.a); }
    fn rla_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.a &= r; self.set_zn(self.a); }
    fn rla_absolute(&mut self) { let (addr, v) = self.read_absolute(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.a &= r; self.set_zn(self.a); }
    fn rla_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.a &= r; self.set_zn(self.a); }
    fn rla_absolute_y(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.y as u16); let v = self.bus.read(addr); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.a &= r; self.set_zn(self.a); }
    fn rla_indexed_indirect(&mut self) { let (addr, v) = self.read_indexed_indirect(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.a &= r; self.set_zn(self.a); }
    fn rla_indirect_indexed(&mut self) { let (addr, v, _) = self.read_indirect_indexed(); let c = if self.status.carry { 1 } else { 0 }; self.status.carry = v & 0x80 != 0; let r = (v << 1) | c; self.bus.write(addr, r); self.a &= r; self.set_zn(self.a); }

    // SRE - LSR + EOR
    fn sre_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.a ^= r; self.set_zn(self.a); }
    fn sre_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.a ^= r; self.set_zn(self.a); }
    fn sre_absolute(&mut self) { let (addr, v) = self.read_absolute(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.a ^= r; self.set_zn(self.a); }
    fn sre_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.a ^= r; self.set_zn(self.a); }
    fn sre_absolute_y(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.y as u16); let v = self.bus.read(addr); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.a ^= r; self.set_zn(self.a); }
    fn sre_indexed_indirect(&mut self) { let (addr, v) = self.read_indexed_indirect(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.a ^= r; self.set_zn(self.a); }
    fn sre_indirect_indexed(&mut self) { let (addr, v, _) = self.read_indirect_indexed(); self.status.carry = v & 0x01 != 0; let r = v >> 1; self.bus.write(addr, r); self.a ^= r; self.set_zn(self.a); }

    // RRA - ROR + ADC
    fn rra_zero_page(&mut self) { let (addr, v) = self.read_zero_page(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.adc(r); }
    fn rra_zero_page_x(&mut self) { let (addr, v) = self.read_zero_page_x(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.adc(r); }
    fn rra_absolute(&mut self) { let (addr, v) = self.read_absolute(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.adc(r); }
    fn rra_absolute_x(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.x as u16); let v = self.bus.read(addr); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.adc(r); }
    fn rra_absolute_y(&mut self) { let base = self.read16(self.pc); self.pc = self.pc.wrapping_add(2); let addr = base.wrapping_add(self.y as u16); let v = self.bus.read(addr); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.adc(r); }
    fn rra_indexed_indirect(&mut self) { let (addr, v) = self.read_indexed_indirect(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.adc(r); }
    fn rra_indirect_indexed(&mut self) { let (addr, v, _) = self.read_indirect_indexed(); let c = if self.status.carry { 0x80 } else { 0 }; self.status.carry = v & 0x01 != 0; let r = (v >> 1) | c; self.bus.write(addr, r); self.adc(r); }
}

impl<B: Bus6502> Cpu for Mos6502<B> {
    fn step(&mut self) -> u32 {
        self.execute()
    }

    fn reset(&mut self) {
        self.a = 0;
        self.x = 0;
        self.y = 0;
        self.sp = 0xFD;
        self.status = StatusFlags {
            carry: false,
            zero: false,
            interrupt_disable: true,
            decimal: false,
            break_flag: false,
            overflow: false,
            negative: false,
        };
        self.pc = self.read16(0xFFFC);
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

    impl Bus6502 for TestBus {
        fn read(&mut self, addr: u16) -> u8 {
            self.ram[addr as usize]
        }

        fn write(&mut self, addr: u16, data: u8) {
            self.ram[addr as usize] = data;
        }
    }

    #[test]
    fn test_lda_immediate() {
        let mut bus = TestBus::new();
        bus.ram[0xFFFC] = 0x00;
        bus.ram[0xFFFD] = 0x80;
        bus.ram[0x8000] = 0xA9; // LDA #$42
        bus.ram[0x8001] = 0x42;

        let mut cpu = Mos6502::new(bus);
        cpu.reset();
        cpu.step();

        assert_eq!(cpu.a, 0x42);
        assert!(!cpu.status.zero);
        assert!(!cpu.status.negative);
    }

    #[test]
    fn test_adc() {
        let mut bus = TestBus::new();
        bus.ram[0xFFFC] = 0x00;
        bus.ram[0xFFFD] = 0x80;
        bus.ram[0x8000] = 0xA9; // LDA #$50
        bus.ram[0x8001] = 0x50;
        bus.ram[0x8002] = 0x69; // ADC #$50
        bus.ram[0x8003] = 0x50;

        let mut cpu = Mos6502::new(bus);
        cpu.reset();
        cpu.step();
        cpu.step();

        assert_eq!(cpu.a, 0xA0);
        assert!(cpu.status.negative);
        assert!(cpu.status.overflow);
    }
}
