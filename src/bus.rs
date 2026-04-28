/// NES CPU + PPU memory buses.
///
/// CPU map: https://www.nesdev.org/wiki/CPU_memory_map
/// PPU map: https://www.nesdev.org/wiki/PPU_memory_map
///
/// CPU address map:
///   $0000–$07FF  2 KB CPU RAM (mirrored through $1FFF)
///   $2000–$3FFF  PPU registers (8 registers, mirrored every 8 bytes)
///   $4000–$4017  APU / I/O (open bus stub)
///   $4018–$5FFF  Open bus stub
///   $6000–$7FFF  WRAM (8 KB)
///   $8000–$FFFF  Routed through mapper

use crate::cartridge::{Mapper, Mirroring};
use crate::ppu::Ppu;

const RAM_SIZE: usize = 2048;
const RAM_MASK: u16 = 0x07FF;
const RAM_END: u16 = 0x1FFF;
const WRAM_START: u16 = 0x6000;
const WRAM_END: u16 = 0x7FFF;
const WRAM_SIZE: usize = 8192;

pub struct Bus {
    ram: [u8; RAM_SIZE],
    /// Work RAM at $6000–$7FFF.
    wram: [u8; WRAM_SIZE],
    /// Physical nametable VRAM — 2 KB, shared by PPU slots $2000/$2400/$2800/$2C00
    /// according to the cartridge's mirroring mode.
    pub nametable_vram: [u8; 2048],
    pub mapper: Box<dyn Mapper>,
    pub ppu: Ppu,

    /// OAM DMA pending flag — set by a write to $4014; consumed by the main loop.
    /// Spec: https://www.nesdev.org/wiki/PPU_registers#OAM_DMA_.28.244014.29_write
    pub oam_dma_pending: bool,
    pub oam_dma_page: u8,
}

impl Bus {
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        Self {
            ram: [0u8; RAM_SIZE],
            wram: [0u8; WRAM_SIZE],
            nametable_vram: [0u8; 2048],
            mapper,
            ppu: Ppu::new(),
            oam_dma_pending: false,
            oam_dma_page: 0,
        }
    }

    /// CPU bus read. PPU register reads have side effects (e.g. PPUSTATUS clears VBlank),
    /// so this requires &mut self.
    pub fn read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=RAM_END => self.ram[(addr & RAM_MASK) as usize],
            0x2000..=0x3FFF => {
                let reg = (addr & 0x07) as u8;
                // Split-field borrow: ppu (mut) vs nametable_vram + mapper (shared).
                let nt = &self.nametable_vram;
                let mapper = self.mapper.as_ref();
                self.ppu.register_read(reg, nt, mapper)
            }
            0x4000..=0x4017 => 0xFF, // APU/IO stub
            0x4018..=0x5FFF => 0xFF, // open bus
            WRAM_START..=WRAM_END => self.wram[(addr - WRAM_START) as usize],
            0x8000..=0xFFFF => self.mapper.cpu_read(addr).unwrap_or(0xFF),
        }
    }

    pub fn read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    /// JMP ($xxFF) hardware bug: high byte wraps within the same page.
    pub fn read_u16_page_wrap(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi_addr = (addr & 0xFF00) | ((addr.wrapping_add(1)) & 0x00FF);
        let hi = self.read(hi_addr) as u16;
        (hi << 8) | lo
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=RAM_END => self.ram[(addr & RAM_MASK) as usize] = val,
            0x2000..=0x3FFF => {
                let reg = (addr & 0x07) as u8;
                // Split-field borrow: ppu (mut) vs nametable_vram (mut) vs mapper (mut).
                self.ppu.register_write(
                    reg,
                    val,
                    &mut self.nametable_vram,
                    self.mapper.as_mut(),
                );
            }
            0x4014 => {
                // OAM DMA: defer the actual transfer to the main loop so it can
                // also stall the CPU correctly.
                self.oam_dma_pending = true;
                self.oam_dma_page = val;
            }
            0x4000..=0x4013 | 0x4015..=0x4017 => {} // APU/IO stub
            0x4018..=0x5FFF => {} // open bus
            WRAM_START..=WRAM_END => self.wram[(addr - WRAM_START) as usize] = val,
            0x8000..=0xFFFF => self.mapper.cpu_write(addr, val),
        }
    }

    // ── PPU bus ──────────────────────────────────────────────────────────────
    //
    // PPU memory map (spec §4):
    //   $0000–$1FFF  pattern tables → mapper
    //   $2000–$3EFF  nametables → 2 KB physical VRAM, mirrored per cartridge
    //   $3F00–$3FFF  palette RAM (owned by the PPU itself)
    //
    // These methods are not on the rendering hot path — `Ppu::tick` uses split-field
    // borrows directly via the free functions below to avoid borrowing all of `Bus`.
    // They satisfy spec §9's API and are available for future DMA, debugger, and
    // test paths.

    #[allow(dead_code)]
    pub fn ppu_read(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.mapper.ppu_read(addr).unwrap_or(0),
            0x2000..=0x3EFF => {
                self.nametable_vram[nametable_index(addr, self.mapper.mirroring())]
            }
            0x3F00..=0x3FFF => self.ppu.palette_ram[palette_addr(addr)],
            _ => 0,
        }
    }

    #[allow(dead_code)]
    pub fn ppu_write(&mut self, addr: u16, val: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.mapper.ppu_write(addr, val),
            0x2000..=0x3EFF => {
                let mirroring = self.mapper.mirroring();
                self.nametable_vram[nametable_index(addr, mirroring)] = val;
            }
            0x3F00..=0x3FFF => {
                self.ppu.palette_ram[palette_addr(addr)] = val;
            }
            _ => {}
        }
    }
}

// ── PPU bus routing helpers ─────────────────────────────────────────────────
//
// Free functions so `Ppu::tick` can call them without borrowing `Bus` (which
// would conflict with the existing `&mut bus.ppu` borrow per spec §9).

/// Map a PPU nametable address ($2000–$3EFF) to an index into the 2 KB physical VRAM.
///
/// Source: https://www.nesdev.org/wiki/Mirroring
pub(crate) fn nametable_index(addr: u16, mirroring: Mirroring) -> usize {
    let addr = (addr - 0x2000) & 0x0FFF;
    let table = addr / 0x400;
    let offset = (addr % 0x400) as usize;
    let bank = match mirroring {
        Mirroring::Horizontal => table / 2, // slots 0,1 → bank A; 2,3 → bank B
        Mirroring::Vertical => table & 1,   // slots 0,2 → bank A; 1,3 → bank B
        Mirroring::FourScreen => table & 1, // no cart VRAM for NROM; treat as vertical
    };
    (bank as usize) * 0x400 + offset
}

/// Collapse a palette RAM address (low 5 bits) handling sprite-transparent mirrors:
/// $10/$14/$18/$1C → $00/$04/$08/$0C.
///
/// Source: https://www.nesdev.org/wiki/PPU_memory_map#Palette_RAM
pub(crate) fn palette_addr(addr: u16) -> usize {
    let idx = (addr & 0x1F) as usize;
    match idx {
        0x10 | 0x14 | 0x18 | 0x1C => idx & 0x0F,
        _ => idx,
    }
}