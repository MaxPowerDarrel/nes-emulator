/// NES PPU (Picture Processing Unit) — 2C02.
///
/// Spec refs:
///   https://www.nesdev.org/wiki/PPU
///   https://www.nesdev.org/wiki/PPU_registers
///   https://www.nesdev.org/wiki/PPU_scrolling  (Loopy registers)
///   https://www.nesdev.org/wiki/PPU_rendering
///   https://www.nesdev.org/wiki/PPU_memory_map
///   https://www.nesdev.org/wiki/PPU_palettes

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
    pub nmi_output: bool,   // PPUCTRL bit 7 (NMI enable; wired in Milestone 4)
    pub nmi_occurred: bool, // VBlank flag — bit 7 of PPUSTATUS

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
            palette_ram: [0; 32],
            read_buffer: 0,
            open_bus: 0,
            framebuffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT * 4],
            frame_complete: false,
        }
    }

    fn rendering_enabled(&self) -> bool {
        self.ppumask & 0x08 != 0
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
            // Side effects: clear VBlank flag; reset w latch.
            2 => {
                let status = ((self.nmi_occurred as u8) << 7) | (self.open_bus & 0x1F);
                self.nmi_occurred = false;
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
                    // Palette: bypass buffer; buffer gets nametable data at mirrored addr.
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
            0 => {
                self.ppuctrl = val;
                self.nmi_output = val & 0x80 != 0;
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
                    // First write: fine X → x; coarse X → t[4:0]
                    self.x = val & 0x07;
                    self.t = (self.t & 0xFFE0) | ((val >> 3) as u16);
                } else {
                    // Second write: fine Y → t[14:12]; coarse Y → t[9:5]
                    self.t = (self.t & 0x8FFF) | (((val & 0x07) as u16) << 12);
                    self.t = (self.t & 0xFC1F) | (((val >> 3) as u16) << 5);
                }
                self.w = !self.w;
            }
            // $2006 PPUADDR (two writes via w latch)
            6 => {
                if !self.w {
                    // First write: high 6 bits → t[13:8]; clear t[14]
                    self.t = (self.t & 0x00FF) | (((val & 0x3F) as u16) << 8);
                    self.t &= 0x3FFF;
                } else {
                    // Second write: low 8 bits → t[7:0]; copy t → v
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

    /// Advance the PPU by one dot.
    ///
    /// Called from the master clock loop — 3 times per CPU cycle.
    /// Signature uses split-field borrows so Bus can pass nametable_vram and mapper
    /// while ppu itself is mutably borrowed.
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

        // Pre-render scanline: copy t → v so scroll set during VBlank takes effect.
        // Milestone 3 approximation — cycle-accurate per-scanline updates deferred to M4.
        if self.scanline == 261 && self.dot == 0 {
            self.v = self.t;
        }

        // Clear VBlank/status flags at pre-render scanline dot 1.
        if self.scanline == 261 && self.dot == 1 {
            self.nmi_occurred = false;
            self.frame_complete = false;
        }

        // Render background pixel during visible scanlines.
        if self.scanline < 240 && self.dot >= 1 && self.dot <= 256 {
            let px = self.dot - 1;
            let py = self.scanline;
            if self.rendering_enabled() {
                self.render_pixel(py, px, nametable_vram, mapper);
            } else {
                let color_idx = self.palette_ram[0] & 0x3F;
                let (r, g, b) = NES_PALETTE[color_idx as usize];
                let off = (py as usize * SCREEN_WIDTH + px as usize) * 4;
                self.framebuffer[off] = r;
                self.framebuffer[off + 1] = g;
                self.framebuffer[off + 2] = b;
                self.framebuffer[off + 3] = 0xFF;
            }
        }

        // VBlank start: scanline 241, dot 1. Frame pixels are complete at this point.
        if self.scanline == 241 && self.dot == 1 {
            self.nmi_occurred = true;
            self.frame_complete = true;
            // NMI wire-up deferred to Milestone 4.
        }
    }

    /// Render one background pixel at screen position (y, x).
    ///
    /// Computes the tile position from v (set at pre-render) + fine-X scroll register,
    /// fetches tile ID + attribute from nametable, looks up pattern and palette,
    /// then writes RGBA to the framebuffer.
    fn render_pixel(&mut self, y: u16, x: u16, nametable_vram: &[u8; 2048], mapper: &dyn Mapper) {
        // Left-8-pixel masking: PPUMASK bit 1 (m) — hide BG in leftmost 8 columns.
        if x < 8 && self.ppumask & 0x02 == 0 {
            let color_idx = self.palette_ram[0] & 0x3F;
            let (r, g, b) = NES_PALETTE[color_idx as usize];
            let off = (y as usize * SCREEN_WIDTH + x as usize) * 4;
            self.framebuffer[off] = r;
            self.framebuffer[off + 1] = g;
            self.framebuffer[off + 2] = b;
            self.framebuffer[off + 3] = 0xFF;
            return;
        }

        // Extract initial scroll state from v (set at pre-render).
        // v layout: yyy NN YYYYY XXXXX (bits 14–0)
        let initial_coarse_x = (self.v & 0x001F) as usize;
        let initial_coarse_y = ((self.v >> 5) & 0x001F) as usize;
        let initial_fine_y = ((self.v >> 12) & 0x0007) as usize;
        let nt_x_init = (self.v >> 10) & 1;
        let nt_y_init = (self.v >> 11) & 1;

        // Horizontal position for this pixel, accounting for fine-X scroll.
        let total_x = x as usize + self.x as usize;
        let fine_x = (total_x % 8) as u16;
        let tile_col_offset = total_x / 8;

        // Vertical position for this scanline, accounting for initial fine-Y.
        let total_y = initial_fine_y + y as usize;
        let fine_y = (total_y % 8) as u16;
        let tile_row_offset = total_y / 8;

        // Effective tile coordinates with nametable wraparound.
        let eff_coarse_x_abs = initial_coarse_x + tile_col_offset;
        let eff_coarse_y_abs = initial_coarse_y + tile_row_offset;

        // Horizontal: nametable wraps at 32 tiles.
        let nt_x = nt_x_init ^ ((eff_coarse_x_abs >= 32) as u16);
        let eff_coarse_x = (eff_coarse_x_abs % 32) as u16;

        // Vertical: nametable wraps at 30 tile rows.
        let nt_y = nt_y_init ^ ((eff_coarse_y_abs >= 30) as u16);
        let eff_coarse_y = (eff_coarse_y_abs % 30) as u16;

        // Construct a virtual v for address generation.
        let virt_v = (nt_y << 11) | (nt_x << 10) | (eff_coarse_y << 5) | eff_coarse_x;

        // Nametable fetch: tile ID.
        let nt_addr = 0x2000 | (virt_v & 0x0FFF);
        let tile_id = self.ppu_read_addr(nt_addr, nametable_vram, mapper) as u16;

        // Attribute table fetch: palette select.
        // at_addr = $23C0 | nt_select | (coarse_Y/4 << 3) | (coarse_X/4)
        let at_addr = 0x23C0
            | (virt_v & 0x0C00)
            | ((virt_v >> 4) & 0x0038)
            | ((virt_v >> 2) & 0x0007);
        let attr_byte = self.ppu_read_addr(at_addr, nametable_vram, mapper);

        // 2×2 quadrant within the 4×4 tile attribute block.
        let x_quad = (eff_coarse_x / 2) & 1;
        let y_quad = (eff_coarse_y / 2) & 1;
        let shift = (y_quad * 2 + x_quad) * 2;
        let palette_select = ((attr_byte >> shift) & 0x03) as usize;

        // Pattern table fetch: low and high bit-planes for this tile row.
        let pt_base = self.bg_pattern_table_base();
        let low_byte = self.ppu_read_addr(pt_base + tile_id * 16 + fine_y, nametable_vram, mapper);
        let high_byte =
            self.ppu_read_addr(pt_base + tile_id * 16 + fine_y + 8, nametable_vram, mapper);

        // 2-bit color index: MSB of tile is leftmost pixel.
        let bit_pos = 7 - fine_x as u8;
        let low_bit = (low_byte >> bit_pos) & 1;
        let high_bit = (high_byte >> bit_pos) & 1;
        let color_idx = (high_bit << 1) | low_bit;

        // Palette lookup.
        let palette_byte = if color_idx == 0 {
            self.palette_ram[0]
        } else {
            self.palette_ram[palette_select * 4 + color_idx as usize]
        };
        let (r, g, b) = NES_PALETTE[(palette_byte & 0x3F) as usize];

        let off = (y as usize * SCREEN_WIDTH + x as usize) * 4;
        self.framebuffer[off] = r;
        self.framebuffer[off + 1] = g;
        self.framebuffer[off + 2] = b;
        self.framebuffer[off + 3] = 0xFF;
    }
}

