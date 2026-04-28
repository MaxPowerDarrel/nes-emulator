/// 6502 addressing mode resolution.
///
/// Spec: http://www.6502.org/tutorials/6502opcodes.html
///
/// Each mode returns the effective address (if any) and whether a page boundary
/// was crossed (which costs +1 cycle for read instructions).

use super::Cpu;
use crate::bus::Bus;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AddrMode {
    Implied,
    Accumulator,
    Immediate,
    ZeroPage,
    ZeroPageX,
    ZeroPageY,
    Absolute,
    AbsoluteX,
    AbsoluteY,
    Indirect,
    IndirectX, // (Ind,X) — indexed indirect
    IndirectY, // (Ind),Y — indirect indexed
    Relative,
}

/// Result of resolving an addressing mode.
pub struct Operand {
    /// Effective address. For Implied/Accumulator, 0 and unused.
    pub addr: u16,
    /// True when a page boundary was crossed (may add a cycle).
    pub page_crossed: bool,
}

impl Cpu {
    /// Resolve the addressing mode and advance PC past the operand bytes.
    /// Returns an `Operand`. For Implied/Accumulator, addr is 0 and unused.
    pub fn resolve_operand(&mut self, bus: &mut Bus, mode: AddrMode) -> Operand {
        match mode {
            AddrMode::Implied | AddrMode::Accumulator => Operand {
                addr: 0,
                page_crossed: false,
            },

            AddrMode::Immediate => {
                let addr = self.pc;
                self.pc = self.pc.wrapping_add(1);
                Operand { addr, page_crossed: false }
            }

            AddrMode::ZeroPage => {
                let addr = bus.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                Operand { addr, page_crossed: false }
            }

            AddrMode::ZeroPageX => {
                let base = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let addr = base.wrapping_add(self.x) as u16;
                Operand { addr, page_crossed: false }
            }

            AddrMode::ZeroPageY => {
                let base = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let addr = base.wrapping_add(self.y) as u16;
                Operand { addr, page_crossed: false }
            }

            AddrMode::Absolute => {
                let addr = bus.read_u16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                Operand { addr, page_crossed: false }
            }

            AddrMode::AbsoluteX => {
                let base = bus.read_u16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                let addr = base.wrapping_add(self.x as u16);
                let page_crossed = (base & 0xFF00) != (addr & 0xFF00);
                Operand { addr, page_crossed }
            }

            AddrMode::AbsoluteY => {
                let base = bus.read_u16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                let addr = base.wrapping_add(self.y as u16);
                let page_crossed = (base & 0xFF00) != (addr & 0xFF00);
                Operand { addr, page_crossed }
            }

            AddrMode::Indirect => {
                let ptr = bus.read_u16(self.pc);
                self.pc = self.pc.wrapping_add(2);
                // Hardware bug: wraps within page if ptr low byte is $FF
                let addr = bus.read_u16_page_wrap(ptr);
                Operand { addr, page_crossed: false }
            }

            AddrMode::IndirectX => {
                let base = bus.read(self.pc);
                self.pc = self.pc.wrapping_add(1);
                let ptr = base.wrapping_add(self.x) as u16;
                let addr = bus.read_u16_page_wrap(ptr);
                Operand { addr, page_crossed: false }
            }

            AddrMode::IndirectY => {
                let ptr = bus.read(self.pc) as u16;
                self.pc = self.pc.wrapping_add(1);
                let base = bus.read_u16_page_wrap(ptr);
                let addr = base.wrapping_add(self.y as u16);
                let page_crossed = (base & 0xFF00) != (addr & 0xFF00);
                Operand { addr, page_crossed }
            }

            AddrMode::Relative => {
                let offset = bus.read(self.pc) as i8;
                self.pc = self.pc.wrapping_add(1);
                // Effective address computed at branch time, not here.
                // We store the raw offset in addr (as u16 two's complement).
                Operand {
                    addr: offset as i16 as u16,
                    page_crossed: false,
                }
            }
        }
    }
}