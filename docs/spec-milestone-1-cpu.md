# Milestone 1 Spec: MOS 6502 CPU

**Goal**: Implement all 56 official 6502 opcodes (151 opcode/addressing-mode combinations), a stub memory bus, and pass the `nestest.nes` log-comparison test in automation mode.

**Primary references**:
- [NESDev 6502 CPU reference](https://www.nesdev.org/wiki/CPU)
- [obelisk.me.uk 6502 reference](http://www.6502.org/tutorials/6502opcodes.html) — per-opcode cycle counts and flag behavior
- [nestest.nes log](https://www.nesdev.org/wiki/Emulator_tests) — ground truth for log-comparison mode
- [NES CPU memory map](https://www.nesdev.org/wiki/CPU_memory_map)

---

## 1. Registers

| Register | Width | Description |
|----------|-------|-------------|
| `A`      | u8    | Accumulator |
| `X`      | u8    | Index register X |
| `Y`      | u8    | Index register Y |
| `SP`     | u8    | Stack pointer — points into page $01; hardware adds $0100 offset on access |
| `PC`     | u16   | Program counter |
| `P`      | u8    | Processor status (flags) |

### Processor Status Flags (P register)

Bit layout (bit 7 = MSB):

```
N V - B D I Z C
7 6 5 4 3 2 1 0
```

| Bit | Name | Meaning |
|-----|------|---------|
| 7   | N    | Negative — set when result bit 7 = 1 |
| 6   | V    | Overflow — set on signed overflow |
| 5   | -    | Unused — always reads as 1 |
| 4   | B    | Break — set when P is pushed by BRK, clear when pushed by IRQ/NMI |
| 3   | D    | Decimal — exists on real 6502 but **ignored on NES** (2A03 has no BCD) |
| 2   | I    | Interrupt Disable |
| 1   | Z    | Zero — set when result = 0 |
| 0   | C    | Carry |

On reset, `P` initializes to `0x24` (I set, unused bit 5 always set). The B flag is **not a real register bit** — it only appears in pushed copies of P on the stack.

---

## 2. Memory Map (CPU bus, stub implementation)

The NES CPU sees a 64 KB address space. For Milestone 1 we only need enough to run nestest in headless mode (no PPU, no APU).

| Address Range  | Size   | Description |
|----------------|--------|-------------|
| $0000–$07FF    | 2 KB   | CPU RAM |
| $0800–$1FFF    | —      | RAM mirrors (repeat every $0800) |
| $2000–$3FFF    | —      | PPU registers (stub: open bus / ignore) |
| $4000–$4017    | —      | APU + I/O (stub: open bus / ignore) |
| $4018–$5FFF    | —      | Open bus (ignore reads/writes) |
| $6000–$7FFF    | 8 KB   | Work RAM (WRAM) — nestest writes result codes here |
| $8000–$FFFF    | —      | Cartridge PRG-ROM |
| $FFFA–$FFFB    | 2 B    | NMI vector |
| $FFFC–$FFFD    | 2 B    | Reset vector |
| $FFFE–$FFFF    | 2 B    | IRQ/BRK vector |

**RAM mirroring**: addresses $0000–$07FF are mirrored at $0800, $1000, $1800. Mask with `addr & 0x07FF`.

**Cartridge stub**: For milestone 1, load a ROM into a flat byte array and map `$8000–$FFFF` to it (NROM-128 or NROM-256 layout from the iNES file). Full mapper logic comes in Milestone 2 — a minimal "load PRG-ROM into upper memory" is sufficient here.

---

## 3. Addressing Modes

All 13 addressing modes used by the 6502:

| Mode | Notation | Bytes | Description |
|------|----------|-------|-------------|
| Implied | — | 1 | Operand is implicit (e.g., `CLC`) |
| Accumulator | `A` | 1 | Operand is the A register |
| Immediate | `#$nn` | 2 | Operand is the literal next byte |
| Zero Page | `$nn` | 2 | Address is in page $00 |
| Zero Page,X | `$nn,X` | 2 | Zero page address + X (wraps in page) |
| Zero Page,Y | `$nn,Y` | 2 | Zero page address + Y (wraps in page) |
| Absolute | `$nnnn` | 3 | Full 16-bit address |
| Absolute,X | `$nnnn,X` | 3 | Full address + X (may cross page → +1 cycle) |
| Absolute,Y | `$nnnn,Y` | 3 | Full address + Y (may cross page → +1 cycle) |
| Indirect | `($nnnn)` | 3 | JMP only; reads address from pointer |
| Indirect,X | `($nn,X)` | 2 | Zero page pointer + X before dereference |
| Indirect,Y | `($nn),Y` | 2 | Zero page pointer dereferenced, then + Y (may cross page → +1 cycle) |
| Relative | `±$nn` | 2 | Branch offset (signed); may cross page → +1 cycle |

**Page-cross penalty**: For the modes marked above, if the final effective address crosses a page boundary (high byte changes), add 1 cycle. Exception: write instructions (STA, STX, etc.) always pay the full cycle count — no page-cross discount.

**Indirect JMP bug** (hardware errata, must emulate): `JMP ($xxFF)` reads the low byte from `$xxFF` and the high byte from `$xx00` (not `$xx00+1` of the next page). This is a documented 6502 hardware bug.

---

## 4. Instruction Set

### 4.1 Load/Store

| Opcode | Modes | Flags |
|--------|-------|-------|
| LDA | Imm, ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | N, Z |
| LDX | Imm, ZP, ZP,Y, Abs, Abs,Y | N, Z |
| LDY | Imm, ZP, ZP,X, Abs, Abs,X | N, Z |
| STA | ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | — |
| STX | ZP, ZP,Y, Abs | — |
| STY | ZP, ZP,X, Abs | — |

### 4.2 Register Transfers

| Opcode | Operation | Flags |
|--------|-----------|-------|
| TAX | A → X | N, Z |
| TAY | A → Y | N, Z |
| TXA | X → A | N, Z |
| TYA | Y → A | N, Z |
| TSX | SP → X | N, Z |
| TXS | X → SP | — (no flags) |

### 4.3 Stack Operations

| Opcode | Operation | Flags |
|--------|-----------|-------|
| PHA | Push A | — |
| PHP | Push P (with B=1, bit5=1) | — |
| PLA | Pull A | N, Z |
| PLP | Pull P (B and bit5 ignored on pull) | all |

**Stack**: grows downward. Push: write to `$0100 + SP`, decrement SP. Pull: increment SP, read from `$0100 + SP`.

### 4.4 Arithmetic

| Opcode | Modes | Flags | Notes |
|--------|-------|-------|-------|
| ADC | Imm, ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | N, V, Z, C | A + M + C |
| SBC | Imm, ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | N, V, Z, C | A - M - (1-C) |

**Overflow flag (V)**: set when the sign of the result differs from what the signs of both operands predict. Formula: `V = (A ^ result) & (M ^ result) & 0x80 != 0` (for ADC; for SBC, invert M first).

### 4.5 Increment/Decrement

| Opcode | Modes | Flags |
|--------|-------|-------|
| INC | ZP, ZP,X, Abs, Abs,X | N, Z |
| INX | Implied | N, Z |
| INY | Implied | N, Z |
| DEC | ZP, ZP,X, Abs, Abs,X | N, Z |
| DEX | Implied | N, Z |
| DEY | Implied | N, Z |

### 4.6 Logical

| Opcode | Modes | Flags |
|--------|-------|-------|
| AND | Imm, ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | N, Z |
| ORA | Imm, ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | N, Z |
| EOR | Imm, ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | N, Z |

### 4.7 Shifts and Rotates

| Opcode | Modes | Flags | Notes |
|--------|-------|-------|-------|
| ASL | Acc, ZP, ZP,X, Abs, Abs,X | N, Z, C | Shift left; bit 0 ← 0; old bit 7 → C |
| LSR | Acc, ZP, ZP,X, Abs, Abs,X | N, Z, C | Shift right; bit 7 ← 0; old bit 0 → C |
| ROL | Acc, ZP, ZP,X, Abs, Abs,X | N, Z, C | Rotate left through C |
| ROR | Acc, ZP, ZP,X, Abs, Abs,X | N, Z, C | Rotate right through C |

### 4.8 Compare

| Opcode | Modes | Flags | Notes |
|--------|-------|-------|-------|
| CMP | Imm, ZP, ZP,X, Abs, Abs,X, Abs,Y, (Ind,X), (Ind),Y | N, Z, C | A - M (no store) |
| CPX | Imm, ZP, Abs | N, Z, C | X - M |
| CPY | Imm, ZP, Abs | N, Z, C | Y - M |

**Compare flag rules**: C is set if register ≥ M (unsigned). Z is set if equal. N reflects bit 7 of result.

### 4.9 Bit Test

| Opcode | Modes | Flags |
|--------|-------|-------|
| BIT | ZP, Abs | N ← M7, V ← M6, Z ← (A AND M) == 0 |

N and V are set from bits 7 and 6 of the **memory value**, not the AND result.

### 4.10 Branches

All branches are relative mode (signed 1-byte offset). Cycles: 2 base + 1 if branch taken + 1 more if page crossed.

| Opcode | Condition |
|--------|-----------|
| BCC | C = 0 |
| BCS | C = 1 |
| BEQ | Z = 1 |
| BNE | Z = 0 |
| BMI | N = 1 |
| BPL | N = 0 |
| BVC | V = 0 |
| BVS | V = 1 |

### 4.11 Jumps and Calls

| Opcode | Mode | Cycles | Notes |
|--------|------|--------|-------|
| JMP | Absolute | 3 | PC ← address |
| JMP | Indirect | 5 | PC ← mem[address]; has page-wrap bug (see §3) |
| JSR | Absolute | 6 | Push PC-1 (high then low), PC ← address |
| RTS | Implied | 6 | Pull PC (low then high), PC ← PC+1 |
| RTI | Implied | 6 | Pull P, pull PC (low then high); B flag and bit5 ignored on P pull |

**JSR push order**: pushes PC+2 (the address of the last byte of the JSR instruction). RTS increments the pulled address by 1 to get the return address. Net result: execution resumes at the instruction after JSR.

### 4.12 Interrupts

| Opcode | Cycles | Notes |
|--------|--------|-------|
| BRK | 7 | Push PC+2, push P with B=1; load PC from $FFFE/$FFFF; set I |

**BRK padding**: BRK is a 2-byte instruction (1 opcode + 1 padding byte that is ignored). PC+2 is pushed.

### 4.13 Flag Instructions

| Opcode | Effect |
|--------|--------|
| CLC | C ← 0 |
| SEC | C ← 1 |
| CLD | D ← 0 |
| SED | D ← 1 |
| CLI | I ← 0 |
| SEI | I ← 1 |
| CLV | V ← 0 |

### 4.14 No-op

| Opcode | Cycles |
|--------|--------|
| NOP | 2 (Implied) |

---

## 4.15 Unofficial / Illegal Opcodes

nestest exercises a subset of the 6502's illegal opcodes after the official suite. These must be implemented correctly (right behavior **and** right cycle count) for `$6000` to reach `$00`.

Reference: [NESDev unofficial opcodes](https://www.nesdev.org/wiki/CPU_unofficial_opcodes)

In the log, unofficial instructions are prefixed with `*` (e.g., `*NOP`).

### Unofficial NOP variants

The 6502 has many opcode slots that behave as NOPs but with different addressing modes and cycle counts. All read (and discard) their operand; none affect flags or registers.

| Opcodes | Mode | Cycles |
|---------|------|--------|
| $1A $3A $5A $7A $DA $FA | Implied | 2 |
| $80 $82 $89 $C2 $E2 | Immediate | 2 |
| $04 $44 $64 | Zero Page | 3 |
| $14 $34 $54 $74 $D4 $F4 | Zero Page,X | 4 |
| $0C | Absolute | 4 |
| $1C $3C $5C $7C $DC $FC | Absolute,X (+1 if page cross) | 4 |

### ALU illegal opcodes

These combine two official operations in a single instruction.

| Mnemonic | Opcode(s) | Operation | Flags | Notes |
|----------|-----------|-----------|-------|-------|
| SLO | $07 $17 $0F $1F $1B $03 $13 | ASL mem, then ORA result into A | N, Z, C | All mem write modes; no page-cross discount |
| RLA | $27 $37 $2F $3F $3B $23 $33 | ROL mem, then AND result into A | N, Z, C | |
| SRE | $47 $57 $4F $5F $5B $43 $53 | LSR mem, then EOR result into A | N, Z, C | |
| RRA | $67 $77 $6F $7F $7B $63 $73 | ROR mem, then ADC result into A | N, V, Z, C | |
| SAX | $87 $97 $8F $83 | Store A AND X into mem | — | No flags affected |
| LAX | $A7 $B7 $AF $BF $A3 $B3 | Load mem into both A and X | N, Z | |
| DCP | $C7 $D7 $CF $DF $DB $C3 $D3 | DEC mem, then CMP A with result | N, Z, C | |
| ISC | $E7 $F7 $EF $FF $FB $E3 $F3 | INC mem, then SBC A with result | N, V, Z, C | |

**Addressing modes per opcode**: The opcode byte encodes the mode using the standard 6502 `aaabbbcc` encoding. Use the NESDev table to map each opcode byte to its mode and cycle count.

### Log format for unofficial opcodes

Unofficial instructions appear with a `*` prefix in the mnemonic field, e.g.:

```
C6BD  04 A9     *NOP $A9 = 00                   A:AA X:97 Y:4E P:EF SP:F9 PPU:  0,  0 CYC:14579
```

---

## 5. Cycle Counts

Cycle counts must be exact — nestest verifies them via the cycle counter in the log. Key rules:

1. Every fetch costs 1 cycle (including opcode fetch, operand bytes, pointer dereferences).
2. Write instructions (STA, STX, STY, INC, DEC, shifts on memory) do not get a page-cross discount.
3. Read-modify-write instructions (INC, DEC, ASL/LSR/ROL/ROR on memory) perform: fetch, fetch operand, read, **dummy write** (write old value back), write new value. The dummy write is required for cycle accuracy.
4. Stack push = 1 cycle per byte; pull = 1 cycle per byte.
5. Branch not taken: 2 cycles. Taken, same page: 3. Taken, page cross: 4.

Full cycle table: consult [obelisk.me.uk opcode reference](http://www.6502.org/tutorials/6502opcodes.html) or the NESDev cycle-by-cycle breakdown.

---

## 6. Reset Sequence

On power-on/reset, the CPU performs:
1. 7 cycles of initialization (internal — no visible bus activity for first 6, then reads reset vector).
2. Reads `$FFFC` (low byte of reset vector), then `$FFFD` (high byte).
3. Sets `PC` to the reset vector address.
4. Sets `SP` to `$FD` (decremented 3 times during reset sequence: `$00 → $FD`).
5. Sets `I` flag.
6. `A`, `X`, `Y` are undefined at power-on (treat as 0 for simplicity).

---

## 7. Interrupt Handling (IRQ and NMI)

Not required to pass nestest automation mode, but implement the vectors correctly:

- **NMI**: edge-triggered on PPU VBlank (not yet wired in milestone 1). Vector at `$FFFA/$FFFB`.
- **IRQ**: level-triggered; only fires when I=0. Vector at `$FFFE/$FFFF`.
- Both push PC (high, low) then P (with B=0) onto stack, set I, and load PC from vector.

---

## 8. nestest Integration

nestest runs in two modes:
- **Interactive** (start PC at reset vector `$C000` on the nestest ROM): runs normally, uses PPU.
- **Automation** (start PC at `$C000` directly, no PPU needed): writes pass/fail codes to zero-page RAM.

**For milestone 1, use automation mode**:
1. Load `nestest.nes` — PRG-ROM is 16 KB, mapped at `$C000–$FFFF` (and mirrored at `$8000–$BFFF`).
2. Force `PC = $C000` at startup (override the reset vector read, or patch `$FFFC/$FFFD` to point to `$C000`).
3. Run the step loop until the ROM halts in its infinite BRK loop at `$0001`.
4. After halting, read result codes from zero page: `$02` = official opcode result, `$03` = unofficial opcode result. `$00` in each means that group passed; any other value is a failure code.
5. Compare CPU log output against [nestest.log](https://www.nesdev.org/wiki/Emulator_tests#nestest.log) line by line.

**Note**: The `$6000` result-code mechanism (running sentinel `$80`, pass = `$00`, human-readable message at `$6001–$6003`) applies to Blargg's *other* test ROMs (`cpu_timing_test`, `ppu_vbl_nmi`, etc.) — **not** to nestest.nes. nestest.nes uses `$02`/`$03` in zero page.

**Log format** (one line per instruction, before execution):
```
C000  4C F5 C5  JMP $C5F5                       A:00 X:00 Y:00 P:24 SP:FD PPU:  0, 21 CYC:7
```
Fields: `PC`, up to 3 opcode bytes, disassembly, register state, PPU scanline/dot, total cycle count.

**PPU scanline/dot** are derived from the CPU cycle counter — no real PPU needed:
- `dot      = (cycles * 3) % 341`
- `scanline = (cycles * 3) / 341`

The cycle counter starts at **7** (the reset sequence cycles).

---

## 9. Module Structure

```
src/
  main.rs          # Loads ROM, constructs Bus + Cpu, runs step loop, writes log
  bus.rs           # Memory bus: RAM, ROM stub, read/write dispatch
  cpu/
    mod.rs         # Cpu struct, registers, step(), reset(), interrupt handling
    opcodes.rs     # Opcode dispatch table (opcode byte → handler)
    addressing.rs  # Addressing mode resolution → (effective_addr, extra_cycles)
```

The `Cpu` holds a mutable reference to `Bus`. `Bus` owns CPU RAM (`[u8; 2048]`) and the ROM bytes loaded from the iNES file.

---

## 10. Acceptance Criteria

- [ ] `cargo build` succeeds with no warnings on stable Rust.
- [ ] All 56 official opcodes implemented across all documented addressing modes.
- [ ] Unofficial opcodes from §4.15 implemented (unofficial NOPs + ALU illegals).
- [ ] JMP indirect page-wrap bug emulated.
- [ ] Bus has readable/writable WRAM at `$6000–$7FFF` (needed for Blargg test ROMs in later milestones).
- [ ] CPU log output matches `nestest.log` exactly for all 8991 lines (PC, bytes, disassembly, registers, PPU dot/scanline, cycle count). Unofficial instructions use a `*` prefix in the mnemonic field.
- [ ] `$02` reads `$00` (official tests passed) and `$03` reads `$00` (unofficial tests passed) at the end of the nestest run.
- [ ] No `unwrap()` in CPU or bus core paths.