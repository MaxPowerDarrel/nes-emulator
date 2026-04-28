# Milestone 4 Spec: PPU — Full Scanline Rendering, NMI, Sprite 0 Hit

**Goal**: Upgrade the PPU from the Milestone 3 pixel-at-a-time approximation to cycle-accurate
scanline rendering: Loopy `v` register updates on the correct dots, NMI generation wired to the
CPU, full sprite evaluation and rendering (8×8 and 8×16), sprite 0 hit detection, sprite overflow
flag, and OAM DMA. The result must boot Super Mario Bros 1 with correct scrolling and visible
sprites.

**Primary references**:
- [NESDev PPU rendering](https://www.nesdev.org/wiki/PPU_rendering) — dot-accurate scanline timing
- [NESDev PPU scrolling (Loopy)](https://www.nesdev.org/wiki/PPU_scrolling) — v/t/x/w update schedule
- [NESDev PPU OAM](https://www.nesdev.org/wiki/PPU_OAM) — sprite attribute layout
- [NESDev Sprite evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation) — per-scanline sprite scan
- [NESDev NMI](https://www.nesdev.org/wiki/NMI) — CPU NMI vector and PPU VBlank handshake
- [NESDev PPU registers §OAMDMA](https://www.nesdev.org/wiki/PPU_registers#OAM_DMA_.28.244014.29_write) — $4014 DMA

**Deferred to later milestones**: APU, mapper IRQ, mid-scanline palette/scroll changes (MMC3),
open-bus decay, color emphasis (PPUMASK bits 5–7).

---

## 1. Changes from Milestone 3

Milestone 3 rendered background pixels using a simplified per-pixel approach that read directly
from `v` and ignored the per-dot Loopy update schedule. Milestone 4 replaces that with the
hardware-accurate update sequence:

- Coarse X is incremented inside `tick` every 8 dots during visible and pre-fetch regions.
- Fine Y is incremented at dot 256 of visible scanlines.
- Horizontal bits of `v` are reloaded from `t` at dot 257 of visible and pre-render scanlines.
- Vertical bits of `v` are reloaded from `t` at dots 280–304 of the pre-render scanline.

`render_pixel` no longer reconstructs a virtual `v` from scratch; instead it reads `v` (already
updated by the scheduler) plus fine-X (`x`) at each dot 1–256.

Additionally:
- NMI is wired from `ppu.nmi_occurred && ppu.nmi_output` into the CPU's NMI line.
- Sprite evaluation, rendering, and mixing are added.
- OAM DMA ($4014) is handled in the Bus/CPU path.
- Master loop ordering is corrected to PPU-first (see §2).

---

## 2. Master Loop Ordering Fix

**Source**: [NESDev PPU rendering](https://www.nesdev.org/wiki/PPU_rendering) — "The PPU renders
pixels during H-blank of the CPU."

Milestone 3 stepped the CPU before ticking the PPU. This is inverted from hardware and will
cause NMI to fire one CPU instruction late. Fix the main loop to tick PPU first:

```rust
// In run_windowed — each iteration of the master clock:
loop {
    bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
    bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
    bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());

    // Check and latch NMI before CPU step (§3).
    if bus.ppu.nmi_pending {
        bus.ppu.nmi_pending = false;
        cpu.nmi(&mut bus);
    }

    cpu.step(&mut bus);

    if bus.ppu.frame_complete { break; }
}
```

`cpu.step` returns CPU cycles; the loop above runs one *CPU instruction* per iteration, ticking
the PPU 3 times per CPU cycle consumed. Adjust to:

```rust
while !bus.ppu.frame_complete {
    bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
    bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());
    bus.ppu.tick(&bus.nametable_vram, bus.mapper.as_ref());

    if bus.ppu.nmi_pending {
        bus.ppu.nmi_pending = false;
        cpu.nmi(&mut bus);
    }

    cpu.step(&mut bus);
}
```

This is still not sample-accurate (each `cpu.step` consumes N cycles and the PPU only sees 3 ticks
regardless), but it is the correct ordering for NMI detection and is sufficient for Milestone 4.

---

## 3. NMI Generation

**Source**: [NESDev NMI](https://www.nesdev.org/wiki/NMI)

The PPU asserts NMI when both conditions are true simultaneously:
- `nmi_occurred` (VBlank flag, PPUSTATUS bit 7) is set.
- `nmi_output` (PPUCTRL bit 7) is set.

### PPU side

Add a `nmi_pending: bool` field to `Ppu`. Set it at the moment both conditions first become true:

```
// In tick(), after setting nmi_occurred at scanline 241, dot 1:
if self.nmi_occurred && self.nmi_output {
    self.nmi_pending = true;
}
```

Also set `nmi_pending` when PPUCTRL bit 7 transitions 0→1 while `nmi_occurred` is already set
(writing PPUCTRL during VBlank must re-trigger NMI):

```
// In register_write, case 0 (PPUCTRL), after updating nmi_output:
if self.nmi_output && self.nmi_occurred {
    self.nmi_pending = true;
}
```

Clear `nmi_pending` when `nmi_occurred` is cleared (PPUSTATUS read) or when `nmi_output` is
cleared (PPUCTRL write with bit 7 = 0). The main loop clears `nmi_pending` after servicing it.

### CPU NMI handler

Add `Cpu::nmi(&mut self, bus: &mut Bus)`. The 6502 NMI sequence:

1. Push `(PC >> 8)` to stack; push `(PC & 0xFF)` to stack.
2. Push `P` with bit 4 (B flag) **clear** and bit 5 set.
3. Set interrupt disable flag (I = 1).
4. Load PC from NMI vector at `$FFFA/$FFFB` (little-endian).
5. Consume 7 CPU cycles (add to `self.cycles`).

**Source**: [NESDev CPU interrupts](https://www.nesdev.org/wiki/CPU_interrupts) — NMI is
non-maskable and always fires regardless of the I flag.

```rust
pub fn nmi(&mut self, bus: &mut Bus) {
    let pc_hi = (self.pc >> 8) as u8;
    let pc_lo = (self.pc & 0xFF) as u8;
    self.push(bus, pc_hi);
    self.push(bus, pc_lo);
    // Push P with B clear (bit 4 = 0), bit 5 always set.
    self.push(bus, (self.p & !0x10) | 0x20);
    self.p |= 0x04; // set I
    let lo = bus.read(0xFFFA) as u16;
    let hi = bus.read(0xFFFB) as u16;
    self.pc = (hi << 8) | lo;
    self.cycles += 7;
}
```

---

## 4. Loopy v Register Updates During Rendering

**Source**: [NESDev PPU scrolling](https://www.nesdev.org/wiki/PPU_scrolling) — "During rendering"
section.

These updates happen inside `Ppu::tick` when rendering is enabled (`ppumask & 0x18 != 0`).

### 4a. Coarse X increment — `hori(v)`

Increments the coarse X component of `v` and toggles the horizontal nametable bit when coarse X
wraps from 31 to 0.

**When**: At dot 8, 16, 24 … 256 of visible scanlines (0–239) and also at dots 328 and 336 of
every scanline (the pre-fetch of the first two tiles of the next scanline).

```rust
fn increment_coarse_x(&mut self) {
    if self.v & 0x001F == 31 {
        self.v &= !0x001F;   // coarse X = 0
        self.v ^= 0x0400;    // flip horizontal nametable
    } else {
        self.v += 1;
    }
}
```

### 4b. Fine Y increment — `vert(v)`

Increments fine Y; when fine Y overflows (was 7), resets fine Y to 0 and increments coarse Y.
Coarse Y wraps at 29 (not 31) and toggles the vertical nametable bit when it does.

**When**: At dot 256 of visible scanlines (0–239).

```rust
fn increment_fine_y(&mut self) {
    if (self.v & 0x7000) != 0x7000 {
        self.v += 0x1000; // increment fine Y
    } else {
        self.v &= !0x7000; // fine Y = 0
        let mut coarse_y = (self.v >> 5) & 0x1F;
        if coarse_y == 29 {
            coarse_y = 0;
            self.v ^= 0x0800; // flip vertical nametable
        } else if coarse_y == 31 {
            coarse_y = 0; // wrap without flipping (out-of-range attribute rows)
        } else {
            coarse_y += 1;
        }
        self.v = (self.v & !0x03E0) | (coarse_y << 5);
    }
}
```

### 4c. Horizontal reload — `hori(v) = hori(t)`

Copy horizontal scroll bits from `t` to `v`:
- Bits 4–0 (coarse X) and bit 10 (horizontal nametable).

**When**: Dot 257 of visible scanlines (0–239) and the pre-render scanline (261).

```rust
fn reload_horizontal(&mut self) {
    // v[4:0]  ← t[4:0]
    // v[10]   ← t[10]
    self.v = (self.v & !0x041F) | (self.t & 0x041F);
}
```

### 4d. Vertical reload — `vert(v) = vert(t)`

Copy vertical scroll bits from `t` to `v`:
- Bits 9–5 (coarse Y), bits 14–12 (fine Y), bit 11 (vertical nametable).

**When**: Dots 280–304 of the pre-render scanline (261) only.

```rust
fn reload_vertical(&mut self) {
    // v[14:12] ← t[14:12]  (fine Y)
    // v[11]    ← t[11]     (vertical nametable)
    // v[9:5]   ← t[9:5]    (coarse Y)
    self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0);
}
```

### 4e. Integration into tick()

Replace the Milestone 3 `v = t` copy at pre-render dot 0 with the full schedule:

```
tick():
  [advance dot/scanline as before]

  if rendering_enabled():
    // Visible scanlines and pre-render: coarse X increment every 8 pixels
    if (scanline < 240 || scanline == 261):
      if dot >= 1 && dot <= 256 && dot % 8 == 0:
        increment_coarse_x()
      if dot == 256:
        if scanline < 240: increment_fine_y()
      if dot == 257:
        reload_horizontal()
      if dot == 328 || dot == 336:
        increment_coarse_x()

    // Pre-render only: vertical reload
    if scanline == 261 && dot >= 280 && dot <= 304:
      reload_vertical()

  // Pixel output — same timing as M3, but render_pixel now reads v directly
  if scanline < 240 && dot >= 1 && dot <= 256:
    render_pixel(scanline, dot - 1, nametable_vram, mapper)

  [VBlank set/clear as before]
```

Remove the Milestone 3 `if scanline == 261 && dot == 0 { v = t }` — the new reload schedule
replaces it.

### 4f. render_pixel simplification

With `v` now maintained by the scheduler, `render_pixel` reads coarse X, coarse Y, fine Y, and
nametable bits directly from `self.v`, plus fine X from `self.x`. No virtual-v reconstruction
is needed:

```rust
fn render_pixel(&mut self, y: u16, x: u16, nametable_vram: &[u8; 2048], mapper: &dyn Mapper) {
    // Left-8-pixel masking: PPUMASK bit 1
    if x < 8 && self.ppumask & 0x02 == 0 { /* output bg color, return */ }

    // Background tile from v + fine-X offset
    let fine_x_pixel = (x + self.x as u16) % 8;
    let nt_addr  = 0x2000 | (self.v & 0x0FFF);
    let tile_id  = self.ppu_read_addr(nt_addr, nametable_vram, mapper) as u16;

    let at_addr  = 0x23C0
        | (self.v & 0x0C00)
        | ((self.v >> 4) & 0x0038)
        | ((self.v >> 2) & 0x0007);
    let attr_byte = self.ppu_read_addr(at_addr, nametable_vram, mapper);

    let x_quad = ((self.v >> 1) & 1) as u8;
    let y_quad = ((self.v >> 6) & 1) as u8;
    let shift = (y_quad * 2 + x_quad) * 2;
    let palette_select = ((attr_byte >> shift) & 0x03) as usize;

    let fine_y = (self.v >> 12) & 0x07;
    let pt_base = self.bg_pattern_table_base();
    let lo = self.ppu_read_addr(pt_base + tile_id * 16 + fine_y, nametable_vram, mapper);
    let hi = self.ppu_read_addr(pt_base + tile_id * 16 + fine_y + 8, nametable_vram, mapper);

    let bit_pos = 7 - fine_x_pixel as u8;
    let color_idx = (((hi >> bit_pos) & 1) << 1) | ((lo >> bit_pos) & 1);

    let bg_palette_byte = if color_idx == 0 {
        self.palette_ram[0]
    } else {
        self.palette_ram[palette_select * 4 + color_idx as usize]
    };
    let bg_opaque = color_idx != 0;

    // Sprite layer — §5
    let (sprite_pixel, sprite_opaque, sprite_priority, is_sprite0) =
        self.sprite_pixel_at(x, y);

    // Pixel priority mux — spec §5d
    let final_palette_byte = pixel_priority_mux(
        bg_palette_byte, bg_opaque,
        sprite_pixel, sprite_opaque,
        sprite_priority, is_sprite0,
        &mut self.sprite_zero_hit,
        self.ppumask,
    );
    let (r, g, b) = NES_PALETTE[(final_palette_byte & 0x3F) as usize];
    // write to framebuffer ...
}
```

---

## 5. Sprite Rendering

**Source**: [NESDev PPU OAM](https://www.nesdev.org/wiki/PPU_OAM),
[NESDev Sprite evaluation](https://www.nesdev.org/wiki/PPU_sprite_evaluation)

### 5a. OAM layout

Each sprite occupies 4 bytes in primary OAM (256 bytes = 64 sprites):

| OAM byte | Field | Notes |
|----------|-------|-------|
| 0 | Y position | Top of sprite; sprite is on scanlines Y+1 through Y+H (H=8 or 16) |
| 1 | Tile index | For 8×16: bit 0 = pattern table, bits 7–1 = tile number |
| 2 | Attributes | bit 7 = flip vertical, bit 6 = flip horizontal, bit 5 = priority, bits 1–0 = palette (4–7) |
| 3 | X position | Left edge |

Y=239 ($EF) hides the sprite off-screen (common idiom).

### 5b. Sprite evaluation (simplified — no hardware-accurate timing)

The NES hardware performs sprite evaluation across dots 65–256 of each visible scanline. For
Milestone 4, perform it as a single scan at dot 257 (after the visible pixels are committed):

```rust
fn evaluate_sprites(&mut self, next_scanline: u16) {
    self.secondary_oam = [(0xFF, 0xFF, 0xFF, 0xFF); 8];
    self.sprite_count = 0;
    self.sprite_zero_in_range = false;

    let sprite_height: u16 = if self.ppuctrl & 0x20 != 0 { 16 } else { 8 };

    for i in 0..64usize {
        let y = self.oam[i * 4] as u16;
        if next_scanline >= y + 1 && next_scanline < y + 1 + sprite_height {
            if self.sprite_count < 8 {
                self.secondary_oam[self.sprite_count] = (
                    self.oam[i * 4],
                    self.oam[i * 4 + 1],
                    self.oam[i * 4 + 2],
                    self.oam[i * 4 + 3],
                );
                if i == 0 {
                    self.sprite_zero_in_range = true;
                }
                self.sprite_count += 1;
            } else {
                // More than 8 sprites on this line — set overflow flag.
                self.sprite_overflow = true;
                break;
            }
        }
    }
}
```

Call `evaluate_sprites(scanline + 1)` at dot 257 of every visible scanline (0–239).

For the pre-render scanline (261), also call `evaluate_sprites(0)` at dot 257 to prime
secondary OAM for scanline 0.

Add to `Ppu`:
```rust
secondary_oam: [(u8, u8, u8, u8); 8],  // (y, tile, attr, x) for up to 8 active sprites
sprite_count: usize,
sprite_zero_in_range: bool,
sprite_overflow: bool,
```

### 5c. Sprite pixel fetch — `sprite_pixel_at`

Called from `render_pixel` for each visible pixel. Returns `(palette_byte, opaque, priority, is_sprite0)`.

```rust
fn sprite_pixel_at(
    &self,
    x: u16,
    y: u16,
    nametable_vram: &[u8; 2048],
    mapper: &dyn Mapper,
) -> (u8, bool, bool, bool) {
    let show_sprites = self.ppumask & 0x10 != 0;
    let show_sprites_left = self.ppumask & 0x04 != 0;
    if !show_sprites || (x < 8 && !show_sprites_left) {
        return (0, false, false, false);
    }

    let sprite_height: u16 = if self.ppuctrl & 0x20 != 0 { 16 } else { 8 };

    for (idx, &(spr_y, tile_idx, attr, spr_x)) in
        self.secondary_oam[..self.sprite_count].iter().enumerate()
    {
        let spr_x = spr_x as u16;
        if x < spr_x || x >= spr_x + 8 { continue; }

        let mut fine_x = x - spr_x; // 0–7
        let mut fine_y = y - (spr_y as u16 + 1); // 0–7 (or 0–15 for 8×16)

        let flip_h = attr & 0x40 != 0;
        let flip_v = attr & 0x80 != 0;
        if flip_h { fine_x = 7 - fine_x; }

        let (pt_base, tile_row) = if sprite_height == 16 {
            // 8×16: tile bank from bit 0 of tile_idx
            let bank: u16 = if tile_idx & 1 != 0 { 0x1000 } else { 0x0000 };
            let base_tile = (tile_idx & 0xFE) as u16;
            if flip_v { fine_y = 15 - fine_y; }
            let (tile_offset, row) = if fine_y < 8 { (0, fine_y) } else { (1, fine_y - 8) };
            (bank + (base_tile + tile_offset) * 16, row)
        } else {
            // 8×8: tile bank from PPUCTRL bit 3
            let bank: u16 = if self.ppuctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };
            if flip_v { fine_y = 7 - fine_y; }
            (bank + tile_idx as u16 * 16, fine_y)
        };

        let lo = self.ppu_read_addr(pt_base + tile_row, nametable_vram, mapper);
        let hi = self.ppu_read_addr(pt_base + tile_row + 8, nametable_vram, mapper);

        let bit_pos = 7 - fine_x as u8;
        let color_idx = (((hi >> bit_pos) & 1) << 1) | ((lo >> bit_pos) & 1);
        if color_idx == 0 { continue; } // transparent

        let palette_idx = (attr & 0x03) as usize;
        let palette_byte = self.palette_ram[0x10 + palette_idx * 4 + color_idx as usize];
        let priority = attr & 0x20 == 0; // 0 = in front of BG, 1 = behind BG
        let is_sprite0 = idx == 0 && self.sprite_zero_in_range;

        return (palette_byte, true, priority, is_sprite0);
    }
    (0, false, false, false)
}
```

### 5d. Pixel priority mux

```
bg_color   = BG palette byte
spr_color  = sprite palette byte
bg_opaque  = bg color_idx != 0
spr_opaque = sprite color_idx != 0
spr_priority = true means sprite is in front of BG

Result:
  if !spr_opaque && !bg_opaque → universal BG color (palette_ram[0])
  if !spr_opaque && bg_opaque  → bg_color
  if spr_opaque && !bg_opaque  → spr_color
  if spr_opaque && bg_opaque:
    if spr_priority → spr_color
    else            → bg_color
```

---

## 6. Sprite 0 Hit

**Source**: [NESDev PPU OAM §Sprite zero hits](https://www.nesdev.org/wiki/PPU_OAM#Sprite_zero_hits)

Sprite zero hit is set when **all** of the following are true at a given pixel:
- Sprite 0 is among the sprites loaded into secondary OAM (`sprite_zero_in_range`).
- The pixel being rendered is sprite 0 (index 0 in secondary OAM, i.e., the first non-transparent sprite 0 pixel hit).
- Both the background pixel and sprite 0 pixel are opaque (`color_idx != 0`).
- Rendering is enabled for both background and sprites.
- The current dot is **not** 255 (X = 255 cannot trigger sprite 0 hit).
- If PPUMASK bit 1 (BG left column mask) or bit 2 (sprite left column mask) is clear, pixels at X = 0–7 do not trigger the hit.

Set `sprite_zero_hit` (PPUSTATUS bit 6) inside `render_pixel` / `pixel_priority_mux` when `is_sprite0 && bg_opaque && spr_opaque && x != 255`.

Add `sprite_zero_hit: bool` and `sprite_overflow: bool` to `Ppu`.

PPUSTATUS read already returns these bits; update the read handler:

```rust
2 => {
    let status = ((self.nmi_occurred    as u8) << 7)
               | ((self.sprite_zero_hit as u8) << 6)
               | ((self.sprite_overflow as u8) << 5)
               | (self.open_bus & 0x1F);
    self.nmi_occurred = false;
    self.w = false;
    status
}
```

Clear `sprite_zero_hit` and `sprite_overflow` at pre-render scanline dot 1 (alongside `nmi_occurred`).

---

## 7. OAM DMA ($4014)

**Source**: [NESDev PPU registers §OAM DMA](https://www.nesdev.org/wiki/PPU_registers#OAM_DMA_.28.244014.29_write)

Writing to $4014 initiates a 256-byte DMA transfer. The written value `N` specifies the CPU page:
bytes are copied from `$NN00–$NNFF` into OAM starting at `oam_addr`.

### Bus routing

Add $4014 to the CPU write path in `bus.rs`:

```rust
0x4014 => {
    self.oam_dma_pending = true;
    self.oam_dma_page = val;
}
```

Add to `Bus`:
```rust
pub oam_dma_pending: bool,
pub oam_dma_page: u8,
```

### Main loop DMA execution

Handle DMA in the main loop, **after** the PPU tick and **before** `cpu.step`:

```rust
if bus.oam_dma_pending {
    bus.oam_dma_pending = false;
    let page = (bus.oam_dma_page as u16) << 8;
    for i in 0u16..256 {
        let val = bus.read(page | i);
        bus.ppu.oam[bus.ppu.oam_addr.wrapping_add(i as u8) as usize] = val;
    }
    // DMA stalls CPU for 513 cycles (+1 on odd cycle). Add to cpu.cycles.
    cpu.cycles += 513;
}
```

The hardware stalls the CPU for 513 or 514 cycles depending on the current cycle parity; adding
513 is a sufficient approximation for Milestone 4.

---

## 8. PPU State Additions

New fields required (add to `Ppu::new` with zero/false defaults):

| Field | Type | Description |
|-------|------|-------------|
| `nmi_pending` | bool | Edge-triggered NMI signal to CPU; consumed by main loop |
| `sprite_zero_hit` | bool | PPUSTATUS bit 6 |
| `sprite_overflow` | bool | PPUSTATUS bit 5 |
| `secondary_oam` | [(u8,u8,u8,u8); 8] | Active sprites for current scanline |
| `sprite_count` | usize | Number of sprites in secondary_oam |
| `sprite_zero_in_range` | bool | Sprite 0 was found during evaluation |

---

## 9. Module Structure (unchanged)

No new source files required. All changes are within:
- `src/ppu/mod.rs` — tick scheduler updates, sprite evaluation/rendering, NMI pending field
- `src/bus.rs` — OAM DMA pending fields, $4014 write handler
- `src/cpu/mod.rs` — `Cpu::nmi()` method
- `src/main.rs` — master loop ordering fix, DMA execution, NMI servicing

---

## 10. Acceptance Criteria

- [ ] `cargo build` with no warnings on stable Rust.
- [ ] Loopy `v` register is updated by the scheduler (coarse X, fine Y, horizontal/vertical reload) at the correct dots — not reconstructed per-pixel.
- [ ] `render_pixel` reads `v` and `x` directly; no virtual-v construction.
- [ ] NMI fires on the correct CPU instruction boundary after VBlank start (scanline 241, dot 1).
- [ ] Writing PPUCTRL with bit 7 = 1 during VBlank immediately asserts NMI.
- [ ] Reading PPUSTATUS clears `nmi_occurred`; NMI does not re-fire until VBlank starts again.
- [ ] `Cpu::nmi` pushes PC and P correctly; sets I flag; loads from $FFFA/$FFFB.
- [ ] nestest still passes (headless `--nestest` mode unaffected).
- [ ] Sprites are evaluated per-scanline; up to 8 sprites rendered per scanline.
- [ ] 8×8 sprite pattern table selected by PPUCTRL bit 3.
- [ ] 8×16 sprite tile bank selected by tile index bit 0; top/bottom half handled correctly.
- [ ] Horizontal and vertical sprite flip correct.
- [ ] Sprite priority (behind/in-front of BG) correct per attribute bit 5.
- [ ] Sprite palette drawn from palette RAM $10–$1F.
- [ ] Sprite 0 hit sets PPUSTATUS bit 6 when sprite 0 and BG are both opaque at the same pixel (excluding dot 255, left-column mask rules observed).
- [ ] Sprite overflow sets PPUSTATUS bit 5 when more than 8 sprites fall on one scanline.
- [ ] `sprite_zero_hit` and `sprite_overflow` cleared at pre-render dot 1.
- [ ] OAM DMA: writing $4014 copies 256 bytes from CPU page into OAM; CPU stalled 513 cycles.
- [ ] PPUMASK bit 4 (show sprites) and bit 2 (sprites in left 8px) respected.
- [ ] Master loop ticks PPU first, then services NMI, then steps CPU.
- [ ] Super Mario Bros 1 boots, title screen renders with correct background scroll and visible sprites.
- [ ] No `unwrap()` in PPU, bus, CPU, or main loop core paths.