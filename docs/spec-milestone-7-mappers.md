# Milestone 7 Spec: Mappers 1–4 — Expand Game Compatibility

**Goal**: Implement Mapper 1 (MMC1), Mapper 2 (UxROM), Mapper 3 (CNROM), and Mapper 4 (MMC3), wiring each into the existing `Mapper` trait and cartridge dispatch. No changes to the bus or CPU core are required; the `Mapper` interface is already the correct abstraction.

**Primary references**:
- [NESDev MMC1 (Mapper 1)](https://www.nesdev.org/wiki/MMC1)
- [NESDev UxROM (Mapper 2)](https://www.nesdev.org/wiki/UxROM)
- [NESDev CNROM (Mapper 3)](https://www.nesdev.org/wiki/INES_Mapper_003)
- [NESDev MMC3 (Mapper 4)](https://www.nesdev.org/wiki/MMC3)
- [NESDev PRG ROM circuit](https://www.nesdev.org/wiki/PRG_ROM_circuit) — address decoding reference
- [NESDev Mirroring](https://www.nesdev.org/wiki/Mirroring)

**Games unlocked** (representative):
| Mapper | Games |
|--------|-------|
| 1 MMC1  | Legend of Zelda, Metroid, Mega Man 2, Final Fantasy |
| 2 UxROM | Mega Man, Castlevania, Contra, DuckTales |
| 3 CNROM | Donkey Kong Jr., Balloon Fight, Gradius |
| 4 MMC3  | Super Mario Bros 3, Kirby's Adventure, Mega Man 3–6 |

---

## 1. Mapper 1 — MMC1 (SxROM)

**Source**: https://www.nesdev.org/wiki/MMC1

### 1.1 Overview

MMC1 uses a 5-bit serial shift register to configure four internal registers. All writes to $8000–$FFFF clock this register. The target register is selected by the address of the write that completes a 5-bit load.

### 1.2 Shift Register

The shift register starts loaded with `0b10000_0` (bit 4 set as the sentinel, data shifted in at bit 0). On each write to $8000–$FFFF:

1. If bit 7 of the written value is set: reset the shift register to `0b100000` and set Control register bits 2–3 to `11` (PRG mode 3: fix last bank at $C000).
2. Otherwise: shift bit 0 of the written value into the MSB of the shift register; rotate right.
3. If this is the 5th write (sentinel bit has reached bit 0): latch the 5-bit value into the target register, then reset the shift register.

The target register is determined by bits 13–14 of the address of the final write:

| Address bits 14–13 | Register | Address range |
|---------------------|----------|---------------|
| 00                  | Control  | $8000–$9FFF  |
| 01                  | CHR bank 0 | $A000–$BFFF |
| 10                  | CHR bank 1 | $C000–$DFFF |
| 11                  | PRG bank | $E000–$FFFF  |

**Consecutive-write guard**: The MMC1 ignores writes on consecutive CPU cycles. Track the cycle number at which the last write was processed; discard writes that arrive on the very next cycle.

### 1.3 Control Register ($8000–$9FFF, bits 4–0)

```
4  3  2  1  0
P  P  M  M  M
```

| Bits | Name | Meaning |
|------|------|---------|
| 4–3  | PRG mode | See §1.5 |
| 2    | CHR mode | 0 = 8 KB single switch; 1 = 4 KB dual switch |
| 1–0  | Mirroring | 0 = single-screen lower; 1 = single-screen upper; 2 = vertical; 3 = horizontal |

Power-on default: `0b01100` (PRG mode 3, CHR mode 0, vertical mirroring).

### 1.4 Mirroring

| Bits 1–0 | Mode |
|----------|------|
| 0        | Single-screen, lower bank (NT all map to $2000–$23FF) |
| 1        | Single-screen, upper bank (NT all map to $2400–$27FF) |
| 2        | Vertical |
| 3        | Horizontal |

Single-screen modes are not used by any game in scope for this milestone; implement them correctly but they need not be tested.

### 1.5 PRG Bank Register ($E000–$FFFF, bits 4–0)

Bit 4 — PRG RAM enable: 0 = enabled, 1 = disabled (chip-enable is inverted). Bits 3–0 — PRG bank select (up to 16 × 16 KB banks).

**PRG mode** (control bits 4–3):

| Mode | $8000–$BFFF | $C000–$FFFF |
|------|-------------|-------------|
| 0, 1 | low half of selected 32 KB pair (bank & !1) | high half of same pair (bank \| 1) |
| 2    | fixed to bank 0 | selected bank |
| 3    | selected bank | fixed to last bank |

For mode 0/1, treat the 4-bit bank number as a 32 KB selector by ignoring the lowest bit.

### 1.6 CHR Bank Registers ($A000–$BFFF and $C000–$DFFF)

- **CHR mode 0** (8 KB switching): CHR bank 0 register selects an 8 KB window. Only bits 4–1 are significant (bit 0 is ignored); the selected 8 KB starts at `(bank & !1) * 4096`. CHR bank 1 register is unused.
- **CHR mode 1** (4 KB switching): CHR bank 0 register selects the 4 KB window at PPU $0000–$0FFF; CHR bank 1 register selects the 4 KB window at PPU $1000–$1FFF.

### 1.7 PRG RAM ($6000–$7FFF)

8 KB writable. When PRG RAM is disabled (PRG bank register bit 4 set), reads return open bus (0xFF) and writes are ignored.

### 1.8 Struct Fields

```rust
pub struct Mapper1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_ram: bool,
    prg_ram: Vec<u8>,
    shift: u8,        // 5-bit shift register; sentinel in bit 5
    control: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,
    last_write_cycle: u64,  // consecutive-write guard
}
```

---

## 2. Mapper 2 — UxROM

**Source**: https://www.nesdev.org/wiki/UxROM

### 2.1 Overview

UxROM is the simplest switchable-PRG mapper. A write to any address in $8000–$FFFF selects the 16 KB PRG bank at $8000–$BFFF. The bank at $C000–$FFFF is always fixed to the last 16 KB bank.

### 2.2 CPU Bus

| Address | Mapped to |
|---------|-----------|
| $6000–$7FFF | Open bus (no PRG RAM on UxROM games) |
| $8000–$BFFF | Selected PRG bank (16 KB) |
| $C000–$FFFF | Last PRG bank (fixed) |

**Bank select register**: write to any address $8000–$FFFF. Only the low bits are significant; mask with `(prg_rom.len() / 16384 - 1)` to handle varying ROM sizes.

Address translation:
```
$8000–$BFFF: prg_rom[bank_select * 16384 + (addr & 0x3FFF)]
$C000–$FFFF: prg_rom[last_bank * 16384 + (addr & 0x3FFF)]
```

### 2.3 PPU Bus ($0000–$1FFF)

UxROM cartridges use 8 KB of CHR-RAM (no CHR-ROM). Reads and writes go to `chr_ram[(addr & 0x1FFF) as usize]`.

### 2.4 Mirroring

Fixed — determined by the iNES header bit as with Mapper 0. The mapper does not change mirroring at runtime.

### 2.5 Struct Fields

```rust
pub struct Mapper2 {
    prg_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    bank_select: usize,
    last_bank: usize,
    mirroring: Mirroring,
}
```

---

## 3. Mapper 3 — CNROM

**Source**: https://www.nesdev.org/wiki/INES_Mapper_003

### 3.1 Overview

CNROM has fixed PRG-ROM (like Mapper 0) and a switchable 8 KB CHR bank selected by writes to $8000–$FFFF.

### 3.2 CPU Bus

Identical to Mapper 0 PRG handling: fixed window(s) into PRG-ROM, mirrored if 16 KB.

| Address | Mapped to |
|---------|-----------|
| $8000–$BFFF | PRG bank 0 (first 16 KB) |
| $C000–$FFFF | PRG bank 1 if 32 KB; else mirror of bank 0 |

Writes to $8000–$FFFF select the CHR bank (do not ignore them as in Mapper 0).

**CHR bank select**: `data & 0x03` (2-bit). Some boards pass through more bits; clamping to 2 bits is correct for all CNROM games in scope.

### 3.3 PPU Bus ($0000–$1FFF)

Selected 8 KB CHR-ROM bank: `chr_rom[chr_bank * 8192 + (addr & 0x1FFF)]`

### 3.4 Mirroring

Fixed from header; no runtime changes.

### 3.5 Struct Fields

```rust
pub struct Mapper3 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_bank: usize,
    mirroring: Mirroring,
}
```

---

## 4. Mapper 4 — MMC3 (TxROM)

**Source**: https://www.nesdev.org/wiki/MMC3

### 4.1 Overview

MMC3 is the most complex mapper in this milestone. It supports switchable PRG and CHR banks via a two-register write protocol and generates scanline-counted CPU IRQs by monitoring PPU A12.

### 4.2 Bank Select / Bank Data Protocol

Two registers share $8000 (even) and $8001 (odd):

**$8000 (even) — Bank Select**:

```
7  6  5  4  3  2  1  0
C  P  —  —  —  R  R  R
```

| Bit(s) | Name | Meaning |
|--------|------|---------|
| 7      | CHR mode | 0 = 2 KB CHR banks at PPU $0000; 1 = 2 KB CHR banks at PPU $1000 |
| 6      | PRG mode | 0 = R6 fixed at $8000, last bank at $C000; 1 = last bank at $8000, R6 at $C000 |
| 2–0    | Register select | Which of R0–R7 the next $8001 write targets |

**$8001 (odd) — Bank Data**: writes the value into the register selected by bits 2–0 of the last $8000 write.

The 8 bank registers R0–R7:

| Register | Type | PPU/CPU window |
|----------|------|----------------|
| R0 | CHR 2 KB | lower 2 KB of 2 KB region (see CHR mode) |
| R1 | CHR 2 KB | upper 2 KB of 2 KB region |
| R2 | CHR 1 KB | 1 KB slot 2 |
| R3 | CHR 1 KB | 1 KB slot 3 |
| R4 | CHR 1 KB | 1 KB slot 4 |
| R5 | CHR 1 KB | 1 KB slot 5 |
| R6 | PRG 8 KB | swappable 8 KB PRG slot (location depends on PRG mode) |
| R7 | PRG 8 KB | always at $A000–$BFFF |

### 4.3 CHR Mapping

CHR mode = 0 (bit 7 of $8000 = 0):

| PPU address | Bank register | Bank size |
|-------------|---------------|-----------|
| $0000–$07FF | R0 (bit 0 cleared) | 2 KB |
| $0800–$0FFF | R1 (bit 0 cleared) | 2 KB |
| $1000–$13FF | R2 | 1 KB |
| $1400–$17FF | R3 | 1 KB |
| $1800–$1BFF | R4 | 1 KB |
| $1C00–$1FFF | R5 | 1 KB |

CHR mode = 1 (bit 7 of $8000 = 1): swap the two halves — R2–R5 map to $0000–$0FFF, R0/R1 map to $1000–$1FFF.

Address translation for a 1 KB slot (example R2 in mode 0):
```
chr_rom[r2 * 1024 + (addr & 0x03FF)]
```

For the 2 KB slots: R0 selects the bank at $0000/$1000 and R1 selects the independent bank at $0800/$1800. Per the NESDev MMC3 spec, the low bit of each register is ignored for 2 KB windows (mask with `& !1`); they are **independent** registers — do not derive the second 2 KB window from R0 by setting bit 0.

### 4.4 PRG Mapping

PRG mode = 0 (bit 6 of $8000 = 0):

| CPU address | Bank |
|-------------|------|
| $8000–$9FFF | R6 (8 KB) |
| $A000–$BFFF | R7 (8 KB) |
| $C000–$DFFF | second-to-last 8 KB bank (fixed) |
| $E000–$FFFF | last 8 KB bank (fixed) |

PRG mode = 1 (bit 6 of $8000 = 1):

| CPU address | Bank |
|-------------|------|
| $8000–$9FFF | second-to-last 8 KB bank (fixed) |
| $A000–$BFFF | R7 (8 KB) |
| $C000–$DFFF | R6 (8 KB) |
| $E000–$FFFF | last 8 KB bank (fixed) |

Address translation for an 8 KB slot:
```
prg_rom[bank * 8192 + (addr & 0x1FFF)]
```

"Last bank" = `(prg_rom.len() / 8192) - 1`. "Second-to-last" = last - 1.

### 4.5 PRG RAM ($6000–$7FFF)

8 KB. Two MMC3 registers govern access:

**$A000 (even) — Mirroring**: bit 0 → 0 = vertical, 1 = horizontal. (Only for cartridges without four-screen VRAM; if flags6 bit 3 is set in the header, ignore this register.)

**$A001 (odd) — PRG RAM protect**:

| Bit | Name | Meaning |
|-----|------|---------|
| 7   | Enable | 0 = PRG RAM disabled (reads open bus) |
| 6   | Write protect | 1 = writes disabled |

Writes: allowed only when bit 7 = 1 and bit 6 = 0.

### 4.6 IRQ Counter

MMC3 generates CPU IRQs by counting rising edges on PPU address bus line A12. On real hardware A12 rises once per scanline during sprite pattern fetches (BG pattern table at $0000, sprite pattern table at $1000): one rising edge per scanline.

**Registers**:

- **$C000 (even) — IRQ latch**: stores the reload value (0–255).
- **$C001 (odd) — IRQ reload**: on the next clock, reload the counter from the latch instead of decrementing.
- **$E000 (even) — IRQ disable**: clear the IRQ enable flag; acknowledge any pending IRQ.
- **$E001 (odd) — IRQ enable**: set the IRQ enable flag.

**Counter behavior** on each clock:
1. If a reload was requested (via $C001) or counter == 0: set counter = IRQ latch value, clear reload flag.
2. Else: counter -= 1.
3. If counter == 0 and IRQ is enabled: assert CPU IRQ line.

**Clocking model — scanline-driven (chosen)**:

A naïve A12-rising-edge detector keyed off every `ppu_read`/`ppu_write` does **not** work with our per-pixel PPU. Because `render_pixel` performs both BG ($0000) and sprite ($1000) pattern reads for every pixel, A12 toggles hundreds of times per scanline rather than once, firehosing spurious IRQ clocks and corrupting any game (e.g. SMB3) that uses MMC3 IRQs to split the screen.

Resolution: clock the IRQ counter **once per visible / pre-render scanline**, driven by the PPU at dot 260 — the canonical sprite-fetch dot where A12 rises on real hardware — and only while rendering is enabled. This is the standard simplified MMC3 IRQ model used by emulators that don't model exact PPU fetch cycles.

The ambiguity (true A12 edges vs. simplified scanline clock) and the chosen resolution are documented inline in `src/cartridge/mapper4.rs` (`clock_scanline_counter`) and `src/ppu/mod.rs` (the dot-260 `notify_scanline` call).

**Bus / PPU integration**: The PPU calls `mapper.notify_scanline()` once per visible/pre-render scanline at dot 260 when rendering is enabled. After each CPU step the bus polls `mapper.poll_irq()`; if it returns `true`, the CPU IRQ line is asserted.

### 4.7 Mapper Trait Extension

Add to the `Mapper` trait in `cartridge/mod.rs`:

```rust
/// Returns true if the mapper is currently asserting a CPU IRQ. Polled by the bus.
fn poll_irq(&mut self) -> bool { false }

/// Called once per visible/pre-render PPU scanline (at dot 260, rendering enabled).
/// Used by mappers (MMC3) that drive their IRQ counter off the PPU scanline clock.
fn notify_scanline(&mut self) {}
```

> Historical note: an earlier draft of this spec proposed a `ppu_notify(addr: u16) -> bool` hook that fired on every PPU address access and detected A12 rising edges in the mapper. That approach was implemented and abandoned — see the rationale above. The `a12_prev` field is retained in `Mapper4` for spec reference but is unused.

Also add `SingleScreenLower` and `SingleScreenUpper` variants to `Mirroring` for MMC1 single-screen modes:

```rust
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SingleScreenLower,
    SingleScreenUpper,
}
```

Update the PPU nametable routing to handle these two new variants (map all four nametable slots to bank A for lower, bank B for upper).

### 4.8 Struct Fields

```rust
pub struct Mapper4 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_ram: bool,
    prg_ram: Vec<u8>,
    prg_ram_enabled: bool,
    prg_ram_write_protect: bool,

    bank_select: u8,    // last $8000 write
    banks: [u8; 8],     // R0–R7

    mirroring: Mirroring,
    header_four_screen: bool,

    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_reload: bool,
    irq_pending: bool,
    a12_prev: bool,     // last A12 state for edge detection
}
```

---

## 5. Cartridge Dispatch

In `cartridge/mod.rs`, add three new module imports and extend the mapper match:

```rust
pub mod mapper1;
pub mod mapper2;
pub mod mapper3;
pub mod mapper4;

// in from_bytes:
let mapper: Box<dyn Mapper> = match header.mapper {
    0 => Box::new(Mapper0::new(&header, prg_rom, chr_rom)?),
    1 => Box::new(Mapper1::new(&header, prg_rom, chr_rom)?),
    2 => Box::new(Mapper2::new(&header, prg_rom, chr_rom)?),
    3 => Box::new(Mapper3::new(&header, prg_rom, chr_rom)?),
    4 => Box::new(Mapper4::new(&header, prg_rom, chr_rom)?),
    other => return Err(CartridgeError::UnsupportedMapper(other)),
};
```

Each `new` constructor mirrors the pattern established in `Mapper0::new`: accept `&RomHeader`, `Vec<u8>` PRG, `Vec<u8>` CHR, return `Result<Self, CartridgeError>`.

---

## 6. PPU / Bus Integration for MMC3 IRQ

The MMC3 IRQ counter is clocked from the PPU's scanline schedule rather than from per-access A12 detection (see §4.6).

**PPU side** — in `src/ppu/mod.rs`, inside `tick()`, when rendering is enabled and the current scanline is visible (0–239) or pre-render (261), at `dot == 260` call:

```rust
mapper.notify_scanline();
```

**Bus / main loop side** — after each CPU step, poll the mapper for a pending IRQ and assert the CPU IRQ line:

```rust
if bus.cartridge.mapper.poll_irq() {
    cpu.request_irq();
}
```

`poll_irq()` is edge-consuming: it returns the current pending state and clears it. `$E000` writes to MMC3 acknowledge any pending IRQ inside the mapper.

---

## 7. Module Structure

```
src/
  cartridge/
    mod.rs         # Mapper trait (+ poll_irq, notify_scanline), Mirroring (+ SingleScreen variants), dispatch
    mapper0.rs     # unchanged
    mapper1.rs     # NEW — MMC1
    mapper2.rs     # NEW — UxROM
    mapper3.rs     # NEW — CNROM
    mapper4.rs     # NEW — MMC3
  bus.rs           # expose cartridge so PPU/main can reach the mapper
  ppu/mod.rs       # call mapper.notify_scanline() at dot 260 of visible/pre-render scanlines
  main.rs          # after each CPU step, poll mapper.poll_irq() → cpu.request_irq()
```

---

## 8. Acceptance Criteria

- [ ] `cargo build` succeeds with no warnings on stable Rust.
- [ ] `Mirroring` enum has `SingleScreenLower` and `SingleScreenUpper` variants; PPU nametable routing handles them.
- [ ] `Mapper` trait has `poll_irq` and `notify_scanline` with default no-op implementations.

**Mapper 1 (MMC1)**:
- [ ] 5-bit shift register clocks correctly; reset on bit-7 write.
- [ ] Consecutive-write guard discards writes on back-to-back cycles.
- [ ] All 4 PRG modes switch correctly for a 128 KB PRG ROM (8 × 16 KB banks).
- [ ] CHR mode 0 (8 KB) and mode 1 (dual 4 KB) switch to the correct CHR-ROM offset.
- [ ] Power-on Control register = `0b01100`; mirroring = vertical.
- [ ] Legend of Zelda title screen renders without graphical corruption.

**Mapper 2 (UxROM)**:
- [ ] Writes to $8000–$FFFF change `bank_select`; last bank is always fixed at $C000.
- [ ] CHR-RAM is writable and readable at $0000–$1FFF.
- [ ] Mega Man or Castlevania boots to title screen without hang.

**Mapper 3 (CNROM)**:
- [ ] PRG is fixed (same NROM logic); writes to $8000–$FFFF switch CHR bank.
- [ ] CHR bank select is masked to valid range (`data & 0x03` minimum).
- [ ] Gradius or Balloon Fight shows correct title graphics.

**Mapper 4 (MMC3)**:
- [ ] PRG mode 0 and 1 map $8000/$C000 correctly.
- [ ] CHR mode 0 and 1 map 2 KB / 1 KB slots to correct PPU addresses.
- [ ] $A000 mirroring register switches horizontal/vertical at runtime.
- [ ] PRG RAM disabled when bit 7 of $A001 = 0; write-protected when bit 6 = 1.
- [ ] IRQ counter is clocked once per visible/pre-render scanline at PPU dot 260 via `notify_scanline()`; CPU IRQ fires when counter reaches 0 and IRQ is enabled.
- [ ] IRQ acknowledge ($E000 write) clears pending IRQ.
- [ ] Super Mario Bros 3 boots to title screen and enters gameplay.

**General**:
- [ ] No `unwrap()` in any mapper core path.
- [ ] `nestest.nes` still passes (no regressions on CPU or bus).
- [ ] All four mappers return `Ok(...)` for valid ROMs and `Err(CartridgeError::...)` for malformed inputs.