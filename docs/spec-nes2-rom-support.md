# Spec: NES 2.0 ROM Support

**Goal**: Replace the current `Nes2NotSupported` rejection with a proper NES 2.0 header
parser. Expose extended fields (PRG/CHR sizes including exponent encoding, 12-bit mapper
IDs, submapper, PRG/CHR RAM and NVRAM sizes, CPU/PPU timing) so existing and future
mappers can use them. Pre-existing iNES 1.0 ROMs must continue to load identically.

This is a refinement of [Milestone 2 — Cartridge](spec-milestone-2-cartridge.md). It is
not a new milestone in the project constitution; it's a focused feature spec to remove
a known limitation and make the emulator forward-compatible with modern test ROM dumps.

**Primary references**:
- [NESDev NES 2.0 format](https://www.nesdev.org/wiki/NES_2.0) — full byte-by-byte header layout
- [NESDev NES 2.0 mapper numbers](https://www.nesdev.org/wiki/Mapper) — extended 12-bit mapper space
- [NESDev NES 2.0 submappers](https://www.nesdev.org/wiki/NES_2.0_submappers) — submapper definitions
- [NESDev iNES](https://www.nesdev.org/wiki/INES) — for comparing NES 2.0 against iNES 1.0
- [NESDev CPU PPU timing](https://www.nesdev.org/wiki/Cycle_reference_chart) — region clock rates

**Deferred / out of scope**: Vs. System detection (byte 13 high nibble), Misc ROMs
(byte 14), default expansion device (byte 15), NES 2.0 trainer handling beyond the iNES
1.0 trainer behavior already in place.

---

## 1. Detection

**Source**: [NESDev NES 2.0 — Identification](https://www.nesdev.org/wiki/NES_2.0#Identification)

A ROM is NES 2.0 iff:
1. Bytes 0–3 are the iNES magic `"NES\x1A"` (same as iNES 1.0).
2. `flags7 & 0x0C == 0x08` (bits 3–2 of byte 7 equal `10`).

Any file that fails (1) is invalid. A file that passes (1) but not (2) is iNES 1.0.

The current `from_bytes` in `cartridge/mod.rs` already detects condition (2) and returns
`CartridgeError::Nes2NotSupported`. Replace that branch with the parser below.

---

## 2. Header Layout (16 bytes)

**Source**: [NESDev NES 2.0 — Header](https://www.nesdev.org/wiki/NES_2.0#Header)

| Offset | Field | iNES 1.0 meaning | NES 2.0 meaning |
|--------|-------|------------------|------------------|
| 0–3 | Magic | `"NES\x1A"` | same |
| 4 | PRG-ROM size LSB | banks of 16 KB | low 8 bits of size (units depend on byte 9) |
| 5 | CHR-ROM size LSB | banks of 8 KB | low 8 bits of size (units depend on byte 9) |
| 6 | Flags 6 | mapper low nibble + mirroring + battery + trainer + four-screen | same |
| 7 | Flags 7 | mapper high nibble + NES 2.0 indicator | mapper bits 7–4 + console type (low 2 bits) + format ID (bits 3–2 = `10`) |
| 8 | (padding) | unused | mapper bits 11–8 (low nibble) + submapper (high nibble) |
| 9 | (padding) | unused | PRG/CHR ROM size MSB (low nibble = PRG hi, high nibble = CHR hi) |
| 10 | (padding) | unused | PRG-RAM (low nibble) + PRG-NVRAM (high nibble) shift counts |
| 11 | (padding) | unused | CHR-RAM (low nibble) + CHR-NVRAM (high nibble) shift counts |
| 12 | (padding) | unused | CPU/PPU timing (low 2 bits) |
| 13 | (padding) | unused | Vs. System or Extended Console Type — **out of scope** |
| 14 | (padding) | unused | Miscellaneous ROMs count (low 2 bits) — **out of scope** |
| 15 | (padding) | unused | Default expansion device — **out of scope** |

### Flags 6 (unchanged from iNES 1.0)

```
M M M M  F  T  B  H
7 6 5 4  3  2  1  0
```

| Bit(s) | Meaning |
|--------|---------|
| 7–4 | Mapper number bits 3–0 |
| 3   | Four-screen VRAM (overrides bit 0) |
| 2   | 512-byte trainer present at offset 16 |
| 1   | Battery-backed PRG-RAM at $6000–$7FFF |
| 0   | Mirroring: 0 = horizontal, 1 = vertical |

### Flags 7 (NES 2.0 reuses bits 3–2)

```
M M M M  F F  C C
7 6 5 4  3 2  1 0
```

| Bit(s) | Meaning |
|--------|---------|
| 7–4 | Mapper number bits 7–4 |
| 3–2 | Format ID — `10` = NES 2.0 |
| 1–0 | Console type: 0 = NES/Famicom, 1 = Vs. System, 2 = Playchoice 10, 3 = extended |

For Milestone scope, accept console type `0` only. Reject 1/2/3 with
`CartridgeError::UnsupportedConsoleType(u8)`.

### Byte 8 — mapper high bits + submapper

```
S S S S  M M M M
7 6 5 4  3 2 1 0
```

- Low nibble: mapper number bits 11–8.
- High nibble: submapper number (0–15).

Full mapper ID is the 12-bit value:
```
mapper = ((byte8 & 0x0F) << 8) | (flags7 & 0xF0) | (flags6 >> 4)
submapper = byte8 >> 4
```

### Byte 9 — PRG/CHR ROM size MSB

```
C C C C  P P P P
7 6 5 4  3 2 1 0
```

- Low nibble: PRG-ROM size bits 11–8 (when in linear-size mode).
- High nibble: CHR-ROM size bits 11–8 (when in linear-size mode).

When the high nibble of either size is `$F`, that size switches to **exponent
notation** — see §3.

### Byte 10 — PRG-RAM and PRG-NVRAM

```
N N N N  V V V V
7 6 5 4  3 2 1 0
```

- Low nibble (V): PRG-RAM (volatile) shift count.
- High nibble (N): PRG-NVRAM (battery-backed, non-volatile) shift count.

Size in bytes: `0` if shift count is 0, else `64 << shift`. Minimum non-zero size is
64 bytes; maximum (shift = 15) is 1 MB.

### Byte 11 — CHR-RAM and CHR-NVRAM

Same encoding as byte 10 but for CHR.

```
N N N N  V V V V
7 6 5 4  3 2 1 0
```

### Byte 12 — CPU/PPU timing

Only the low 2 bits are defined:

| Value | Region | Frame rate | PPU scanlines |
|-------|--------|------------|---------------|
| 0 | NTSC NES | 60.0988 Hz | 262 |
| 1 | Licensed PAL NES | 50.0070 Hz | 312 |
| 2 | Multi-region (cartridge works on both) | use NTSC defaults | 262 |
| 3 | Dendy (UMC 6527P clone) | 50.0070 Hz | 312 |

The PPU timing change (262 vs 312 scanlines) is **out of scope for this spec** — record
the timing field in the cartridge metadata, but the PPU continues to use NTSC timing
until a follow-up spec expands the renderer. PAL ROMs will load and may render with
incorrect frame rate / NMI cadence; document this clearly.

---

## 3. ROM Size Encoding

**Source**: [NESDev NES 2.0 — PRG-ROM area](https://www.nesdev.org/wiki/NES_2.0#PRG-ROM_Area)

Two modes, switched per-size by the high nibble of byte 9:

### 3.1 Linear mode (high nibble of byte 9 ≠ `$F` for the corresponding size)

```
prg_units = ((byte9 & 0x0F) << 8) | byte4
chr_units = ((byte9 & 0xF0) << 4) | byte5
prg_size_bytes = prg_units * 16384
chr_size_bytes = chr_units * 8192
```

Maximum representable size: 4095 × 16 KB = 64 MB PRG, 4095 × 8 KB = 32 MB CHR.

### 3.2 Exponent-multiplier mode (high nibble of byte 9 == `$F` for the corresponding size)

When `byte9 & 0x0F == 0x0F`, byte 4 encodes PRG size as:

```
exponent   = (byte4 >> 2) & 0x3F     // 6 bits, 0–63
multiplier = byte4 & 0x03            // 2 bits → odd value 1, 3, 5, 7
prg_size_bytes = (1 << exponent) * (multiplier * 2 + 1)
```

Same logic with byte 5 for CHR when `byte9 & 0xF0 == 0xF0`.

Exponent mode allows any size that's an odd multiple of a power of two (e.g.,
`3 * 2^20 = 3 MB`). Most modern dumps use linear mode; exponent mode mostly appears
in homebrew and unusual cartridge sizes.

**Implementation note**: the exponent can be very large; cap at a sane limit (e.g.,
reject sizes > 256 MB) to avoid `Vec` allocation panics. Use
`CartridgeError::RomTooLarge { kind: PrgOrChr, requested: usize }`.

---

## 4. Mapper and Submapper

The 12-bit mapper number opens up the `$0xx`–`$Fxx` mapper space. For this project:

- Mappers 0–4 already have NES 2.0–compatible behavior planned (see project constitution).
- Mappers 5–4095: dispatch unchanged — return `UnsupportedMapper(u16)`.

Update `CartridgeError::UnsupportedMapper(u8)` to `UnsupportedMapper(u16)` so the new
mapper-number range is representable in errors.

### Submapper

The submapper number distinguishes hardware variants of the same base mapper. Examples:
- Mapper 1 (MMC1) submappers: 1 = SUROM, 2 = SOROM, 3 = SXROM, 5 = SEROM/SHROM/SH1ROM
- Mapper 4 (MMC3) submappers: 0 = MMC3C/MMC6, 1 = MMC3A (no IRQ reload glitch), 4 = MMC3A revision A

For Milestone scope: pass the submapper to the mapper constructor; mappers may consult
or ignore it. Mapper 0 (NROM) ignores submapper entirely — submapper 0 is the only
defined value and any other is undefined behavior on real hardware (treat as 0).

---

## 5. PRG-RAM, CHR-RAM, and NVRAM

iNES 1.0 implies a fixed 8 KB PRG-RAM at `$6000–$7FFF` regardless of cartridge. NES 2.0
spells out the actual sizes. For now:

- **PRG-RAM size** drives the WRAM allocation in the bus (or in the mapper when the
  mapper owns the WRAM region). Update `Bus` to allocate `prg_ram_size` bytes instead
  of a hardcoded `8192`. Default to `8192` if both PRG-RAM and PRG-NVRAM are 0 (legacy
  iNES 1.0 ROMs).
- **PRG-NVRAM size** (battery-backed) is recorded but not yet wired to a save-file
  loader — that's a separate spec. Allocate the bytes alongside PRG-RAM and treat
  them as one combined region for now; `has_battery` from flags6 indicates which
  region is persistent.
- **CHR-RAM size** is meaningful only when `chr_size == 0`. Mapper 0 currently
  hardcodes `[u8; 8192]`. Replace with a `Vec<u8>` of `chr_ram_size` (default 8192
  if unspecified for legacy iNES 1.0 ROMs).
- **CHR-NVRAM size**: same as PRG-NVRAM — record, defer save support.

The 8 KB legacy default applies only to iNES 1.0 ROMs (where the spec doesn't
distinguish). For NES 2.0 ROMs the parsed size is authoritative — including 0 (the
mapper genuinely has no PRG-RAM).

---

## 6. Cartridge Module Changes

### 6.1 New types

In `src/cartridge/mod.rs`:

```rust
pub enum RomFormat {
    INes1,
    Nes2,
}

pub enum CpuTiming {
    Ntsc,    // value 0
    Pal,     // value 1
    Multi,   // value 2 — both
    Dendy,   // value 3
}

pub struct RomHeader {
    pub format: RomFormat,
    pub mapper: u16,
    pub submapper: u8,
    pub prg_rom_size: usize,
    pub chr_rom_size: usize,
    pub prg_ram_size: usize,
    pub prg_nvram_size: usize,
    pub chr_ram_size: usize,
    pub chr_nvram_size: usize,
    pub mirroring: Mirroring,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub timing: CpuTiming,
}
```

### 6.2 Updated error type

```rust
pub enum CartridgeError {
    TooShort,
    BadMagic,
    UnsupportedMapper(u16),                    // was u8 — widen for NES 2.0
    UnsupportedConsoleType(u8),                // new
    RomTooLarge { kind: SizeKind, bytes: usize }, // new
    InvalidSizeEncoding,                       // new — exponent overflow, etc.
}

pub enum SizeKind { Prg, Chr }
```

Remove `Nes2NotSupported` — that branch is the entry point for the new parser.

### 6.3 Parsing flow

```
from_bytes(data):
    validate length >= 16
    validate magic "NES\x1A"
    if (flags7 & 0x0C) == 0x08:
        header = parse_nes2_header(data[0..16])?
    else:
        header = parse_ines1_header(data[0..16])
    if header.format == Nes2 && (flags7 & 0x03) != 0:
        return UnsupportedConsoleType(flags7 & 0x03)
    prg_start = 16 + if header.has_trainer { 512 } else { 0 }
    prg_end = prg_start + header.prg_rom_size
    chr_start = prg_end
    chr_end = chr_start + header.chr_rom_size
    if data.len() < chr_end:
        return TooShort
    prg_rom = data[prg_start..prg_end].to_vec()
    chr_rom = data[chr_start..chr_end].to_vec()  // empty if chr_rom_size == 0
    dispatch_mapper(header, prg_rom, chr_rom)
```

`parse_nes2_header` reads bytes 4–15 per §2 and §3, applying exponent decoding when
needed and the shift-count decoding for RAM sizes:

```
fn ram_size(shift: u8) -> usize {
    if shift == 0 { 0 } else { 64usize << shift }  // 64 << 15 = 2 MB max
}
```

`parse_ines1_header` produces the same `RomHeader` shape with:
- `format = INes1`
- `mapper = u16::from((flags7 & 0xF0) | (flags6 >> 4))`
- `submapper = 0`
- `prg_rom_size = byte4 * 16384`, `chr_rom_size = byte5 * 8192`
- `prg_ram_size = 8192`, `chr_ram_size = if chr_rom_size == 0 { 8192 } else { 0 }`
- `prg_nvram_size = chr_nvram_size = 0`
- `timing = CpuTiming::Ntsc`

This keeps the iNES 1.0 path behaviorally identical to today.

### 6.4 Mapper trait additions

Two new methods, both with default implementations so existing mappers don't break:

```rust
pub trait Mapper {
    // … existing methods …

    /// Submapper number from the iNES 2.0 header (0 for iNES 1.0).
    fn submapper(&self) -> u8 { 0 }

    /// CPU/PPU timing region.
    fn timing(&self) -> CpuTiming { CpuTiming::Ntsc }
}
```

Pass the full `RomHeader` to mapper constructors so they can record what they need:

```rust
impl Mapper0 {
    pub fn new(header: &RomHeader, prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self { … }
}
```

Mapper 0 stores `mirroring`, `submapper`, and `timing` from the header for later
reporting.

---

## 7. Bus Changes

The bus's hardcoded 8 KB WRAM (`wram: [u8; 8192]`) becomes a `Vec<u8>` sized from the
header:

```rust
pub struct Bus {
    ram: [u8; RAM_SIZE],
    wram: Vec<u8>,    // header.prg_ram_size + header.prg_nvram_size; default 8192
    nametable_vram: [u8; 2048],
    pub mapper: Box<dyn Mapper>,
    pub ppu: Ppu,
    pub oam_dma_pending: bool,
    pub oam_dma_page: u8,
}
```

`Bus::new` takes the cartridge factory output (`Box<dyn Mapper>` plus PRG-RAM size)
and allocates accordingly. The simplest signature change:

```rust
pub struct Cartridge {
    pub mapper: Box<dyn Mapper>,
    pub prg_ram_size: usize,
}

pub fn from_bytes(data: &[u8]) -> Result<Cartridge, CartridgeError>
```

`Bus::new(cart: Cartridge)` reads `cart.prg_ram_size` and allocates `vec![0u8;
cart.prg_ram_size.max(1)]` — keep at least 1 byte to avoid empty-slice indexing
quirks, but `read`/`write` on `$6000–$7FFF` should range-check against the configured
size and return open bus for unmapped addresses if PRG-RAM is smaller than 8 KB.

Reads/writes in `$6000–$7FFF` mask address modulo `wram.len()` if PRG-RAM exists:
```
$6000–$7FFF → wram[(addr - $6000) as usize % wram.len()]   when wram.len() > 0
            → 0xFF                                          when wram.len() == 0
```

(Mapper 0 will always have non-zero PRG-RAM by default; this matters for future
mappers that genuinely lack it.)

---

## 8. Mapper 0 Adjustments

Mapper 0's CHR allocation changes from a fixed `[u8; 8192]` to a `Vec<u8>` of
`header.chr_ram_size` bytes when `chr_rom.is_empty()`. Behavior is otherwise
unchanged — CHR-RAM reads and writes use `addr & (chr.len() - 1)` (assumes
power-of-two size; reject non-power-of-two CHR-RAM sizes with
`InvalidSizeEncoding`).

---

## 9. Acceptance Criteria

- [ ] `cargo build` succeeds with no warnings on stable Rust.
- [ ] `RomHeader`, `RomFormat`, `CpuTiming`, and updated `CartridgeError` defined in `cartridge/mod.rs`.
- [ ] `Cartridge` struct (mapper + PRG-RAM size) returned from `from_bytes`; `Bus::new` consumes it.
- [ ] iNES 1.0 path unchanged: every existing iNES 1.0 ROM (including `nestest.nes` and any current SMB1 dumps) loads identically and produces the same ROM data slices.
- [ ] NES 2.0 detection: `flags7 & 0x0C == 0x08` selects the new parser; iNES 1.0 path otherwise.
- [ ] Mapper number is 12 bits: `((byte8 & 0x0F) << 8) | (flags7 & 0xF0) | (flags6 >> 4)`.
- [ ] Submapper read from `byte8 >> 4`; exposed via `Mapper::submapper()` (default `0`).
- [ ] PRG/CHR ROM sizes computed from byte 4/5 + byte 9 high/low nibbles in linear mode.
- [ ] Exponent-multiplier mode triggered when high nibble of byte 9 = `$F` for that size; size = `(1 << ((byte >> 2) & 0x3F)) * ((byte & 0x03) * 2 + 1)`.
- [ ] PRG/CHR RAM and NVRAM sizes computed via `64 << shift` (or 0 when shift is 0) from bytes 10 and 11.
- [ ] `Bus` allocates PRG-RAM as a `Vec<u8>` sized from `header.prg_ram_size + header.prg_nvram_size`; iNES 1.0 ROMs default to 8 KB.
- [ ] CHR-RAM allocation in Mapper 0 honors `header.chr_ram_size`; iNES 1.0 ROMs with `chr_size == 0` default to 8 KB; non-power-of-two CHR-RAM sizes rejected with `InvalidSizeEncoding`.
- [ ] CPU/PPU timing parsed (NTSC / PAL / Multi / Dendy); `Mapper::timing()` exposes it. PPU continues to use NTSC frame timing — document this in the PPU module.
- [ ] Console type (flags7 bits 1–0) other than 0 → `UnsupportedConsoleType(u8)`.
- [ ] Mapper IDs other than 0 (until further milestones land) → `UnsupportedMapper(u16)` carrying the full 12-bit number.
- [ ] PRG/CHR sizes exceeding a configurable cap (recommend 256 MB) → `RomTooLarge { kind, bytes }`.
- [ ] Trainer handling unchanged from iNES 1.0: 512-byte block at offset 16 if flags6 bit 2 set.
- [ ] nestest still passes (regression: `--nestest` mode loads the same iNES 1.0 ROM via the unified path).
- [ ] Loading a known NES 2.0 ROM (e.g., a dumped SMB1 with NES 2.0 header, or a Vs. test ROM) parses without panic and either runs (if mapper supported) or returns `UnsupportedMapper`/`UnsupportedConsoleType`.
- [ ] No `unwrap()` in cartridge, mapper, or bus core paths.

---

## 10. Notes for Future Work

The following are intentionally **not** required by this spec:

- **PAL/Dendy timing**: the PPU stays NTSC. A separate spec should add the 312-scanline frame layout, adjusted VBlank window, and CPU clock divider for PAL.
- **Battery-backed save files**: `prg_nvram_size` is recorded; persisting it to disk between runs is a separate spec.
- **Vs. System / Playchoice 10 console types**: rejected until a Vs.-specific spec lands (it requires DIP switches, a coin slot, and a different palette).
- **Misc ROMs (byte 14)**: rare; only used for cartridges with extra ROM not part of PRG/CHR (e.g., audio sample ROMs). Reject silently for now (count = 0 always assumed).
- **Default expansion device (byte 15)**: no controller-input plumbing yet; ignore.
- **CHR-NVRAM persistence**: same as PRG-NVRAM — record, defer.
