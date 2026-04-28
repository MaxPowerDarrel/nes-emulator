/// NES PPU (Picture Processing Unit) — 2C02.
///
/// Spec refs:
///   https://www.nesdev.org/wiki/PPU
///   https://www.nesdev.org/wiki/PPU_registers
///   https://www.nesdev.org/wiki/PPU_scrolling  (Loopy registers)
///   https://www.nesdev.org/wiki/PPU_rendering
///   https://www.nesdev.org/wiki/PPU_memory_map
///   https://www.nesdev.org/wiki/PPU_palettes
///   https://www.nesdev.org/wiki/PPU_OAM
///   https://www.nesdev.org/wiki/PPU_sprite_evaluation
///   https://www.nesdev.org/wiki/NMI

pub mod palette;

use crate::bus::{nametable_index, palette_addr};
use crate::cartridge::Mapper;
use palette::NES_PALETTE;

const SCREEN_WIDTH: usize = 256;
const SCREEN_HEIGHT: usize = 240;

pub struct Ppu {
    // Loopy scrolling registers — spec: PPU_scrolling
    // v[14:12]=fine_y, v[11]=nt_y, v[10]=nt_x, v[9:5]=coarse_y, v[4:0]=coarse_x
    pub v: u16,
    pub t: u16,
    pub x: u8,   // fine X scroll (3-bit)
    pub w: bool, // write latch

    // Frame timing
    pub scanline: u16, // 0–261; 261 = pre-render
    pub dot: u16,      // 0–340
    pub odd_frame: bool,

    // CPU-visible registers
    pub ppuctrl: u8,
    pub ppumask: u8,
    pub oam_addr: u8,
    pub oam: [u8; 256],

    // Status flags
    pub nmi_output: bool,    // PPUCTRL bit 7
    pub nmi_occurred: bool,  // VBlank flag — PPUSTATUS bit 7
    pub sprite_zero_hit: bool, // PPUSTATUS bit 6
    pub sprite_overflow: bool, // PPUSTATUS bit 5

    /// Edge-triggered NMI signal to the CPU; set when `nmi_occurred && nmi_output`
    /// transitions to true. The main loop consumes it and dispatches `Cpu::nmi`.
    /// Source: https://www.nesdev.org/wiki/NMI
    pub nmi_pending: bool,

    // Sprite evaluation state — spec §5
    /// Active sprites for the current scanline: (y, tile, attr, x). Up to 8 entries.
    pub secondary_oam: [(u8, u8, u8, u8); 8],
    pub sprite_count: usize,
    pub sprite_zero_in_range: bool,

    // Memory
    pub palette_ram: [u8; 32],

    // I/O bookkeeping
    pub read_buffer: u8, // PPUDATA read buffer for non-palette addresses
    pub open_bus: u8,    // last byte written to any PPU register

    // Output
    pub framebuffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
    pub frame_complete: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Self {
            v: 0,
            t: 0,
            x: 0,
            w: false,
            scanline: 0,
            dot: 0,
            odd_frame: false,
            ppuctrl: 0,
            ppumask: 0,
            oam_addr: 0,
            oam: [0; 256],
            nmi_output: false,
            nmi_occurred: false,
            sprite_zero_hit: false,
            sprite_overflow: false,
            nmi_pending: false,
            secondary_oam: [(0xFF, 0xFF, 0xFF, 0xFF); 8],
            sprite_count: 0,
            sprite_zero_in_range: false,
            palette_ram: [0; 32],
            read_buffer: 0,
            open_bus: 0,
            framebuffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
            frame_complete: false,
        }
    }

    fn rendering_enabled(&self) -> bool {
        // PPUMASK bit 3 = show BG, bit 4 = show sprites.
        // Loopy v register updates are gated on either being enabled.
        self.ppumask & 0x18 != 0
    }

    fn show_background(&self) -> bool {
        self.ppumask & 0x08 != 0
    }

    fn show_sprites(&self) -> bool {
        self.ppumask & 0x10 != 0
    }

    fn vram_increment(&self) -> u16 {
        if self.ppuctrl & 0x04 != 0 { 32 } else { 1 }
    }

    fn bg_pattern_table_base(&self) -> u16 {
        if self.ppuctrl & 0x10 != 0 { 0x1000 } else { 0x0000 }
    }

    /// Read from the PPU address space.
    /// Routes: $0000–$1FFF → mapper CHR; $2000–$3EFF → nametable VRAM; $3F00–$3FFF → palette RAM.
    fn ppu_read_addr(&self, addr: u16, nametable_vram: &[u8; 2048], mapper: &dyn Mapper) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => mapper.ppu_read(addr).unwrap_or(0),
            0x2000..=0x3EFF => {
                nametable_vram[nametable_index(addr, mapper.mirroring())]
            }
            0x3F00..=0x3FFF => self.palette_ram[palette_addr(addr)],
            _ => 0,
        }
    }

    /// Write to the PPU address space.
    fn ppu_write_addr(
        &mut self,
        addr: u16,
        val: u8,
        nametable_vram: &mut [u8; 2048],
        mapper: &mut dyn Mapper,
    ) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => mapper.ppu_write(addr, val),
            0x2000..=0x3EFF => {
                nametable_vram[nametable_index(addr, mapper.mirroring())] = val;
            }
            0x3F00..=0x3FFF => {
                self.palette_ram[palette_addr(addr)] = val;
            }
            _ => {}
        }
    }

    /// Handle CPU read from a PPU register ($2000–$2007, passed as 0–7).
    /// Source: https://www.nesdev.org/wiki/PPU_registers
    pub fn register_read(
        &mut self,
        reg: u8,
        nametable_vram: &[u8; 2048],
        mapper: &dyn Mapper,
    ) -> u8 {
        match reg & 0x07 {
            // $2000 PPUCTRL — write-only, return open bus
            0 => self.open_bus,
            // $2001 PPUMASK — write-only, return open bus
            1 => self.open_bus,
            // $2002 PPUSTATUS
            // Side effects: clear VBlank flag; clear nmi_pending; reset w latch.
            2 => {
                let status = ((self.nmi_occurred as u8) << 7)
                    | ((self.sprite_zero_hit as u8) << 6)
                    | ((self.sprite_overflow as u8) << 5)
                    | (self.open_bus & 0x1F);
                self.nmi_occurred = false;
                // Spec §3: clearing nmi_occurred deasserts the pending edge.
                self.nmi_pending = false;
                self.w = false;
                status
            }
            // $2003 OAMADDR — write-only
            3 => self.open_bus,
            // $2004 OAMDATA
            4 => self.oam[self.oam_addr as usize],
            // $2005/$2006 — write-only
            5 | 6 => self.open_bus,
            // $2007 PPUDATA
            // Reads from $0000–$3EFF return buffered value; $3F00–$3FFF return immediately.
            7 => {
                let addr = self.v & 0x3FFF;
                let result = if addr >= 0x3F00 {
                    let val = self.palette_ram[palette_addr(addr)];
                    self.read_buffer =
                        self.ppu_read_addr(addr & 0x2FFF, nametable_vram, mapper);
                    val
                } else {
                    let buffered = self.read_buffer;
                    self.read_buffer = self.ppu_read_addr(addr, nametable_vram, mapper);
                    buffered
                };
                self.v = self.v.wrapping_add(self.vram_increment());
                result
            }
            _ => 0,
        }
    }

    /// Handle CPU write to a PPU register ($2000–$2007, passed as 0–7).
    pub fn register_write(
        &mut self,
        reg: u8,
        val: u8,
        nametable_vram: &mut [u8; 2048],
        mapper: &mut dyn Mapper,
    ) {
        self.open_bus = val;
        match reg & 0x07 {
            // $2000 PPUCTRL
            // Side effect: bits 1–0 written into t[11:10].
            // Spec §3: 0→1 transition of bit 7 during VBlank re-asserts NMI.
            0 => {
                self.ppuctrl = val;
                let new_nmi_output = val & 0x80 != 0;
                if !self.nmi_output && new_nmi_output && self.nmi_occurred {
                    self.nmi_pending = true;
                } else if !new_nmi_output {
                    // Disabling NMI cancels any pending edge.
                    self.nmi_pending = false;
                }
                self.nmi_output = new_nmi_output;
                self.t = (self.t & 0xF3FF) | (((val & 0x03) as u16) << 10);
            }
            // $2001 PPUMASK
            1 => {
                self.ppumask = val;
            }
            // $2002 PPUSTATUS — read-only; writes ignored
            2 => {}
            // $2003 OAMADDR
            3 => {
                self.oam_addr = val;
            }
            // $2004 OAMDATA
            4 => {
                self.oam[self.oam_addr as usize] = val;
                self.oam_addr = self.oam_addr.wrapping_add(1);
            }
            // $2005 PPUSCROLL (two writes via w latch)
            5 => {
                if !self.w {
                    self.x = val & 0x07;
                    self.t = (self.t & 0xFFE0) | ((val >> 3) as u16);
                } else {
                    self.t = (self.t & 0x8FFF) | (((val & 0x07) as u16) << 12);
                    self.t = (self.t & 0xFC1F) | (((val >> 3) as u16) << 5);
                }
                self.w = !self.w;
            }
            // $2006 PPUADDR (two writes via w latch)
            6 => {
                if !self.w {
                    self.t = (self.t & 0x00FF) | (((val & 0x3F) as u16) << 8);
                    self.t &= 0x3FFF;
                } else {
                    self.t = (self.t & 0xFF00) | (val as u16);
                    self.v = self.t;
                }
                self.w = !self.w;
            }
            // $2007 PPUDATA
            7 => {
                let addr = self.v;
                self.ppu_write_addr(addr, val, nametable_vram, mapper);
                self.v = self.v.wrapping_add(self.vram_increment());
            }
            _ => {}
        }
    }

    // ── Loopy v register update schedule (spec §4) ──────────────────────────

    fn increment_coarse_x(&mut self) {
        if self.v & 0x001F == 31 {
            self.v &= !0x001F;
            self.v ^= 0x0400;
        } else {
            self.v += 1;
        }
    }

    fn increment_fine_y(&mut self) {
        if (self.v & 0x7000) != 0x7000 {
            self.v += 0x1000;
        } else {
            self.v &= !0x7000;
            let mut coarse_y = (self.v >> 5) & 0x1F;
            if coarse_y == 29 {
                coarse_y = 0;
                self.v ^= 0x0800;
            } else if coarse_y == 31 {
                coarse_y = 0;
            } else {
                coarse_y += 1;
            }
            self.v = (self.v & !0x03E0) | (coarse_y << 5);
        }
    }

    fn reload_horizontal(&mut self) {
        self.v = (self.v & !0x041F) | (self.t & 0x041F);
    }

    fn reload_vertical(&mut self) {
        self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0);
    }

    /// Advance the PPU by one dot.
    ///
    /// Source: https://www.nesdev.org/wiki/PPU_rendering
    pub fn tick(&mut self, nametable_vram: &[u8; 2048], mapper: &dyn Mapper) {
        // NTSC odd-frame dot skip: pre-render scanline is 340 dots when rendering enabled.
        let odd_skip = self.odd_frame
            && self.rendering_enabled()
            && self.scanline == 261
            && self.dot == 339;

        self.dot += 1;
        if self.dot == 341 || odd_skip {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline == 262 {
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
            }
        }

        // Clear status flags at pre-render scanline dot 1 (spec §6).
        if self.scanline == 261 && self.dot == 1 {
            self.nmi_occurred = false;
            self.nmi_pending = false;
            self.sprite_zero_hit = false;
            self.sprite_overflow = false;
            self.frame_complete = false;
        }

        // Render one pixel during visible scanlines.
        //
        // Must happen BEFORE the Loopy v scheduler. At dots where dot % 8 == 0 (screen
        // x = 7, 15, …, 255) the scheduler increments coarse_x to point at the next
        // tile; if we rendered after that, every 8th column would sample the wrong
        // tile. Real hardware avoids this with shift registers — fetch advances v
        // while rendering reads from already-shifted data — but for the simplified
        // per-pixel path, render-then-advance produces equivalent output.
        if self.scanline < 240 && self.dot >= 1 && self.dot <= 256 {
            let px = self.dot - 1;
            let py = self.scanline;
            self.render_pixel(py, px, nametable_vram, mapper);
        }

        // Loopy v register schedule — only when rendering enabled (spec §4).
        if self.rendering_enabled() {
            let visible = self.scanline < 240;
            let prerender = self.scanline == 261;

            if visible || prerender {
                if self.dot >= 1 && self.dot <= 256 && self.dot % 8 == 0 {
                    self.increment_coarse_x();
                }
                if self.dot == 256 && visible {
                    self.increment_fine_y();
                }
                if self.dot == 257 {
                    self.reload_horizontal();
                }
                // Dots 328/336 pre-fetch increments are omitted here.
                // Real hardware uses shift registers: these increments advance v to the
                // third pre-fetched tile while the first two are already in the shift
                // register. Since render_pixel reads v directly (no shift registers), the
                // increments would leave coarse_x 2 tiles ahead at the start of every
                // scanline, producing a constant 16-pixel horizontal offset.
            }

            if prerender && self.dot >= 280 && self.dot <= 304 {
                self.reload_vertical();
            }

            // Sprite evaluation for the next scanline at dot 257 (spec §5b).
            if self.dot == 257 {
                if visible {
                    self.evaluate_sprites(self.scanline + 1);
                } else if prerender {
                    self.evaluate_sprites(0);
                }
            }
        }

        // VBlank start: scanline 241, dot 1.
        if self.scanline == 241 && self.dot == 1 {
            self.nmi_occurred = true;
            self.frame_complete = true;
            if self.nmi_output {
                self.nmi_pending = true;
            }
        }
    }

    // ── Sprite evaluation (spec §5b) ────────────────────────────────────────

    fn evaluate_sprites(&mut self, next_scanline: u16) {
        self.secondary_oam = [(0xFF, 0xFF, 0xFF, 0xFF); 8];
        self.sprite_count = 0;
        self.sprite_zero_in_range = false;

        let sprite_height: u16 = if self.ppuctrl & 0x20 != 0 { 16 } else { 8 };

        for i in 0..64usize {
            let y = self.oam[i * 4] as u16;
            // Sprite is rendered on scanlines y+1 .. y+1+H.
            if y >= 0xEF {
                // Y >= 239 hides sprite; also avoids overflow on y+1+H.
                continue;
            }
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
                    self.sprite_overflow = true;
                    break;
                }
            }
        }
    }

    // ── Sprite pixel fetch (spec §5c) ───────────────────────────────────────

    /// Returns (palette_byte, opaque, priority_in_front, is_sprite0).
    fn sprite_pixel_at(
        &self,
        x: u16,
        y: u16,
        nametable_vram: &[u8; 2048],
        mapper: &dyn Mapper,
    ) -> (u8, bool, bool, bool) {
        if !self.show_sprites() {
            return (0, false, false, false);
        }
        let show_left = self.ppumask & 0x04 != 0;
        if x < 8 && !show_left {
            return (0, false, false, false);
        }

        let sprite_height: u16 = if self.ppuctrl & 0x20 != 0 { 16 } else { 8 };

        for idx in 0..self.sprite_count {
            let (spr_y, tile_idx, attr, spr_x) = self.secondary_oam[idx];
            let spr_x = spr_x as u16;
            if x < spr_x || x >= spr_x + 8 {
                continue;
            }

            let mut fine_x = x - spr_x; // 0–7
            let mut fine_y = y - (spr_y as u16 + 1); // 0–7 or 0–15

            let flip_h = attr & 0x40 != 0;
            let flip_v = attr & 0x80 != 0;
            if flip_h {
                fine_x = 7 - fine_x;
            }

            let (pt_addr, tile_row) = if sprite_height == 16 {
                let bank: u16 = if tile_idx & 1 != 0 { 0x1000 } else { 0x0000 };
                let base_tile = (tile_idx & 0xFE) as u16;
                if flip_v {
                    fine_y = 15 - fine_y;
                }
                let (tile_offset, row) = if fine_y < 8 { (0, fine_y) } else { (1, fine_y - 8) };
                (bank + (base_tile + tile_offset) * 16, row)
            } else {
                let bank: u16 = if self.ppuctrl & 0x08 != 0 { 0x1000 } else { 0x0000 };
                if flip_v {
                    fine_y = 7 - fine_y;
                }
                (bank + tile_idx as u16 * 16, fine_y)
            };

            let lo = self.ppu_read_addr(pt_addr + tile_row, nametable_vram, mapper);
            let hi = self.ppu_read_addr(pt_addr + tile_row + 8, nametable_vram, mapper);

            let bit_pos = 7 - fine_x as u8;
            let color_idx = (((hi >> bit_pos) & 1) << 1) | ((lo >> bit_pos) & 1);
            if color_idx == 0 {
                continue; // transparent — fall through to next sprite
            }

            let palette_idx = (attr & 0x03) as usize;
            // Sprite palettes live at $3F10–$3F1F; with palette_addr() mirrors handled.
            let palette_byte = self.palette_ram[0x10 + palette_idx * 4 + color_idx as usize];
            let priority = attr & 0x20 == 0;
            let is_sprite0 = idx == 0 && self.sprite_zero_in_range;

            return (palette_byte, true, priority, is_sprite0);
        }
        (0, false, false, false)
    }

    // ── Background + sprite pixel render (spec §4f, §5d) ────────────────────

    /// Render one pixel at screen position (y, x), mixing background and sprites.
    fn render_pixel(&mut self, y: u16, x: u16, nametable_vram: &[u8; 2048], mapper: &dyn Mapper) {
        // Background fetch from v + fine X.
        let show_bg = self.show_background();
        let show_bg_left = self.ppumask & 0x02 != 0;

        let (bg_palette_byte, bg_opaque) = if !show_bg || (x < 8 && !show_bg_left) {
            (self.palette_ram[0], false)
        } else {
            // Fine-x may cross a tile boundary within an 8-pixel group.
            // coarse_x (in v) increments every 8 screen pixels, but fine_x shifts the
            // window so the crossing happens at pixel (8 - fine_x), not at pixel 8.
            // When shifted_x >= 8, advance to the next tile without mutating v.
            let shifted_x = (x as u8 % 8) + self.x;
            let (render_v, fine_x_pixel) = if shifted_x >= 8 {
                let next_v = if self.v & 0x001F == 31 {
                    (self.v & !0x001F) ^ 0x0400
                } else {
                    self.v + 1
                };
                (next_v, shifted_x - 8)
            } else {
                (self.v, shifted_x)
            };

            let nt_addr = 0x2000 | (render_v & 0x0FFF);
            let tile_id = self.ppu_read_addr(nt_addr, nametable_vram, mapper) as u16;

            let at_addr = 0x23C0
                | (render_v & 0x0C00)
                | ((render_v >> 4) & 0x0038)
                | ((render_v >> 2) & 0x0007);
            let attr_byte = self.ppu_read_addr(at_addr, nametable_vram, mapper);

            let x_quad = ((render_v >> 1) & 1) as u8;
            let y_quad = ((render_v >> 6) & 1) as u8;
            let shift = (y_quad * 2 + x_quad) * 2;
            let palette_select = ((attr_byte >> shift) & 0x03) as usize;

            let fine_y = (self.v >> 12) & 0x07;
            let pt_base = self.bg_pattern_table_base();
            let lo = self.ppu_read_addr(pt_base + tile_id * 16 + fine_y, nametable_vram, mapper);
            let hi = self.ppu_read_addr(pt_base + tile_id * 16 + fine_y + 8, nametable_vram, mapper);

            let bit_pos = 7 - fine_x_pixel;
            let color_idx = (((hi >> bit_pos) & 1) << 1) | ((lo >> bit_pos) & 1);

            if color_idx == 0 {
                (self.palette_ram[0], false)
            } else {
                (self.palette_ram[palette_select * 4 + color_idx as usize], true)
            }
        };

        // Sprite layer.
        let (spr_palette_byte, spr_opaque, spr_priority, is_sprite0) =
            self.sprite_pixel_at(x, y, nametable_vram, mapper);

        // Sprite 0 hit detection (spec §6).
        if is_sprite0 && bg_opaque && spr_opaque && x != 255 {
            // Left-column rules: hit only triggers at x>=8 unless both left-show bits enabled.
            let show_bg_left = self.ppumask & 0x02 != 0;
            let show_spr_left = self.ppumask & 0x04 != 0;
            if x >= 8 || (show_bg_left && show_spr_left) {
                self.sprite_zero_hit = true;
            }
        }

        // Pixel priority mux (spec §5d).
        let final_palette_byte = match (bg_opaque, spr_opaque) {
            (false, false) => self.palette_ram[0],
            (true, false) => bg_palette_byte,
            (false, true) => spr_palette_byte,
            (true, true) => {
                if spr_priority {
                    spr_palette_byte
                } else {
                    bg_palette_byte
                }
            }
        };

        let (r, g, b) = NES_PALETTE[(final_palette_byte & 0x3F) as usize];
        let off = (y as usize * SCREEN_WIDTH + x as usize) * 4;
        self.framebuffer[off] = r;
        self.framebuffer[off + 1] = g;
        self.framebuffer[off + 2] = b;
        self.framebuffer[off + 3] = 0xFF;
    }
}
