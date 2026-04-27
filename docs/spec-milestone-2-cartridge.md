# Milestone 2 Spec: Cartridge — iNES Parser + Mapper 0

**Goal**: Refactor the bus to own a `Cartridge` struct backed by a `Mapper` trait, implement Mapper 0 (NROM) for both the CPU bus and PPU bus, and expose CHR-ROM so Milestone 3 (PPU) can begin reading pattern tables. The CPU nestest run must continue to pass with no regressions.

**Primary references**:
- [NESDev iNES format](https://www.nesdev.org/wiki/INES) — header layout, field definitions
- [NESDev NROM (Mapper 0)](https://www.nesdev.org/wiki/NROM) — PRG and CHR bank layout
- [NESDev Mapper enumeration](https://www.nesdev.org/wiki/Mapper) — mapper ID encoding in flags6/7
- [NESDev PPU memory map](https://www.nesdev.org/wiki/PPU_memory_map) — CHR window $0000–$1FFF
- [NESDev Mirroring](https://www.nesdev.org/wiki/Mirroring) — nametable mirroring modes

---

## 1. iNES 1.0 Header Format

The iNES header is exactly 16 bytes at offset 0 of every `.nes` ROM file.

| Offset | Size | Field | Description |
|--------|------|-------|-------------|
| 0–3    | 4 B  | Magic | `"NES\x1A"` — identifies the file as iNES |
| 4      | 1 B  | `prg_size` | Number of 16 KB PRG-ROM banks |
| 5      | 1 B  | `chr_size` | Number of 8 KB CHR-ROM banks; 0 = CHR-RAM |
| 6      | 1 B  | Flags 6 | Mapper low nibble, mirroring, battery, trainer |
| 7      | 1 B  | Flags 7 | Mapper high nibble, NES 2.0 indicator |
| 8–15   | 8 B  | Padding | Unused in iNES 1.0; must be zeroed |

### Flags 6 bit layout

```
M M M M F T B H
7 6 5 4 3 2 1 0
```

| Bit(s) | Name | Meaning |
|--------|------|---------|
| 7–4    | Mapper low nibble | Lower 4 bits of mapper number |
| 3      | F (Four-screen) | 1 = cartridge provides its own VRAM; ignore mirroring bit |
| 2      | T (Trainer) | 1 = 512-byte trainer present at offset 16 |
| 1      | B (Battery) | 1 = battery-backed SRAM at $6000–$7FFF |
| 0      | H (Mirror) | 0 = horizontal mirroring, 1 = vertical mirroring |

### Flags 7 bit layout

```
M M M M S S A A
7 6 5 4 3 2 1 0
```

| Bit(s) | Name | Meaning |
|--------|------|---------|
| 7–4    | Mapper high nibble | Upper 4 bits of mapper number |
| 3–2    | NES 2.0 indicator | `10` = NES 2.0 format; otherwise iNES 1.0 |
| 1–0    | Unused | |

**Mapper ID**: `(flags7 & 0xF0) | (flags6 >> 4)`

### NES 2.0 detection

If bits 3–2 of flags7 equal `10` (`flags7 & 0x0C == 0x08`), the file is NES 2.0 format. For Milestone 2, reject NES 2.0 files with `CartridgeError::Nes2NotSupported`. All ROMs needed through Milestone 6 are iNES 1.0.

### Trainer

If flags6 bit 3 is set, a 512-byte trainer block follows the header (bytes 16–527). Skip it when computing PRG-ROM start. The trainer bytes are irrelevant to emulation.

### CHR-RAM

If `chr_size == 0`, the cartridge has no CHR-ROM — it uses 8 KB of writable CHR-RAM. Mapper 0 supports this: allocate `[u8; 8192]` inside the mapper and allow PPU writes to $0000–$1FFF to land there.

---

## 2. Mapper Trait

Define in `src/cartridge/mod.rs`:

```rust
pub trait Mapper {
    /// CPU bus read for $4020–$FFFF. Returns Some(value) if handled, None = open bus.
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    /// CPU bus write for $4020–$FFFF.
    fn cpu_write(&mut self, addr: u16, val: u8);
    /// PPU bus read for $0000–$1FFF (pattern tables).
    fn ppu_read(&self, addr: u16) -> Option<u8>;
    /// PPU bus write for $0000–$1FFF.
    fn ppu_write(&mut self, addr: u16, val: u8);
    /// Nametable mirroring mode encoded in the header.
    fn mirroring(&self) -> Mirroring;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen, // cartridge-provided VRAM; flags6 bit 3. Deferred — no mapper in scope uses it yet.
}
```

The `Bus` holds a `Box<dyn Mapper>`. `Cartridge::from_bytes` parses the header and returns `Box<dyn Mapper>` (dispatching on mapper ID). The `Bus` no longer holds a raw `prg_rom` field.

---

## 3. Mapper 0 — NROM

**Source**: [NESDev NROM](https://www.nesdev.org/wiki/NROM)

NROM has no bank switching. PRG and CHR are fixed windows.

### 3.1 CPU bus ($8000–$FFFF)

| Address range | Mapped to |
|---------------|-----------|
| $8000–$BFFF   | PRG-ROM bank 0 (first 16 KB) |
| $C000–$FFFF   | PRG-ROM bank 1 if 32 KB; mirror of bank 0 if 16 KB (NROM-128) |

Address translation:
```
offset = (addr - 0x8000) as usize % prg_rom.len()
value  = prg_rom[offset]
```

`% prg_rom.len()` handles both NROM-128 (`len == 0x4000`) and NROM-256 (`len == 0x8000`) without branching. CPU writes to $8000–$FFFF are silently ignored.

### 3.2 PRG-RAM ($6000–$7FFF)

The `Bus` continues to own and manage WRAM independently. The mapper does not handle $6000–$7FFF.

### 3.3 PPU bus ($0000–$1FFF)

| Address range | Mapped to |
|---------------|-----------|
| $0000–$0FFF   | CHR bank 0 (first 4 KB) |
| $1000–$1FFF   | CHR bank 1 (second 4 KB) |

**CHR-ROM**: `chr_rom[(addr & 0x1FFF) as usize]`

**CHR-RAM** (when `chr_size == 0`): reads and writes go to an internal `[u8; 8192]`. Writes: `chr_ram[(addr & 0x1FFF) as usize] = val`.

PPU writes to CHR-ROM are silently ignored.

### 3.4 Mirroring

flags6 bit 0: `0` → `Mirroring::Horizontal`, `1` → `Mirroring::Vertical`. Stored in the mapper; returned from `mirroring()`.

---

## 4. Nametable Mirroring Reference

Nametable routing is implemented in Milestone 3, but driven by `mapper.mirroring()`. Documented here for context.

The PPU address space has four 1 KB nametable slots ($2000, $2400, $2800, $2C00), but the NES has only 2 KB of physical VRAM. Two slots always alias two others:

| Mode       | NT0 ($2000) | NT1 ($2400) | NT2 ($2800) | NT3 ($2C00) | Scroll axis |
|------------|-------------|-------------|-------------|-------------|-------------|
| Horizontal | bank A      | bank A      | bank B      | bank B      | Vertical    |
| Vertical   | bank A      | bank B      | bank A      | bank B      | Horizontal  |

No nametable code is required for Milestone 2.

---

## 5. Bus Refactor

Replace `Bus::new(prg_rom: Vec<u8>)` with `Bus::new(mapper: Box<dyn Mapper>)`.

### New Bus struct

```rust
pub struct Bus {
    ram:    [u8; 2048],
    wram:   [u8; 8192],
    mapper: Box<dyn Mapper>,
}
```

### CPU bus read routing

| Address range | Handler |
|---------------|---------|
| $0000–$1FFF   | `ram[(addr & 0x07FF) as usize]` |
| $2000–$3FFF   | PPU registers stub → 0xFF |
| $4000–$4017   | APU/IO stub → 0xFF |
| $4018–$5FFF   | Open bus → 0xFF |
| $6000–$7FFF   | `wram[(addr - 0x6000) as usize]` |
| $8000–$FFFF   | `mapper.cpu_read(addr).unwrap_or(0xFF)` |

### CPU bus write routing

| Address range | Handler |
|---------------|---------|
| $0000–$1FFF   | `ram[(addr & 0x07FF) as usize] = val` |
| $2000–$3FFF   | PPU registers stub → ignore |
| $4000–$4017   | APU/IO stub → ignore |
| $4018–$5FFF   | Open bus → ignore |
| $6000–$7FFF   | `wram[(addr - 0x6000) as usize] = val` |
| $8000–$FFFF   | `mapper.cpu_write(addr, val)` |

### PPU bus methods (stub — plumbed now, called in Milestone 3)

```rust
pub fn ppu_read(&self, addr: u16) -> u8
pub fn ppu_write(&mut self, addr: u16, val: u8)
```

| Address range | Handler |
|---------------|---------|
| $0000–$1FFF   | `mapper.ppu_read(addr).unwrap_or(0)` / `mapper.ppu_write(addr, val)` |
| $2000–$3EFF   | Nametable stub → 0 (Milestone 3) |
| $3F00–$3FFF   | Palette stub → 0 (Milestone 3) |

---

## 6. Cartridge Factory

`from_bytes` becomes a free function (or associated function) returning `Box<dyn Mapper>`:

```rust
pub fn from_bytes(data: &[u8]) -> Result<Box<dyn Mapper>, CartridgeError>
```

Steps:
1. Check `data.len() >= 16` and magic bytes `"NES\x1A"`.
2. Detect NES 2.0: if `flags7 & 0x0C == 0x08` → `Err(CartridgeError::Nes2NotSupported)`.
3. Extract `prg_banks`, `chr_banks`, `flags6`, `flags7`.
4. Compute `mapper_id = (flags7 & 0xF0) | (flags6 >> 4)`.
5. Compute `has_trainer = flags6 & 0x04 != 0`, `prg_start = 16 + if has_trainer { 512 } else { 0 }`.
6. Compute `prg_end = prg_start + prg_banks as usize * 16384`. Check `prg_end <= data.len()` → `Err(CartridgeError::TooShort)`. Slice `data[prg_start..prg_end]`.
7. If `chr_banks > 0`: compute `chr_start = prg_end`, `chr_end = chr_start + chr_banks as usize * 8192`. Check `chr_end <= data.len()` → `Err(CartridgeError::TooShort)`. Slice `data[chr_start..chr_end]`.
8. Parse mirroring: `if flags6 & 0x08 != 0 { Mirroring::FourScreen } else if flags6 & 0x01 != 0 { Mirroring::Vertical } else { Mirroring::Horizontal }`.
9. Dispatch on `mapper_id`:
   - `0` → `Ok(Box::new(Mapper0::new(prg_rom, chr_data, mirroring)))`
   - other → `Err(CartridgeError::UnsupportedMapper(mapper_id))`

Updated `CartridgeError`:

```rust
pub enum CartridgeError {
    TooShort,
    BadMagic,
    Nes2NotSupported,
    UnsupportedMapper(u8),
}
```

---

## 7. Module Structure

```
src/
  main.rs              # from_bytes → Bus::new(mapper) → Cpu; no structural change
  bus.rs               # CPU + PPU routing; owns Box<dyn Mapper>, ram, wram
  cartridge/
    mod.rs             # Mapper trait, Mirroring enum, CartridgeError, from_bytes
    mapper0.rs         # Mapper0 struct implementing Mapper
  cpu/
    mod.rs
    opcodes.rs
    addressing.rs
```

`main.rs` change: replace `Cartridge::from_bytes(&data)?.prg_rom` with `cartridge::from_bytes(&data)?` passed directly into `Bus::new`.

---

## 8. Acceptance Criteria

- [ ] `cargo build` succeeds with no warnings on stable Rust.
- [ ] `Mapper` trait and `Mirroring` enum defined in `cartridge/mod.rs`.
- [ ] `Mapper0` in `cartridge/mapper0.rs` implements `Mapper`.
- [ ] `Mapper0::mirroring()` returns the correct mode from the iNES header.
- [ ] `Bus` owns `Box<dyn Mapper>`; CPU reads to $8000–$FFFF route through `mapper.cpu_read`.
- [ ] `Bus::ppu_read` and `Bus::ppu_write` implemented; $0000–$1FFF routes through mapper.
- [ ] NROM-128 (16 KB PRG) mirrors correctly into $8000–$FFFF.
- [ ] NROM-256 (32 KB PRG) maps correctly without mirroring.
- [ ] CHR-ROM reads return the correct byte at a given PPU address.
- [ ] CHR-RAM (chr_size == 0): 8 KB allocated; `ppu_write` stores, `ppu_read` returns it.
- [ ] NES 2.0 files rejected with `CartridgeError::Nes2NotSupported`.
- [ ] nestest.nes still passes: log matches `nestest.log` for all 8991 lines, `$02` = `$00`, `$03` = `$00`.
- [ ] No `unwrap()` in cartridge, mapper, or bus core paths.
