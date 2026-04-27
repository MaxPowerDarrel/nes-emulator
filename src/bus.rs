/// NES CPU + PPU memory buses.
///
/// CPU map: https://www.nesdev.org/wiki/CPU_memory_map
/// PPU map: https://www.nesdev.org/wiki/PPU_memory_map
///
/// CPU address map (Milestone 2):
///   $0000–$07FF  2 KB CPU RAM (mirrored through $1FFF)
///   $2000–$3FFF  PPU registers (open bus stub)
///   $4000–$4017  APU / I/O (open bus stub)
///   $4018–$5FFF  Open bus stub
///   $6000–$7FFF  WRAM (8 KB; nestest result codes live here)
///   $8000–$FFFF  Routed through `Mapper::cpu_read` / `cpu_write`
///
/// PPU address map (stub — wired in Milestone 3):
///   $0000–$1FFF  Pattern tables, routed through `Mapper::ppu_read` / `ppu_write`
///   $2000–$3EFF  Nametables (Milestone 3)
///   $3F00–$3FFF  Palette RAM (Milestone 3)

use crate::cartridge::Mapper;

const RAM_SIZE: usize = 2048;
const RAM_MASK: u16 = 0x07FF;
const RAM_END: u16 = 0x1FFF;
const WRAM_START: u16 = 0x6000;
const WRAM_END: u16 = 0x7FFF;
const WRAM_SIZE: usize = 8192;

pub struct Bus {
    ram: [u8; RAM_SIZE],
    /// Work RAM at $6000–$7FFF. Bus-owned (NROM has no PRG-RAM register logic).
    wram: [u8; WRAM_SIZE],
    mapper: Box<dyn Mapper>,
}

impl Bus {
    pub fn new(mapper: Box<dyn Mapper>) -> Self {
        Self {
            ram: [0u8; RAM_SIZE],
            wram: [0u8; WRAM_SIZE],
            mapper,
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=RAM_END => self.ram[(addr & RAM_MASK) as usize],
            0x2000..=0x3FFF => 0,    // PPU stub
            0x4000..=0x4017 => 0xFF, // APU/IO stub — open bus (write-only regs read as $FF)
            0x4018..=0x5FFF => 0xFF, // open bus
            WRAM_START..=WRAM_END => self.wram[(addr - WRAM_START) as usize],
            0x8000..=0xFFFF => self.mapper.cpu_read(addr).unwrap_or(0xFF),
        }
    }

    pub fn read_u16(&self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    /// JMP ($xxFF) hardware bug: high byte wraps within the same page.
    pub fn read_u16_page_wrap(&self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi_addr = (addr & 0xFF00) | ((addr.wrapping_add(1)) & 0x00FF);
        let hi = self.read(hi_addr) as u16;
        (hi << 8) | lo
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=RAM_END => self.ram[(addr & RAM_MASK) as usize] = val,
            0x2000..=0x3FFF => {} // PPU stub
            0x4000..=0x4017 => {} // APU/IO stub
            0x4018..=0x5FFF => {} // open bus
            WRAM_START..=WRAM_END => self.wram[(addr - WRAM_START) as usize] = val,
            0x8000..=0xFFFF => self.mapper.cpu_write(addr, val),
        }
    }

    // ── PPU bus (stub plumbing — exercised in Milestone 3) ───────────────────

    #[allow(dead_code)]
    pub fn ppu_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.mapper.ppu_read(addr).unwrap_or(0),
            0x2000..=0x3EFF => 0, // nametables — Milestone 3
            0x3F00..=0x3FFF => 0, // palette — Milestone 3
            _ => 0,
        }
    }

    #[allow(dead_code)]
    pub fn ppu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x0000..=0x1FFF => self.mapper.ppu_write(addr, val),
            0x2000..=0x3EFF => {} // nametables — Milestone 3
            0x3F00..=0x3FFF => {} // palette — Milestone 3
            _ => {}
        }
    }
}
