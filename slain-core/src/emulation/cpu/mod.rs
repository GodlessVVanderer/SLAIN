//! CPU emulation cores
//!
//! Implements various CPU architectures used in retro gaming consoles.

pub mod mos6502;
pub mod z80;
pub mod sh4;

/// Common CPU trait
pub trait Cpu {
    /// Execute one instruction and return cycles consumed
    fn step(&mut self) -> u32;

    /// Reset the CPU to initial state
    fn reset(&mut self);

    /// Trigger an interrupt (IRQ)
    fn irq(&mut self);

    /// Trigger a non-maskable interrupt (NMI)
    fn nmi(&mut self);

    /// Get the program counter
    fn pc(&self) -> u16;

    /// Set the program counter
    fn set_pc(&mut self, pc: u16);

    /// Get total cycles executed
    fn cycles(&self) -> u64;
}

/// CPU status flags (common across architectures)
#[derive(Debug, Clone, Copy, Default)]
pub struct StatusFlags {
    pub carry: bool,
    pub zero: bool,
    pub interrupt_disable: bool,
    pub decimal: bool,
    pub break_flag: bool,
    pub overflow: bool,
    pub negative: bool,
}
