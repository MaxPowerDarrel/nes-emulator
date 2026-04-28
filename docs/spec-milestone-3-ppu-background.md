# Milestone 3 Spec: PPU — Render a Static Background Frame

**Goal**: Implement the PPU module, wire it into the master clock and CPU bus, render background tiles to a `winit`+`pixels` window, and demonstrate a static background frame from a real ROM.

Sprites, NMI, and mid-scanline effects are deferred to Milestone 4.

**Primary references**:
- [NESDev PPU overview](https://www.nesdev.org/wiki/PPU) — high-level architecture
- [NESDev PPU registers](https://www.nesdev.org/wiki/PPU_registers) — CPU-visible register semantics
- [NESDev PPU memory map](https://www.nesdev.org/wiki/PPU_memory_map) — nametable and palette layout
- [NESDev PPU scrolling (Loopy)](https://www.nesdev.org/wiki/PPU_scrolling) — v/t/x/w internal register behavior
- [NESDev PPU rendering](https://www.nesdev.org/wiki/PPU_rendering) — scanline/dot timing
- [NESDev PPU palettes](https://www.nesdev.org/wiki/PPU_palettes) — color index → RGB table
- [NESDev Nametable mirroring](https://www.nesdev.org/wiki/Mirroring) — physical VRAM layout

---

## 1. Architecture Overview

The PPU runs at **3× the CPU clock rate** — every master clock tick advances the PPU by one dot. The master clock loop drives this interleaving:

```
loop {
    ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());  // every master cycle
    ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
    ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
    cpu.step(&mut bus);                                   // every 3 master cycles
}
```

The PPU produces a 256×240 pixel frame at ~60 Hz. Each frame has 262 scanlines × 341 dots. The CPU interacts with the PPU through 8 memory-mapped registers at `$2000–$2007` (mirrored across `$2000–$3FFF`) and through VRAM via `PPUADDR`/`PPUDATA`.

---

## 2. PPU Frame Timing

**Source**: [NESDev PPU rendering](https://www.nesdev.org/wiki/PPU_rendering)

A full frame is 262 scanlines × 341 dots = 89,342 PPU dots (≈ 29,780.5 CPU cycles).

| Scanline range | Type | Description |
|----------------|------|-------------|
| 0–239 | Visible | Background and sprite pixels rendered |
| 240 | Post-render | Idle scanline |
| 241–260 | VBlank | PPUSTATUS VBlank flag set on dot 1 of scanline 241; NMI fires here (Milestone 4) |
| 261 (−1) | Pre-render | Clears VBlank flag, sprite overflow, sprite-zero-hit on dot 1 |

Dot 0 of every scanline is an idle dot. Visible scanlines render pixels during dots 1–256.

### NTSC quirk: odd-frame dot skip

On odd frames (when rendering is enabled), the pre-render scanline is one dot shorter — the PPU jumps directly from `(scanline 261, dot 339)` to `(scanline 0, dot 0)` of the next frame, skipping what would have been `(scanline 261, dot 340)`. Track this with a frame-parity counter. **Required for accurate NMI timing in Milestone 4; implement the counter now.**

Source: [NESDev PPU frame timing](https://www.nesdev.org/wiki/PPU_frame_timing).

---

## 3. PPU Registers (CPU-Visible)

**Source**: [NESDev PPU registers](https://www.nesdev.org/wiki/PPU_registers)

The CPU sees 8 PPU registers at `$2000–$2007`, mirrored every 8 bytes across `$2000–$3FFF`.

### $2000 — PPUCTRL (write)

```
V P H B S I NN
7 6 5 4 3 2 10
```

| Bit(s) | Name | Meaning |
|--------|------|---------|
| 7 | V (NMI enable) | 1 = generate NMI at VBlank start (Milestone 4 — store the bit now) |
| 6 | P (PPU master/slave) | Ignored on NES hardware |
| 5 | H (sprite height) | 0 = 8×8 sprites, 1 = 8×16 sprites (Milestone 4) |
| 4 | B (BG pattern table) | 0 = $0000, 1 = $1000 |
| 3 | S (sprite pattern table) | 0 = $0000, 1 = $1000 (Milestone 4) |
| 2 | I (VRAM increment) | 0 = +1 (horizontal), 1 = +32 (vertical) |
| 1–0 | NN (base nametable) | 0=$2000, 1=$2400, 2=$2800, 3=$2C00; written into `t` bits 11–10 |

**Side effect on write**: bits 1–0 are copied into bits 11–10 of the `t` register (see §6).

### $2001 — PPUMASK (write)

```
B G R s b M m g
7 6 5 4 3 2 1 0
```

| Bit(s) | Name | Meaning |
|--------|------|---------|
| 7–5 | B,G,R | Color emphasis (ignore for Milestone 3) |
| 4 | s (show sprites) | Milestone 4 |
| 3 | b (show background) | 1 = render background tiles |
| 2 | M (sprites in left 8px) | Milestone 4 |
| 1 | m (BG in left 8px) | 0 = hide background in leftmost 8 pixels |
| 0 | g (greyscale) | Ignore for Milestone 3 |

### $2002 — PPUSTATUS (read)

```
V S O x x x x x
7 6 5 4 3 2 1 0
```

| Bit(s) | Name | Meaning |
|--------|------|---------|
| 7 | V (VBlank) | Set on dot 1 of scanline 241; cleared on pre-render scanline dot 1 |
| 6 | S (sprite zero hit) | Milestone 4 |
| 5 | O (sprite overflow) | Milestone 4 |
| 4–0 | — | Open bus (return low 5 bits of last PPU bus write) |

**Side effects on read**:
1. Clear VBlank flag (bit 7) immediately after reading.
2. Reset write-latch `w` to 0.

### $2003 — OAMADDR (write) / $2004 — OAMDATA (read/write)

OAM (sprite) registers. Store OAMADDR and OAMDATA writes for completeness; do not use them for rendering until Milestone 4. Reads from $2004 return `oam[oam_addr as usize]` — some games poll this during VBlank setup.

### $2005 — PPUSCROLL (write, twice)

Two consecutive writes using the `w` latch (cleared by PPUSTATUS read):

- First write (`w = 0`): fine X scroll into `x`; coarse X scroll into `t` bits 4–0. Set `w = 1`.
- Second write (`w = 1`): fine Y scroll into `t` bits 14–12; coarse Y scroll into `t` bits 9–5. Set `w = 0`.

**Full bit assignment** (see §6 for the `t` register layout):
```
First write:
  x  ← data[2:0]
  t[4:0] ← data[7:3]   (coarse X)

Second write:
  t[14:12] ← data[2:0]  (fine Y)
  t[9:5]   ← data[7:3]  (coarse Y)
```

### $2006 — PPUADDR (write, twice)

Two consecutive writes using the `w` latch:

- First write (`w = 0`): high 6 bits of address into `t` bits 13–8; bit 14 of `t` cleared. Set `w = 1`.
- Second write (`w = 1`): low 8 bits into `t` bits 7–0; copy `t` → `v`. Set `w = 0`.

```
First write:
  t[13:8]  ← data[5:0]
  t[14]    ← 0

Second write:
  t[7:0]   ← data[7:0]
  v        ← t
```

### $2007 — PPUDATA (read/write)

Reads and writes to VRAM at address `v` (as mapped through the PPU bus). After each access, increment `v` by 1 (PPUCTRL bit 2 = 0) or 32 (bit 2 = 1).

**Read buffer**: Reads from `$0000–$3EFF` return the *buffered* value from the previous read, then fill the buffer with the current address's data. Reads from `$3F00–$3FFF` (palette) return data immediately (buffer is still updated with the nametable byte at `v & $2FFF`). The read buffer initializes to `0x00` on power-on; the first PPUDATA read after reset will return 0x00 and advance `v`.

---

## 4. PPU Memory Map

**Source**: [NESDev PPU memory map](https://www.nesdev.org/wiki/PPU_memory_map)

| Address range | Size | Contents |
|---------------|------|----------|
| $0000–$0FFF | 4 KB | Pattern table 0 — routed through `mapper.ppu_read` (CHR-ROM/RAM) |
| $1000–$1FFF | 4 KB | Pattern table 1 — routed through `mapper.ppu_read` |
| $2000–$23FF | 1 KB | Nametable 0 |
| $2400–$27FF | 1 KB | Nametable 1 |
| $2800–$2BFF | 1 KB | Nametable 2 |
| $2C00–$2FFF | 1 KB | Nametable 3 |
| $3000–$3EFF | — | Mirror of $2000–$2EFF |
| $3F00–$3F1F | 32 B | Palette RAM |
| $3F20–$3FFF | — | Palette mirrors (repeat every $20) |

The PPU address space is 14-bit; the bus stubs both pattern table access and nametable/palette access. All three regions must be live for Milestone 3.

### Nametable VRAM

The NES has 2 KB of physical VRAM (internal to the PPU), sufficient for two 1 KB nametables. The four slots ($2000, $2400, $2800, $2C00) are mapped to two physical banks according to `mapper.mirroring()`:

| Mirroring mode | NT slot 0 ($2000) | NT slot 1 ($2400) | NT slot 2 ($2800) | NT slot 3 ($2C00) |
|----------------|--------------------|--------------------|--------------------|---------------------|
| Horizontal | Bank A | Bank A | Bank B | Bank B |
| Vertical | Bank A | Bank B | Bank A | Bank B |

Address translation function (place in `bus.rs`):
```
fn nametable_index(addr: u16, mirroring: Mirroring) -> usize {
    let addr = (addr - 0x2000) & 0x0FFF;  // strip base, fold $3000 mirror
    let table = addr / 0x400;              // 0–3: which nametable slot
    let offset = (addr % 0x400) as usize;
    let bank = match mirroring {
        Horizontal => table / 2,           // 0,0,1,1
        Vertical   => table & 1,           // 0,1,0,1
        FourScreen => unreachable!(),      // requires 4 KB cart VRAM; no Mapper 0/1/2 ROM uses this
    };
    bank * 0x400 + offset
}
```

Allocate `[u8; 2048]` for nametable VRAM in the `Bus` struct.

### Palette RAM

32 bytes stored in the PPU (owned by `Ppu` directly, not the bus, since it is entirely internal).

| Offset | Contents |
|--------|----------|
| $00 | Universal background color |
| $01–$03 | BG palette 0 |
| $04 | Mirror of $00 |
| $05–$07 | BG palette 1 |
| $08 | Mirror of $00 |
| $09–$0B | BG palette 2 |
| $0C | Mirror of $00 |
| $0D–$0F | BG palette 3 |
| $10 | Mirror of $00 |
| $11–$1F | Sprite palettes 0–3 (Milestone 4) |

Address $3F10, $3F14, $3F18, $3F1C mirror $3F00, $3F04, $3F08, $3F0C (sprite palette 0 color 0 = BG color).

Palette address masking: `palette_ram[(addr & 0x1F) as usize]`. Additionally handle the sprite-transparent mirrors by mapping `0x10, 0x14, 0x18, 0x1C → 0x00, 0x04, 0x08, 0x0C` after the `& 0x1F` mask:
```
fn palette_addr(addr: u16) -> usize {
    let idx = (addr & 0x1F) as usize;
    match idx {
        0x10 | 0x14 | 0x18 | 0x1C => idx & 0x0F,
        _ => idx,
    }
}
```

### Bus PPU memory routing

Extend `Bus::ppu_read` and `Bus::ppu_write` (currently stubbed):

```
$0000–$1FFF  →  mapper.ppu_read / mapper.ppu_write
$2000–$3EFF  →  nametable_vram[nametable_index(addr, mapper.mirroring())]
$3F00–$3FFF  →  palette_ram[palette_addr(addr)]
```

---

## 5. Internal PPU State

### Loopy scrolling registers

**Source**: [NESDev PPU scrolling](https://www.nesdev.org/wiki/PPU_scrolling)

| Register | Width | Description |
|----------|-------|-------------|
| `v` | 15 bits | Current VRAM address (and scroll position during rendering) |
| `t` | 15 bits | Temporary VRAM address; "top-left corner of the screen" |
| `x` | 3 bits | Fine X scroll (0–7) |
| `w` | 1 bit | Write latch for $2005/$2006 |

Bit layout of `v` and `t`:
```
yyy NN YYYYY XXXXX
14  13 12 11 10  9   8   7   6   5   4   3   2   1   0
fine_y  NT_y  NT_x       coarse_Y               coarse_X
```

| Bits | Name | Notes |
|------|------|-------|
| 14–12 | Fine Y scroll | Sub-tile vertical pixel (0–7) |
| 11 | Nametable Y select | 0 = top screen ($2000/$2400), 1 = bottom screen ($2800/$2C00) |
| 10 | Nametable X select | 0 = left screen ($2000/$2800), 1 = right screen ($2400/$2C00) |
| 9–5 | Coarse Y scroll | Tile row 0–29 |
| 4–0 | Coarse X scroll | Tile column 0–31 |

### Other PPU state

| Field | Type | Description |
|-------|------|-------------|
| `scanline` | i16 | Current scanline (-1 / 0–261 depending on convention; use 0–261 with 261 = pre-render) |
| `dot` | u16 | Current dot within scanline (0–340) |
| `odd_frame` | bool | Frame parity for NTSC dot-skip |
| `nmi_output` | bool | PPUCTRL bit 7 (NMI enable); store now, wire in Milestone 4 |
| `nmi_occurred` | bool | VBlank flag in PPUSTATUS; cleared on PPUSTATUS read |
| `ppuctrl` | u8 | Latched PPUCTRL byte |
| `ppumask` | u8 | Latched PPUMASK byte |
| `oam_addr` | u8 | OAMADDR ($2003) |
| `oam` | [u8; 256] | OAM memory (sprite data; Milestone 4) |
| `palette_ram` | [u8; 32] | Palette RAM |
| `read_buffer` | u8 | PPUDATA read buffer; initialized to 0x00 |
| `open_bus` | u8 | Full byte of last PPU register write; bits 4–0 are returned in PPUSTATUS reads |
| `framebuffer` | [u8; 256×240×4] | RGBA pixel output; written by `render_pixel`, copied to `pixels` each frame |
| `frame_complete` | bool | Set at VBlank start (scanline 241 dot 1); cleared by main loop after render |

---

## 6. Background Rendering

**Source**: [NESDev PPU rendering](https://www.nesdev.org/wiki/PPU_rendering)

For Milestone 3 rendering strategy: generate each pixel during the visible scanline at the appropriate dot. This is not tile-fetch-cycle-accurate (that level is Milestone 4), but the *pixel output* must be correct.

### Pattern table tile format

Each tile is 16 bytes in CHR memory: 8 bytes low plane followed by 8 bytes high plane.

```
Tile byte at row y:
  low_byte  = ppu_read(pattern_table_base + tile_id * 16 + y)
  high_byte = ppu_read(pattern_table_base + tile_id * 16 + y + 8)
```

For pixel column x (0 = leftmost), the 2-bit color index is:
```
bit_pos   = 7 - x              // MSB is leftmost pixel
low_bit   = (low_byte  >> bit_pos) & 1
high_bit  = (high_byte >> bit_pos) & 1
color_idx = (high_bit << 1) | low_bit  // 0–3
```

`color_idx == 0` means transparent (use universal background color).

### Nametable and attribute table fetch

For tile at nametable column `coarse_x` (0–31) and row `coarse_y` (0–29):

**Nametable fetch** (tile ID):
```
nt_addr   = 0x2000 | (v & 0x0FFF)
tile_id   = ppu_read(nt_addr)
```

**Attribute table fetch** (palette select):
```
at_addr = 0x23C0
        | (v & 0x0C00)          // nametable select bits
        | ((v >> 4) & 0x38)     // coarse_Y / 4, shifted to bits 5–3
        | ((v >> 2) & 0x07)     // coarse_X / 4, shifted to bits 2–0
attr_byte = ppu_read(at_addr)
```

The attribute byte covers a 4×4 tile area divided into four 2×2 quadrants. Each quadrant is 2 bits of the attribute byte:

```
Quadrant (bit position in attr_byte):
  (coarse_X / 2) & 1 → x-quadrant (0=left, 1=right)
  (coarse_Y / 2) & 1 → y-quadrant (0=top, 1=bottom)
  shift = (y_quadrant * 2 + x_quadrant) * 2
  palette_select = (attr_byte >> shift) & 0x03  // 0–3
```

### Palette lookup

```
if color_idx == 0 {
    pixel_color_index = palette_ram[0]        // universal BG color
} else {
    palette_offset = palette_select * 4 + color_idx  // 1–15
    pixel_color_index = palette_ram[palette_offset]
}
```

`pixel_color_index` is a 6-bit value (0–63) into the NES hardware RGB table (see §7).

### Left-8-pixel masking

When PPUMASK bit 1 (`m`) is 0, pixels at screen X = 0–7 use the universal background color ($3F00) regardless of tile content. This is commonly used by games to hide garbage tiles at the left edge during horizontal scrolling.

### Rendering is disabled

When PPUMASK bit 3 (`b`) is 0, output the universal background color for every pixel (but still advance dot/scanline counters and set VBlank).

---

## 7. NES Hardware Palette (RGB Lookup Table)

**Source**: [NESDev PPU palettes](https://www.nesdev.org/wiki/PPU_palettes)

The 64-entry NES hardware palette maps 6-bit palette indices to RGB values. Use the "2C02" palette from NESDev (one widely accepted standard):

```rust
pub const NES_PALETTE: [(u8, u8, u8); 64] = [
    (0x62, 0x62, 0x62), (0x00, 0x1F, 0xB2), (0x24, 0x04, 0xC8), (0x52, 0x00, 0xB2),
    (0x73, 0x00, 0x76), (0x80, 0x00, 0x24), (0x73, 0x0B, 0x00), (0x52, 0x28, 0x00),
    (0x24, 0x44, 0x00), (0x00, 0x57, 0x00), (0x00, 0x5C, 0x00), (0x00, 0x53, 0x24),
    (0x00, 0x3C, 0x76), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
    (0xAB, 0xAB, 0xAB), (0x0D, 0x57, 0xFF), (0x4B, 0x30, 0xFF), (0x8A, 0x13, 0xFF),
    (0xBC, 0x08, 0xD6), (0xD2, 0x12, 0x69), (0xC7, 0x2E, 0x00), (0x9D, 0x54, 0x00),
    (0x60, 0x7B, 0x00), (0x20, 0x98, 0x00), (0x00, 0xA3, 0x00), (0x00, 0x99, 0x4E),
    (0x00, 0x7E, 0xC4), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
    (0xFF, 0xFF, 0xFF), (0x53, 0xAE, 0xFF), (0x90, 0x85, 0xFF), (0xD3, 0x65, 0xFF),
    (0xFF, 0x57, 0xFF), (0xFF, 0x5D, 0xCF), (0xFF, 0x77, 0x57), (0xFA, 0x9E, 0x00),
    (0xBD, 0xC7, 0x00), (0x7A, 0xE7, 0x00), (0x43, 0xF6, 0x11), (0x26, 0xEF, 0x7E),
    (0x2C, 0xD5, 0xF6), (0x4E, 0x4E, 0x4E), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
    (0xFF, 0xFF, 0xFF), (0xB6, 0xE1, 0xFF), (0xCE, 0xD1, 0xFF), (0xE9, 0xC3, 0xFF),
    (0xFF, 0xBC, 0xFF), (0xFF, 0xBD, 0xF4), (0xFF, 0xC6, 0xC3), (0xFF, 0xD5, 0x9A),
    (0xE9, 0xE6, 0x81), (0xCE, 0xF4, 0x81), (0xB6, 0xFB, 0x9A), (0xA9, 0xFA, 0xC3),
    (0xA9, 0xF0, 0xF4), (0xB8, 0xB8, 0xB8), (0x00, 0x00, 0x00), (0x00, 0x00, 0x00),
];
```

To write to the `pixels` framebuffer (RGBA format):
```rust
let (r, g, b) = NES_PALETTE[color_index as usize & 0x3F];
let pixel_offset = (scanline * 256 + x) * 4;
framebuffer[pixel_offset]     = r;
framebuffer[pixel_offset + 1] = g;
framebuffer[pixel_offset + 2] = b;
framebuffer[pixel_offset + 3] = 0xFF;
```

---

## 8. Window and Framebuffer (winit + pixels)

**Source**: `winit` and `pixels` crate documentation.

Add to `Cargo.toml`:
```toml
winit  = "0.29"
pixels = "0.14"
```

Pin to these specific versions. **Important**: winit 0.30 replaced `WindowBuilder` and the closure-based `EventLoop::run` with an `ApplicationHandler` trait — the pseudocode below uses the 0.29 API. Check crates.io to confirm 0.29 is still published; if 0.30+ is required, the event loop must be rewritten using `ApplicationHandler::window_event`.

### Window setup

Create a 256×240 window (native NES resolution). Scale it up by 3× for visibility (768×720 logical). The `pixels` framebuffer is always 256×240 — `winit` handles the scaling.

```rust
let event_loop = EventLoop::new()?;
let window = WindowBuilder::new()
    .with_title("NES")
    .with_inner_size(LogicalSize::new(256 * 3, 240 * 3))
    .build(&event_loop)?;

let mut pixels = {
    let window_size = window.inner_size();
    let surface_texture = SurfaceTexture::new(window_size.width, window_size.height, &window);
    Pixels::new(256, 240, surface_texture)?
};
```

### Event loop and master clock

The event loop drives the master clock. Run CPU+PPU until one full frame is ready (341 × 262 = 89,342 PPU dots), then call `pixels.render()` and request a redraw.

```
EventLoop::run → on RedrawRequested:
    // run master clock until frame_complete flag set
    while !bus.ppu.frame_complete {
        bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());  // 3×
        bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
        bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
        cpu.step(&mut bus);
    }
    bus.ppu.frame_complete = false;
    // copy ppu.framebuffer into pixels
    pixels.frame_mut().copy_from_slice(&bus.ppu.framebuffer);
    pixels.render()?;
    window.request_redraw();
```

The `frame_complete` flag is set by the PPU when it advances past dot 1 of scanline 241 (VBlank start) — the frame pixels are fully rendered at that point.

**Error type**: `main` should return `Result<(), Box<dyn std::error::Error>>`. Use `?` throughout; no `unwrap()` in the main loop.

---

## 9. CPU Bus Integration

The `Bus` must now route `$2000–$3FFF` to the PPU registers instead of the current stub (which returns 0 / ignores writes).

**Ownership model**: `Bus` owns `Ppu` as a field (`bus.ppu`). The main loop ticks the PPU and the CPU register routing both go through `bus`.

**Borrow-checker resolution**: `Ppu::tick` must read CHR data (from `mapper`) and nametable VRAM (from `bus.nametable_vram`) while `bus.ppu` is mutably borrowed. Rust allows simultaneous mutable and immutable borrows of *different fields* of the same struct, so passing those fields explicitly avoids the conflict:

```rust
// In Ppu:
pub fn tick(&mut self, nametable_vram: &[u8; 2048], mapper: &dyn Mapper) { ... }

// In main loop — Rust split-borrows bus.ppu (mut) from bus.nametable_vram and bus.mapper (shared):
bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
```

Do **not** pass `&mut Bus` into `tick` — that would borrow all of `bus` mutably, conflicting with the existing `&mut bus.ppu` borrow.

- CPU register reads/writes (`$2000–$2007`) route through `bus.ppu.register_read(reg)` / `bus.ppu.register_write(reg, val)`.

Nametable VRAM (`[u8; 2048]`) is owned by `Bus`. PPU reads/writes to `$2000–$3EFF` call into `Bus::ppu_read` / `Bus::ppu_write`, which route through the nametable translation function.

Palette RAM (`[u8; 32]`) is owned by `Ppu` directly since it is entirely internal to the PPU chip.

### CPU bus read routing update

| Address range | Handler |
|---------------|---------|
| $0000–$1FFF | `ram[(addr & 0x07FF)]` |
| $2000–$3FFF | `ppu.register_read((addr & 0x07) as u8)` |
| $4000–$4017 | APU/IO stub → 0xFF |
| $4018–$5FFF | Open bus → 0xFF |
| $6000–$7FFF | `wram[(addr - 0x6000)]` |
| $8000–$FFFF | `mapper.cpu_read(addr).unwrap_or(0xFF)` |

### CPU bus write routing update

| Address range | Handler |
|---------------|---------|
| $0000–$1FFF | `ram[(addr & 0x07FF)] = val` |
| $2000–$3FFF | `ppu.register_write((addr & 0x07) as u8, val)` |
| $4000–$4017 | APU/IO stub (ignore) |
| $4018–$5FFF | Open bus (ignore) |
| $6000–$7FFF | `wram[(addr - 0x6000)] = val` |
| $8000–$FFFF | `mapper.cpu_write(addr, val)` |

---

## 10. Ppu::tick Behavior

```rust
pub fn tick(&mut self, nametable_vram: &[u8; 2048], mapper: &dyn Mapper)
```

`Ppu::tick` advances `dot` by 1, wrapping at the end of each scanline. The full frame is 262 scanlines × 341 dots, with a dot-0 skip on odd frames when rendering is enabled.

```
tick(nametable_vram, mapper):
  dot += 1
  if dot == 341 (or 340 on the pre-render scanline of an odd frame when rendering enabled —
                 i.e. jump from (339, 261) directly to (0, 0)):
    dot = 0
    scanline += 1
    if scanline == 262:
      scanline = 0
      odd_frame = !odd_frame

  // Pre-render scanline: copy t → v so rendering uses the scroll position
  // set by the ROM during VBlank (via PPUADDR/PPUSCROLL writes).
  // This is a simplified Milestone 3 approximation; cycle-accurate
  // per-scanline v updates are deferred to Milestone 4.
  if scanline == 261 && dot == 0:
    v = t

  // VBlank clear (pre-render scanline dot 1)
  if scanline == 261 && dot == 1:
    nmi_occurred = false
    sprite_zero_hit = false   // Milestone 4
    sprite_overflow = false   // Milestone 4

  // Pixel output
  if scanline < 240 && dot >= 1 && dot <= 256 && rendering_enabled():
    render_pixel(scanline, dot - 1, nametable_vram, mapper)

  // VBlank set + frame complete signal (both at scanline 241 dot 1).
  // All visible pixels (scanlines 0–239) are committed at this point,
  // so raising frame_complete here is correct — the main loop can
  // safely copy the framebuffer.
  if scanline == 241 && dot == 1:
    nmi_occurred = true
    frame_complete = true
    // NMI wire-up deferred to Milestone 4
```

`render_pixel(y, x, nametable_vram, mapper)` calls the background rendering pipeline from §6.

---

## 11. Module Structure

```
src/
  main.rs              # Window creation, event loop, master clock
  bus.rs               # CPU + PPU routing; owns Ppu, nametable_vram, wram, mapper
  cpu/
    mod.rs
    opcodes.rs
    addressing.rs
  ppu/
    mod.rs             # Ppu struct, tick(), register_read(), register_write()
    palette.rs         # NES_PALETTE constant (64 RGB entries)
  cartridge/
    mod.rs
    mapper0.rs
```

`main.rs` changes:
- Replace `run_forever` with the `winit` event loop and master clock.
- `run_nestest` remains unchanged (headless, no PPU rendering).
- `Bus::new` signature unchanged; `Bus` gains `ppu: Ppu` and `nametable_vram: [u8; 2048]` fields.

---

## 12. Acceptance Criteria

- [ ] `cargo build` succeeds with no warnings on stable Rust.
- [ ] `Bus` owns `Ppu`; CPU bus reads/writes to `$2000–$3FFF` call `ppu.register_read`/`register_write`.
- [ ] `Bus::ppu_read` and `Bus::ppu_write` route: `$0000–$1FFF` → mapper; `$2000–$3EFF` → nametable VRAM with correct horizontal/vertical mirroring; `$3F00–$3FFF` → palette RAM with transparent-mirror collapse.
- [ ] PPUADDR ($2006) double-write correctly updates `v` via `t`.
- [ ] PPUDATA ($2007) reads use the read buffer for `$0000–$3EFF`; palette reads bypass it.
- [ ] PPUDATA writes and reads increment `v` by 1 or 32 per PPUCTRL bit 2.
- [ ] PPUSTATUS read clears VBlank flag and resets `w`.
- [ ] VBlank flag set on scanline 241 dot 1; cleared on scanline 261 dot 1.
- [ ] `Ppu::tick` signature is `fn tick(&mut self, nametable_vram: &[u8; 2048], mapper: &dyn Mapper)`; called from the main loop using Rust split-field borrows (no `&mut Bus` passed in).
- [ ] `Ppu::tick` advances dot/scanline at the correct 341-dot-per-scanline rate; on an odd frame with rendering enabled, the PPU jumps from `(339, 261)` directly to `(0, 0)` of the next frame, skipping `(340, 261)`.
- [ ] `t` is copied to `v` at pre-render scanline dot 0, so ROM scroll setup (PPUADDR/PPUSCROLL writes during VBlank) is reflected in the next frame's rendering.
- [ ] Background pixels rendered correctly: nametable tile ID → pattern table lookup → attribute palette → palette RAM → NES RGB table.
- [ ] Left-8-pixel masking (PPUMASK bit 1) applied correctly.
- [ ] Rendering disabled (PPUMASK bit 3 = 0) outputs universal background color.
- [ ] `winit` window opens at 256×240 NES resolution (scaled for visibility).
- [ ] `pixels` framebuffer is updated and rendered each frame.
- [ ] Master clock interleaves PPU (every tick) and CPU (every 3 ticks) correctly.
- [ ] nestest still passes: log matches `nestest.log` for all 8991 lines (headless `--nestest` mode unaffected).
- [ ] No `unwrap()` in PPU, bus, or main loop core paths.
- [ ] A Mapper 0 ROM with a static background (e.g., a test pattern ROM or early SMB1 title screen) renders visibly correct tiles on screen.
