pub mod addressing;
pub mod opcodes;

use crate::bus::Bus;
use opcodes::decode;

/// Processor status flag bit positions.
/// Spec: https://www.nesdev.org/wiki/Status_flags
#[repr(u8)]
#[derive(Clone, Copy)]
pub enum Flag {
    C = 0b0000_0001, // Carry
    Z = 0b0000_0010, // Zero
    I = 0b0000_0100, // Interrupt disable
    D = 0b0000_1000, // Decimal (ignored on NES)
    B = 0b0001_0000, // Break (stack copy only)
    // bit 5 = unused, always 1
    V = 0b0100_0000, // Overflow
    N = 0b1000_0000, // Negative
}

pub struct Cpu {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    /// Processor status register. Bit 5 is always 1.
    pub p: u8,
    /// Total master cycles elapsed (each CPU cycle = 1 here; caller multiplies by 3).
    pub cycles: u64,
    /// Pending maskable interrupt — set by `request_irq()`, honoured at start of next instruction
    /// if the I flag is clear. Spec: https://www.nesdev.org/wiki/CPU_interrupts
    pub irq_pending: bool,
    /// Number of cycles to stall the CPU (used for DMC DMA).
    pub stall_cycles: u32,
}

impl Cpu {
    /// Power-on state. Spec: https://www.nesdev.org/wiki/CPU_power_up_state
    pub fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            sp: 0xFD,
            pc: 0,
            p: 0x24, // I set, bit 5 always set
            cycles: 0,
            irq_pending: false,
            stall_cycles: 0,
        }
    }

    /// Assert a maskable IRQ. The interrupt will be serviced at the start of the next
    /// instruction if the I flag is clear.
    pub fn request_irq(&mut self) {
        self.irq_pending = true;
    }

    /// Load the reset vector and set PC. Consumes 7 cycles (reset sequence).
    pub fn reset(&mut self, bus: &mut Bus) {
        self.pc = bus.read_u16(0xFFFC);
        self.sp = 0xFD;
        self.p = 0x24;
        self.cycles = 7;
    }

    // ── Flag helpers ──────────────────────────────────────────────────────────

    #[inline]
    pub fn get_flag(&self, f: Flag) -> bool {
        self.p & (f as u8) != 0
    }

    #[inline]
    pub fn set_flag(&mut self, f: Flag, val: bool) {
        if val {
            self.p |= f as u8;
        } else {
            self.p &= !(f as u8);
        }
        self.p |= 0x20; // bit 5 always set
    }

    /// Set N and Z flags from a result byte.
    #[inline]
    pub fn set_nz(&mut self, val: u8) {
        self.set_flag(Flag::N, val & 0x80 != 0);
        self.set_flag(Flag::Z, val == 0);
    }

    /// Compare: set C, Z, N based on (reg - mem) without storing.
    #[inline]
    pub fn compare(&mut self, m: u8, reg: u8) {
        let result = reg.wrapping_sub(m);
        self.set_flag(Flag::C, reg >= m);
        self.set_nz(result);
    }

    // ── Stack helpers ─────────────────────────────────────────────────────────

    #[inline]
    pub fn stack_push(&mut self, bus: &mut Bus, val: u8) {
        bus.write(0x0100 | self.sp as u16, val);
        self.sp = self.sp.wrapping_sub(1);
    }

    #[inline]
    pub fn stack_pull(&mut self, bus: &mut Bus) -> u8 {
        self.sp = self.sp.wrapping_add(1);
        bus.read(0x0100 | self.sp as u16)
    }

    // ── Branch helper ─────────────────────────────────────────────────────────

    /// Apply a branch if `taken`. Returns extra cycles consumed (0, 1, or 2).
    /// `offset` is the raw signed offset from the Relative addressing resolver
    /// (stored as i16-cast-to-u16 in op.addr).
    pub fn branch(&mut self, offset: u16, taken: bool) -> u8 {
        if !taken {
            return 0;
        }
        let offset = offset as i16; // recover sign
        let old_pc = self.pc;
        self.pc = self.pc.wrapping_add(offset as u16);
        let page_crossed = (old_pc & 0xFF00) != (self.pc & 0xFF00);
        1 + page_crossed as u8
    }

    // ── Interrupts ────────────────────────────────────────────────────────────

    /// Service a non-maskable interrupt.
    ///
    /// Sequence (Spec: https://www.nesdev.org/wiki/CPU_interrupts):
    ///   1. Push PCH, then PCL.
    ///   2. Push P with B (bit 4) clear and bit 5 set.
    ///   3. Set I flag.
    ///   4. Load PC from $FFFA/$FFFB.
    ///   5. Consume 7 cycles.
    pub fn nmi(&mut self, bus: &mut Bus) {
        let pc_hi = (self.pc >> 8) as u8;
        let pc_lo = (self.pc & 0xFF) as u8;
        self.stack_push(bus, pc_hi);
        self.stack_push(bus, pc_lo);
        // Push P with B (bit 4) clear, bit 5 always set.
        let pushed_p = (self.p & !0x10) | 0x20;
        self.stack_push(bus, pushed_p);
        self.p |= 0x04; // I flag
        let lo = bus.read(0xFFFA) as u16;
        let hi = bus.read(0xFFFB) as u16;
        self.pc = (hi << 8) | lo;
        self.cycles += 7;
    }

    // ── Main step ─────────────────────────────────────────────────────────────

    /// Service a maskable interrupt (IRQ).
    ///
    /// Sequence mirrors NMI but loads the IRQ vector at $FFFE/$FFFF and sets B=0.
    /// Spec: https://www.nesdev.org/wiki/CPU_interrupts
    pub fn irq(&mut self, bus: &mut Bus) {
        let pc_hi = (self.pc >> 8) as u8;
        let pc_lo = (self.pc & 0xFF) as u8;
        self.stack_push(bus, pc_hi);
        self.stack_push(bus, pc_lo);
        let pushed_p = (self.p & !0x10) | 0x20;
        self.stack_push(bus, pushed_p);
        self.p |= 0x04; // set I flag
        let lo = bus.read(0xFFFE) as u16;
        let hi = bus.read(0xFFFF) as u16;
        self.pc = (hi << 8) | lo;
        self.cycles += 7;
    }

    /// Execute one instruction. Returns total CPU cycles consumed.
    pub fn step(&mut self, bus: &mut Bus) -> u8 {
        // Service pending IRQ before fetch if I flag is clear.
        if self.irq_pending && self.p & 0x04 == 0 {
            self.irq_pending = false;
            self.irq(bus);
        }

        let opcode = bus.read(self.pc);
        self.pc = self.pc.wrapping_add(1);

        match decode(opcode) {
            Some(instr) => {
                let base = instr.cycles;
                let extra = opcodes::execute(self, bus, opcode, &instr);
                let total = base + extra;
                self.cycles += total as u64;
                total
            }
            None => {
                // Illegal opcode — treat as NOP (2 cycles) to avoid halting.
                // nestest does not exercise illegal opcodes in automation mode.
                self.cycles += 2;
                2
            }
        }
    }

    // ── Disassembly (for logging) ─────────────────────────────────────────────

    /// Format one instruction at `addr` for nestest-style log output.
    /// Does not advance any CPU state.
    pub fn disassemble(&self, bus: &mut Bus, addr: u16) -> String {
        let opcode = bus.read(addr);
        let b1 = bus.read(addr.wrapping_add(1));
        let b2 = bus.read(addr.wrapping_add(2));

        match decode(opcode) {
            None => format!("{:04X}  {:02X}        ???", addr, opcode),
            Some(instr) => {
                use addressing::AddrMode::*;
                // Unofficial instructions are prefixed with '*' in the mnemonic field.
                let mn = if instr.unofficial {
                    format!("*{}", instr.name)
                } else {
                    instr.name.to_string()
                };
                let (bytes_str, asm) = match instr.mode {
                    Implied => (format!("{:02X}      ", opcode), mn.clone()),
                    Accumulator => (format!("{:02X}      ", opcode), format!("{} A", mn)),
                    Immediate => (
                        format!("{:02X} {:02X}   ", opcode, b1),
                        format!("{} #${:02X}", mn, b1),
                    ),
                    ZeroPage => (
                        format!("{:02X} {:02X}   ", opcode, b1),
                        format!("{} ${:02X} = {:02X}", mn, b1, bus.read(b1 as u16)),
                    ),
                    ZeroPageX => {
                        let eff = b1.wrapping_add(self.x) as u16;
                        (
                            format!("{:02X} {:02X}   ", opcode, b1),
                            format!(
                                "{} ${:02X},X @ {:02X} = {:02X}",
                                mn,
                                b1,
                                eff as u8,
                                bus.read(eff)
                            ),
                        )
                    }
                    ZeroPageY => {
                        let eff = b1.wrapping_add(self.y) as u16;
                        (
                            format!("{:02X} {:02X}   ", opcode, b1),
                            format!(
                                "{} ${:02X},Y @ {:02X} = {:02X}",
                                mn,
                                b1,
                                eff as u8,
                                bus.read(eff)
                            ),
                        )
                    }
                    Absolute => {
                        let target = b1 as u16 | ((b2 as u16) << 8);
                        let val_str = if instr.name == "JMP" || instr.name == "JSR" {
                            String::new()
                        } else {
                            format!(" = {:02X}", bus.read(target))
                        };
                        (
                            format!("{:02X} {:02X} {:02X}", opcode, b1, b2),
                            format!("{} ${:04X}{}", mn, target, val_str),
                        )
                    }
                    AbsoluteX => {
                        let base = b1 as u16 | ((b2 as u16) << 8);
                        let eff = base.wrapping_add(self.x as u16);
                        (
                            format!("{:02X} {:02X} {:02X}", opcode, b1, b2),
                            format!(
                                "{} ${:04X},X @ {:04X} = {:02X}",
                                mn,
                                base,
                                eff,
                                bus.read(eff)
                            ),
                        )
                    }
                    AbsoluteY => {
                        let base = b1 as u16 | ((b2 as u16) << 8);
                        let eff = base.wrapping_add(self.y as u16);
                        (
                            format!("{:02X} {:02X} {:02X}", opcode, b1, b2),
                            format!(
                                "{} ${:04X},Y @ {:04X} = {:02X}",
                                mn,
                                base,
                                eff,
                                bus.read(eff)
                            ),
                        )
                    }
                    Indirect => {
                        let ptr = b1 as u16 | ((b2 as u16) << 8);
                        let eff = bus.read_u16_page_wrap(ptr);
                        (
                            format!("{:02X} {:02X} {:02X}", opcode, b1, b2),
                            format!("{} (${:04X}) = {:04X}", mn, ptr, eff),
                        )
                    }
                    IndirectX => {
                        let ptr = b1.wrapping_add(self.x) as u16;
                        let eff = bus.read_u16_page_wrap(ptr);
                        (
                            format!("{:02X} {:02X}   ", opcode, b1),
                            format!(
                                "{} (${:02X},X) @ {:02X} = {:04X} = {:02X}",
                                mn,
                                b1,
                                ptr as u8,
                                eff,
                                bus.read(eff)
                            ),
                        )
                    }
                    IndirectY => {
                        let ptr = b1 as u16;
                        let base = bus.read_u16_page_wrap(ptr);
                        let eff = base.wrapping_add(self.y as u16);
                        (
                            format!("{:02X} {:02X}   ", opcode, b1),
                            format!(
                                "{} (${:02X}),Y = {:04X} @ {:04X} = {:02X}",
                                mn,
                                b1,
                                base,
                                eff,
                                bus.read(eff)
                            ),
                        )
                    }
                    Relative => {
                        let offset = b1 as i8 as i16;
                        let target = (addr.wrapping_add(2) as i16).wrapping_add(offset) as u16;
                        (
                            format!("{:02X} {:02X}   ", opcode, b1),
                            format!("{} ${:04X}", mn, target),
                        )
                    }
                };
                // Unofficial instructions use a 1-space separator (official use 2)
                // so the mnemonic column shifts left by 1 to accommodate the '*' prefix,
                // keeping the register field at a fixed column.
                // Reference: compare nestest.log column alignment for *NOP vs NOP.
                if instr.unofficial {
                    format!("{:04X}  {:<8} {:<33}", addr, bytes_str, asm)
                } else {
                    format!("{:04X}  {:<8}  {:<32}", addr, bytes_str, asm)
                }
            }
        }
    }
}
