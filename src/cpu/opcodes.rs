/// 6502 opcode dispatch — official and unofficial (illegal) opcodes.
///
/// Spec references:
///   http://www.6502.org/tutorials/6502opcodes.html  (opcodes + cycle counts)
///   https://www.nesdev.org/wiki/CPU_unofficial_opcodes  (illegal opcode behavior)
///
/// Each decode entry is: (mnemonic, addressing mode, base cycle count, page_cycle, unofficial)

use super::{Cpu, Flag};
use crate::bus::Bus;
use crate::cpu::addressing::AddrMode;

/// Decoded instruction metadata.
pub struct Instr {
    pub name: &'static str,
    pub mode: AddrMode,
    pub cycles: u8,
    /// Whether a page-cross on a read adds 1 cycle.
    pub page_cycle: bool,
    /// True for unofficial/illegal opcodes — disassembly prefixes mnemonic with '*'.
    pub unofficial: bool,
}

/// Decode an opcode byte into instruction metadata.
/// Returns None for unimplemented opcodes.
pub fn decode(opcode: u8) -> Option<Instr> {
    use AddrMode::*;
    let (name, mode, cycles, page_cycle, unofficial) = match opcode {
        // ── LDA ──────────────────────────────────────────────────────────────
        0xA9 => ("LDA", Immediate,  2, false, false),
        0xA5 => ("LDA", ZeroPage,   3, false, false),
        0xB5 => ("LDA", ZeroPageX,  4, false, false),
        0xAD => ("LDA", Absolute,   4, false, false),
        0xBD => ("LDA", AbsoluteX,  4, true,  false),
        0xB9 => ("LDA", AbsoluteY,  4, true,  false),
        0xA1 => ("LDA", IndirectX,  6, false, false),
        0xB1 => ("LDA", IndirectY,  5, true,  false),
        // ── LDX ──────────────────────────────────────────────────────────────
        0xA2 => ("LDX", Immediate,  2, false, false),
        0xA6 => ("LDX", ZeroPage,   3, false, false),
        0xB6 => ("LDX", ZeroPageY,  4, false, false),
        0xAE => ("LDX", Absolute,   4, false, false),
        0xBE => ("LDX", AbsoluteY,  4, true,  false),
        // ── LDY ──────────────────────────────────────────────────────────────
        0xA0 => ("LDY", Immediate,  2, false, false),
        0xA4 => ("LDY", ZeroPage,   3, false, false),
        0xB4 => ("LDY", ZeroPageX,  4, false, false),
        0xAC => ("LDY", Absolute,   4, false, false),
        0xBC => ("LDY", AbsoluteX,  4, true,  false),
        // ── STA ──────────────────────────────────────────────────────────────
        0x85 => ("STA", ZeroPage,   3, false, false),
        0x95 => ("STA", ZeroPageX,  4, false, false),
        0x8D => ("STA", Absolute,   4, false, false),
        0x9D => ("STA", AbsoluteX,  5, false, false),
        0x99 => ("STA", AbsoluteY,  5, false, false),
        0x81 => ("STA", IndirectX,  6, false, false),
        0x91 => ("STA", IndirectY,  6, false, false),
        // ── STX ──────────────────────────────────────────────────────────────
        0x86 => ("STX", ZeroPage,   3, false, false),
        0x96 => ("STX", ZeroPageY,  4, false, false),
        0x8E => ("STX", Absolute,   4, false, false),
        // ── STY ──────────────────────────────────────────────────────────────
        0x84 => ("STY", ZeroPage,   3, false, false),
        0x94 => ("STY", ZeroPageX,  4, false, false),
        0x8C => ("STY", Absolute,   4, false, false),
        // ── Transfers ────────────────────────────────────────────────────────
        0xAA => ("TAX", Implied,    2, false, false),
        0xA8 => ("TAY", Implied,    2, false, false),
        0x8A => ("TXA", Implied,    2, false, false),
        0x98 => ("TYA", Implied,    2, false, false),
        0xBA => ("TSX", Implied,    2, false, false),
        0x9A => ("TXS", Implied,    2, false, false),
        // ── Stack ─────────────────────────────────────────────────────────────
        0x48 => ("PHA", Implied,    3, false, false),
        0x08 => ("PHP", Implied,    3, false, false),
        0x68 => ("PLA", Implied,    4, false, false),
        0x28 => ("PLP", Implied,    4, false, false),
        // ── ADC ──────────────────────────────────────────────────────────────
        0x69 => ("ADC", Immediate,  2, false, false),
        0x65 => ("ADC", ZeroPage,   3, false, false),
        0x75 => ("ADC", ZeroPageX,  4, false, false),
        0x6D => ("ADC", Absolute,   4, false, false),
        0x7D => ("ADC", AbsoluteX,  4, true,  false),
        0x79 => ("ADC", AbsoluteY,  4, true,  false),
        0x61 => ("ADC", IndirectX,  6, false, false),
        0x71 => ("ADC", IndirectY,  5, true,  false),
        // ── SBC ──────────────────────────────────────────────────────────────
        0xE9 => ("SBC", Immediate,  2, false, false),
        0xE5 => ("SBC", ZeroPage,   3, false, false),
        0xF5 => ("SBC", ZeroPageX,  4, false, false),
        0xED => ("SBC", Absolute,   4, false, false),
        0xFD => ("SBC", AbsoluteX,  4, true,  false),
        0xF9 => ("SBC", AbsoluteY,  4, true,  false),
        0xE1 => ("SBC", IndirectX,  6, false, false),
        0xF1 => ("SBC", IndirectY,  5, true,  false),
        // ── INC ──────────────────────────────────────────────────────────────
        0xE6 => ("INC", ZeroPage,   5, false, false),
        0xF6 => ("INC", ZeroPageX,  6, false, false),
        0xEE => ("INC", Absolute,   6, false, false),
        0xFE => ("INC", AbsoluteX,  7, false, false),
        // ── INX / INY ────────────────────────────────────────────────────────
        0xE8 => ("INX", Implied,    2, false, false),
        0xC8 => ("INY", Implied,    2, false, false),
        // ── DEC ──────────────────────────────────────────────────────────────
        0xC6 => ("DEC", ZeroPage,   5, false, false),
        0xD6 => ("DEC", ZeroPageX,  6, false, false),
        0xCE => ("DEC", Absolute,   6, false, false),
        0xDE => ("DEC", AbsoluteX,  7, false, false),
        // ── DEX / DEY ────────────────────────────────────────────────────────
        0xCA => ("DEX", Implied,    2, false, false),
        0x88 => ("DEY", Implied,    2, false, false),
        // ── AND ──────────────────────────────────────────────────────────────
        0x29 => ("AND", Immediate,  2, false, false),
        0x25 => ("AND", ZeroPage,   3, false, false),
        0x35 => ("AND", ZeroPageX,  4, false, false),
        0x2D => ("AND", Absolute,   4, false, false),
        0x3D => ("AND", AbsoluteX,  4, true,  false),
        0x39 => ("AND", AbsoluteY,  4, true,  false),
        0x21 => ("AND", IndirectX,  6, false, false),
        0x31 => ("AND", IndirectY,  5, true,  false),
        // ── ORA ──────────────────────────────────────────────────────────────
        0x09 => ("ORA", Immediate,  2, false, false),
        0x05 => ("ORA", ZeroPage,   3, false, false),
        0x15 => ("ORA", ZeroPageX,  4, false, false),
        0x0D => ("ORA", Absolute,   4, false, false),
        0x1D => ("ORA", AbsoluteX,  4, true,  false),
        0x19 => ("ORA", AbsoluteY,  4, true,  false),
        0x01 => ("ORA", IndirectX,  6, false, false),
        0x11 => ("ORA", IndirectY,  5, true,  false),
        // ── EOR ──────────────────────────────────────────────────────────────
        0x49 => ("EOR", Immediate,  2, false, false),
        0x45 => ("EOR", ZeroPage,   3, false, false),
        0x55 => ("EOR", ZeroPageX,  4, false, false),
        0x4D => ("EOR", Absolute,   4, false, false),
        0x5D => ("EOR", AbsoluteX,  4, true,  false),
        0x59 => ("EOR", AbsoluteY,  4, true,  false),
        0x41 => ("EOR", IndirectX,  6, false, false),
        0x51 => ("EOR", IndirectY,  5, true,  false),
        // ── ASL ──────────────────────────────────────────────────────────────
        0x0A => ("ASL", Accumulator, 2, false, false),
        0x06 => ("ASL", ZeroPage,    5, false, false),
        0x16 => ("ASL", ZeroPageX,   6, false, false),
        0x0E => ("ASL", Absolute,    6, false, false),
        0x1E => ("ASL", AbsoluteX,   7, false, false),
        // ── LSR ──────────────────────────────────────────────────────────────
        0x4A => ("LSR", Accumulator, 2, false, false),
        0x46 => ("LSR", ZeroPage,    5, false, false),
        0x56 => ("LSR", ZeroPageX,   6, false, false),
        0x4E => ("LSR", Absolute,    6, false, false),
        0x5E => ("LSR", AbsoluteX,   7, false, false),
        // ── ROL ──────────────────────────────────────────────────────────────
        0x2A => ("ROL", Accumulator, 2, false, false),
        0x26 => ("ROL", ZeroPage,    5, false, false),
        0x36 => ("ROL", ZeroPageX,   6, false, false),
        0x2E => ("ROL", Absolute,    6, false, false),
        0x3E => ("ROL", AbsoluteX,   7, false, false),
        // ── ROR ──────────────────────────────────────────────────────────────
        0x6A => ("ROR", Accumulator, 2, false, false),
        0x66 => ("ROR", ZeroPage,    5, false, false),
        0x76 => ("ROR", ZeroPageX,   6, false, false),
        0x6E => ("ROR", Absolute,    6, false, false),
        0x7E => ("ROR", AbsoluteX,   7, false, false),
        // ── CMP ──────────────────────────────────────────────────────────────
        0xC9 => ("CMP", Immediate,  2, false, false),
        0xC5 => ("CMP", ZeroPage,   3, false, false),
        0xD5 => ("CMP", ZeroPageX,  4, false, false),
        0xCD => ("CMP", Absolute,   4, false, false),
        0xDD => ("CMP", AbsoluteX,  4, true,  false),
        0xD9 => ("CMP", AbsoluteY,  4, true,  false),
        0xC1 => ("CMP", IndirectX,  6, false, false),
        0xD1 => ("CMP", IndirectY,  5, true,  false),
        // ── CPX ──────────────────────────────────────────────────────────────
        0xE0 => ("CPX", Immediate,  2, false, false),
        0xE4 => ("CPX", ZeroPage,   3, false, false),
        0xEC => ("CPX", Absolute,   4, false, false),
        // ── CPY ──────────────────────────────────────────────────────────────
        0xC0 => ("CPY", Immediate,  2, false, false),
        0xC4 => ("CPY", ZeroPage,   3, false, false),
        0xCC => ("CPY", Absolute,   4, false, false),
        // ── BIT ──────────────────────────────────────────────────────────────
        0x24 => ("BIT", ZeroPage,   3, false, false),
        0x2C => ("BIT", Absolute,   4, false, false),
        // ── Branches ─────────────────────────────────────────────────────────
        0x90 => ("BCC", Relative,   2, false, false),
        0xB0 => ("BCS", Relative,   2, false, false),
        0xF0 => ("BEQ", Relative,   2, false, false),
        0xD0 => ("BNE", Relative,   2, false, false),
        0x30 => ("BMI", Relative,   2, false, false),
        0x10 => ("BPL", Relative,   2, false, false),
        0x50 => ("BVC", Relative,   2, false, false),
        0x70 => ("BVS", Relative,   2, false, false),
        // ── Jumps ─────────────────────────────────────────────────────────────
        0x4C => ("JMP", Absolute,   3, false, false),
        0x6C => ("JMP", Indirect,   5, false, false),
        0x20 => ("JSR", Absolute,   6, false, false),
        0x60 => ("RTS", Implied,    6, false, false),
        0x40 => ("RTI", Implied,    6, false, false),
        // ── BRK ──────────────────────────────────────────────────────────────
        0x00 => ("BRK", Implied,    7, false, false),
        // ── Flag instructions ─────────────────────────────────────────────────
        0x18 => ("CLC", Implied,    2, false, false),
        0x38 => ("SEC", Implied,    2, false, false),
        0xD8 => ("CLD", Implied,    2, false, false),
        0xF8 => ("SED", Implied,    2, false, false),
        0x58 => ("CLI", Implied,    2, false, false),
        0x78 => ("SEI", Implied,    2, false, false),
        0xB8 => ("CLV", Implied,    2, false, false),
        // ── NOP (official) ───────────────────────────────────────────────────
        0xEA => ("NOP", Implied,    2, false, false),

        // ════════════════════════════════════════════════════════════════════
        // Unofficial / Illegal opcodes
        // Spec: https://www.nesdev.org/wiki/CPU_unofficial_opcodes
        // ════════════════════════════════════════════════════════════════════

        // ── Unofficial NOP variants ───────────────────────────────────────
        // Implied (1-byte, 2 cycles)
        0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA
            => ("NOP", Implied,    2, false, true),
        // Immediate (2-byte, 2 cycles)
        0x80 | 0x82 | 0x89 | 0xC2 | 0xE2
            => ("NOP", Immediate,  2, false, true),
        // Zero Page (2-byte, 3 cycles)
        0x04 | 0x44 | 0x64
            => ("NOP", ZeroPage,   3, false, true),
        // Zero Page,X (2-byte, 4 cycles)
        0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4
            => ("NOP", ZeroPageX,  4, false, true),
        // Absolute (3-byte, 4 cycles)
        0x0C
            => ("NOP", Absolute,   4, false, true),
        // Absolute,X (3-byte, 4 cycles + 1 page cross)
        0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC
            => ("NOP", AbsoluteX,  4, true,  true),

        // ── SLO: ASL mem, then ORA A ─────────────────────────────────────
        // Spec: no page-cross discount (write instruction)
        0x07 => ("SLO", ZeroPage,   5, false, true),
        0x17 => ("SLO", ZeroPageX,  6, false, true),
        0x0F => ("SLO", Absolute,   6, false, true),
        0x1F => ("SLO", AbsoluteX,  7, false, true),
        0x1B => ("SLO", AbsoluteY,  7, false, true),
        0x03 => ("SLO", IndirectX,  8, false, true),
        0x13 => ("SLO", IndirectY,  8, false, true),

        // ── RLA: ROL mem, then AND A ──────────────────────────────────────
        0x27 => ("RLA", ZeroPage,   5, false, true),
        0x37 => ("RLA", ZeroPageX,  6, false, true),
        0x2F => ("RLA", Absolute,   6, false, true),
        0x3F => ("RLA", AbsoluteX,  7, false, true),
        0x3B => ("RLA", AbsoluteY,  7, false, true),
        0x23 => ("RLA", IndirectX,  8, false, true),
        0x33 => ("RLA", IndirectY,  8, false, true),

        // ── SRE: LSR mem, then EOR A ──────────────────────────────────────
        0x47 => ("SRE", ZeroPage,   5, false, true),
        0x57 => ("SRE", ZeroPageX,  6, false, true),
        0x4F => ("SRE", Absolute,   6, false, true),
        0x5F => ("SRE", AbsoluteX,  7, false, true),
        0x5B => ("SRE", AbsoluteY,  7, false, true),
        0x43 => ("SRE", IndirectX,  8, false, true),
        0x53 => ("SRE", IndirectY,  8, false, true),

        // ── RRA: ROR mem, then ADC A ──────────────────────────────────────
        0x67 => ("RRA", ZeroPage,   5, false, true),
        0x77 => ("RRA", ZeroPageX,  6, false, true),
        0x6F => ("RRA", Absolute,   6, false, true),
        0x7F => ("RRA", AbsoluteX,  7, false, true),
        0x7B => ("RRA", AbsoluteY,  7, false, true),
        0x63 => ("RRA", IndirectX,  8, false, true),
        0x73 => ("RRA", IndirectY,  8, false, true),

        // ── SAX: store A AND X ────────────────────────────────────────────
        0x87 => ("SAX", ZeroPage,   3, false, true),
        0x97 => ("SAX", ZeroPageY,  4, false, true),
        0x8F => ("SAX", Absolute,   4, false, true),
        0x83 => ("SAX", IndirectX,  6, false, true),

        // ── LAX: load mem into A and X ────────────────────────────────────
        0xA7 => ("LAX", ZeroPage,   3, false, true),
        0xB7 => ("LAX", ZeroPageY,  4, false, true),
        0xAF => ("LAX", Absolute,   4, false, true),
        0xBF => ("LAX", AbsoluteY,  4, true,  true),
        0xA3 => ("LAX", IndirectX,  6, false, true),
        0xB3 => ("LAX", IndirectY,  5, true,  true),

        // ── DCP: DEC mem, then CMP A ──────────────────────────────────────
        0xC7 => ("DCP", ZeroPage,   5, false, true),
        0xD7 => ("DCP", ZeroPageX,  6, false, true),
        0xCF => ("DCP", Absolute,   6, false, true),
        0xDF => ("DCP", AbsoluteX,  7, false, true),
        0xDB => ("DCP", AbsoluteY,  7, false, true),
        0xC3 => ("DCP", IndirectX,  8, false, true),
        0xD3 => ("DCP", IndirectY,  8, false, true),

        // ── ISB (also known as ISC): INC mem, then SBC A ─────────────────
        // nestest.log uses the mnemonic "ISB"; some sources call it "ISC".
        0xE7 => ("ISB", ZeroPage,   5, false, true),
        0xF7 => ("ISB", ZeroPageX,  6, false, true),
        0xEF => ("ISB", Absolute,   6, false, true),
        0xFF => ("ISB", AbsoluteX,  7, false, true),
        0xFB => ("ISB", AbsoluteY,  7, false, true),
        0xE3 => ("ISB", IndirectX,  8, false, true),
        0xF3 => ("ISB", IndirectY,  8, false, true),

        // ── Unofficial SBC ($EB) — identical to official $E9 ──────────────
        0xEB => ("SBC", Immediate,  2, false, true),

        _ => return None,
    };
    Some(Instr { name, mode, cycles, page_cycle, unofficial })
}

/// Execute one decoded instruction. PC has already been advanced past the opcode byte.
/// Returns the number of additional cycles consumed (page cross, branch taken, etc.).
pub fn execute(cpu: &mut Cpu, bus: &mut Bus, _opcode: u8, instr: &Instr) -> u8 {
    use AddrMode::*;

    let op = cpu.resolve_operand(bus, instr.mode);
    let mut extra_cycles: u8 = 0;

    if instr.page_cycle && op.page_crossed {
        extra_cycles += 1;
    }

    match instr.name {
        // ── Load/Store ──────────────────────────────────────────────────────
        "LDA" => {
            let val = bus.read(op.addr);
            cpu.a = val;
            cpu.set_nz(val);
        }
        "LDX" => {
            let val = bus.read(op.addr);
            cpu.x = val;
            cpu.set_nz(val);
        }
        "LDY" => {
            let val = bus.read(op.addr);
            cpu.y = val;
            cpu.set_nz(val);
        }
        "STA" => { bus.write(op.addr, cpu.a); }
        "STX" => { bus.write(op.addr, cpu.x); }
        "STY" => { bus.write(op.addr, cpu.y); }

        // ── Transfers ───────────────────────────────────────────────────────
        "TAX" => { cpu.x = cpu.a; cpu.set_nz(cpu.x); }
        "TAY" => { cpu.y = cpu.a; cpu.set_nz(cpu.y); }
        "TXA" => { cpu.a = cpu.x; cpu.set_nz(cpu.a); }
        "TYA" => { cpu.a = cpu.y; cpu.set_nz(cpu.a); }
        "TSX" => { cpu.x = cpu.sp; cpu.set_nz(cpu.x); }
        "TXS" => { cpu.sp = cpu.x; }

        // ── Stack ────────────────────────────────────────────────────────────
        "PHA" => { cpu.stack_push(bus, cpu.a); }
        "PHP" => {
            // B flag and bit 5 are always set when pushing via PHP/BRK
            let p = cpu.p | Flag::B as u8 | 0x20;
            cpu.stack_push(bus, p);
        }
        "PLA" => {
            let val = cpu.stack_pull(bus);
            cpu.a = val;
            cpu.set_nz(val);
        }
        "PLP" => {
            let val = cpu.stack_pull(bus);
            // B flag (bit 4) is cleared; bit 5 always set
            cpu.p = (val & 0xCF) | 0x20;
        }

        // ── Arithmetic ───────────────────────────────────────────────────────
        "ADC" => {
            let m = bus.read(op.addr);
            let c = cpu.get_flag(Flag::C) as u16;
            let result = cpu.a as u16 + m as u16 + c;
            let a = cpu.a;
            cpu.set_flag(Flag::C, result > 0xFF);
            cpu.set_flag(Flag::V, (!(a ^ m) & (a ^ result as u8) & 0x80) != 0);
            cpu.a = result as u8;
            cpu.set_nz(cpu.a);
        }
        "SBC" => {
            // SBC A - M - (1-C)  ==  ADC with M inverted
            let m = bus.read(op.addr) ^ 0xFF;
            let c = cpu.get_flag(Flag::C) as u16;
            let result = cpu.a as u16 + m as u16 + c;
            let a = cpu.a;
            cpu.set_flag(Flag::C, result > 0xFF);
            cpu.set_flag(Flag::V, (!(a ^ m) & (a ^ result as u8) & 0x80) != 0);
            cpu.a = result as u8;
            cpu.set_nz(cpu.a);
        }

        // ── Increment / Decrement ────────────────────────────────────────────
        "INC" => {
            let val = bus.read(op.addr).wrapping_add(1);
            bus.write(op.addr, val);
            cpu.set_nz(val);
        }
        "INX" => { cpu.x = cpu.x.wrapping_add(1); cpu.set_nz(cpu.x); }
        "INY" => { cpu.y = cpu.y.wrapping_add(1); cpu.set_nz(cpu.y); }
        "DEC" => {
            let val = bus.read(op.addr).wrapping_sub(1);
            bus.write(op.addr, val);
            cpu.set_nz(val);
        }
        "DEX" => { cpu.x = cpu.x.wrapping_sub(1); cpu.set_nz(cpu.x); }
        "DEY" => { cpu.y = cpu.y.wrapping_sub(1); cpu.set_nz(cpu.y); }

        // ── Logical ──────────────────────────────────────────────────────────
        "AND" => { cpu.a &= bus.read(op.addr); cpu.set_nz(cpu.a); }
        "ORA" => { cpu.a |= bus.read(op.addr); cpu.set_nz(cpu.a); }
        "EOR" => { cpu.a ^= bus.read(op.addr); cpu.set_nz(cpu.a); }

        // ── Shifts / Rotates ─────────────────────────────────────────────────
        "ASL" => {
            if instr.mode == Accumulator {
                cpu.set_flag(Flag::C, cpu.a & 0x80 != 0);
                cpu.a <<= 1;
                cpu.set_nz(cpu.a);
            } else {
                let m = bus.read(op.addr);
                cpu.set_flag(Flag::C, m & 0x80 != 0);
                let val = m << 1;
                bus.write(op.addr, val);
                cpu.set_nz(val);
            }
        }
        "LSR" => {
            if instr.mode == Accumulator {
                cpu.set_flag(Flag::C, cpu.a & 0x01 != 0);
                cpu.a >>= 1;
                cpu.set_nz(cpu.a);
            } else {
                let m = bus.read(op.addr);
                cpu.set_flag(Flag::C, m & 0x01 != 0);
                let val = m >> 1;
                bus.write(op.addr, val);
                cpu.set_nz(val);
            }
        }
        "ROL" => {
            let c_in = cpu.get_flag(Flag::C) as u8;
            if instr.mode == Accumulator {
                cpu.set_flag(Flag::C, cpu.a & 0x80 != 0);
                cpu.a = (cpu.a << 1) | c_in;
                cpu.set_nz(cpu.a);
            } else {
                let m = bus.read(op.addr);
                cpu.set_flag(Flag::C, m & 0x80 != 0);
                let val = (m << 1) | c_in;
                bus.write(op.addr, val);
                cpu.set_nz(val);
            }
        }
        "ROR" => {
            let c_in = cpu.get_flag(Flag::C) as u8;
            if instr.mode == Accumulator {
                cpu.set_flag(Flag::C, cpu.a & 0x01 != 0);
                cpu.a = (cpu.a >> 1) | (c_in << 7);
                cpu.set_nz(cpu.a);
            } else {
                let m = bus.read(op.addr);
                cpu.set_flag(Flag::C, m & 0x01 != 0);
                let val = (m >> 1) | (c_in << 7);
                bus.write(op.addr, val);
                cpu.set_nz(val);
            }
        }

        // ── Compare ──────────────────────────────────────────────────────────
        "CMP" => { cpu.compare(bus.read(op.addr), cpu.a); }
        "CPX" => { cpu.compare(bus.read(op.addr), cpu.x); }
        "CPY" => { cpu.compare(bus.read(op.addr), cpu.y); }

        // ── Bit Test ─────────────────────────────────────────────────────────
        "BIT" => {
            let m = bus.read(op.addr);
            cpu.set_flag(Flag::N, m & 0x80 != 0);
            cpu.set_flag(Flag::V, m & 0x40 != 0);
            cpu.set_flag(Flag::Z, (cpu.a & m) == 0);
        }

        // ── Branches ─────────────────────────────────────────────────────────
        "BCC" => extra_cycles += cpu.branch(op.addr, !cpu.get_flag(Flag::C)),
        "BCS" => extra_cycles += cpu.branch(op.addr,  cpu.get_flag(Flag::C)),
        "BEQ" => extra_cycles += cpu.branch(op.addr,  cpu.get_flag(Flag::Z)),
        "BNE" => extra_cycles += cpu.branch(op.addr, !cpu.get_flag(Flag::Z)),
        "BMI" => extra_cycles += cpu.branch(op.addr,  cpu.get_flag(Flag::N)),
        "BPL" => extra_cycles += cpu.branch(op.addr, !cpu.get_flag(Flag::N)),
        "BVC" => extra_cycles += cpu.branch(op.addr, !cpu.get_flag(Flag::V)),
        "BVS" => extra_cycles += cpu.branch(op.addr,  cpu.get_flag(Flag::V)),

        // ── Jumps ─────────────────────────────────────────────────────────────
        "JMP" => { cpu.pc = op.addr; }
        "JSR" => {
            // Push PC-1 (last byte of JSR instruction), then jump.
            let ret = cpu.pc.wrapping_sub(1);
            cpu.stack_push(bus, (ret >> 8) as u8);
            cpu.stack_push(bus, ret as u8);
            cpu.pc = op.addr;
        }
        "RTS" => {
            let lo = cpu.stack_pull(bus) as u16;
            let hi = cpu.stack_pull(bus) as u16;
            cpu.pc = ((hi << 8) | lo).wrapping_add(1);
        }
        "RTI" => {
            let p = cpu.stack_pull(bus);
            cpu.p = (p & 0xCF) | 0x20; // ignore B flag, keep bit5 set
            let lo = cpu.stack_pull(bus) as u16;
            let hi = cpu.stack_pull(bus) as u16;
            cpu.pc = (hi << 8) | lo;
        }

        // ── BRK ──────────────────────────────────────────────────────────────
        "BRK" => {
            let pc = cpu.pc.wrapping_add(1); // BRK is 2 bytes; skip padding byte
            cpu.stack_push(bus, (pc >> 8) as u8);
            cpu.stack_push(bus, pc as u8);
            let p = cpu.p | Flag::B as u8 | 0x20;
            cpu.stack_push(bus, p);
            cpu.set_flag(Flag::I, true);
            cpu.pc = bus.read_u16(0xFFFE);
        }

        // ── Flag instructions ─────────────────────────────────────────────────
        "CLC" => cpu.set_flag(Flag::C, false),
        "SEC" => cpu.set_flag(Flag::C, true),
        "CLD" => cpu.set_flag(Flag::D, false),
        "SED" => cpu.set_flag(Flag::D, true),
        "CLI" => cpu.set_flag(Flag::I, false),
        "SEI" => cpu.set_flag(Flag::I, true),
        "CLV" => cpu.set_flag(Flag::V, false),

        // ── NOP (official and unofficial variants) ────────────────────────────
        "NOP" => {}

        // ════════════════════════════════════════════════════════════════════
        // Unofficial / Illegal opcode handlers
        // Spec: https://www.nesdev.org/wiki/CPU_unofficial_opcodes
        // ════════════════════════════════════════════════════════════════════

        // SLO: ASL mem, then ORA A with result. Flags: N, Z, C.
        "SLO" => {
            let m = bus.read(op.addr);
            cpu.set_flag(Flag::C, m & 0x80 != 0);
            let shifted = m << 1;
            bus.write(op.addr, shifted);
            cpu.a |= shifted;
            cpu.set_nz(cpu.a);
        }

        // RLA: ROL mem, then AND A with result. Flags: N, Z, C.
        "RLA" => {
            let m = bus.read(op.addr);
            let c_in = cpu.get_flag(Flag::C) as u8;
            cpu.set_flag(Flag::C, m & 0x80 != 0);
            let rotated = (m << 1) | c_in;
            bus.write(op.addr, rotated);
            cpu.a &= rotated;
            cpu.set_nz(cpu.a);
        }

        // SRE: LSR mem, then EOR A with result. Flags: N, Z, C.
        "SRE" => {
            let m = bus.read(op.addr);
            cpu.set_flag(Flag::C, m & 0x01 != 0);
            let shifted = m >> 1;
            bus.write(op.addr, shifted);
            cpu.a ^= shifted;
            cpu.set_nz(cpu.a);
        }

        // RRA: ROR mem, then ADC A with result. Flags: N, V, Z, C.
        "RRA" => {
            let m = bus.read(op.addr);
            let c_in = cpu.get_flag(Flag::C) as u8;
            cpu.set_flag(Flag::C, m & 0x01 != 0);
            let rotated = (m >> 1) | (c_in << 7);
            bus.write(op.addr, rotated);
            // ADC with rotated value
            let c = cpu.get_flag(Flag::C) as u16;
            let result = cpu.a as u16 + rotated as u16 + c;
            let a = cpu.a;
            cpu.set_flag(Flag::C, result > 0xFF);
            cpu.set_flag(Flag::V, (!(a ^ rotated) & (a ^ result as u8) & 0x80) != 0);
            cpu.a = result as u8;
            cpu.set_nz(cpu.a);
        }

        // SAX: store A AND X into memory. No flags affected.
        "SAX" => {
            bus.write(op.addr, cpu.a & cpu.x);
        }

        // LAX: load memory into both A and X. Flags: N, Z.
        "LAX" => {
            let val = bus.read(op.addr);
            cpu.a = val;
            cpu.x = val;
            cpu.set_nz(val);
        }

        // DCP: DEC mem, then CMP A with result. Flags: N, Z, C.
        "DCP" => {
            let val = bus.read(op.addr).wrapping_sub(1);
            bus.write(op.addr, val);
            cpu.compare(val, cpu.a);
        }

        // ISB (ISC): INC mem, then SBC A with result. Flags: N, V, Z, C.
        "ISB" => {
            let val = bus.read(op.addr).wrapping_add(1);
            bus.write(op.addr, val);
            // SBC: A - val - (1-C)  ==  ADC with val inverted
            let m = val ^ 0xFF;
            let c = cpu.get_flag(Flag::C) as u16;
            let result = cpu.a as u16 + m as u16 + c;
            let a = cpu.a;
            cpu.set_flag(Flag::C, result > 0xFF);
            cpu.set_flag(Flag::V, (!(a ^ m) & (a ^ result as u8) & 0x80) != 0);
            cpu.a = result as u8;
            cpu.set_nz(cpu.a);
        }

        _ => unreachable!("unhandled mnemonic: {}", instr.name),
    }

    extra_cycles
}