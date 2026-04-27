/// Mapper 0 — NROM. No bank switching; fixed PRG and CHR windows.
///
/// Spec: https://www.nesdev.org/wiki/NROM
///
/// CPU bus:
///   $8000–$BFFF  PRG-ROM bank 0 (first 16 KB)
///   $C000–$FFFF  PRG-ROM bank 1 (NROM-256), or mirror of bank 0 (NROM-128)
///
/// PPU bus:
///   $0000–$1FFF  CHR-ROM (or 8 KB CHR-RAM if header chr_size == 0)

use super::{Mapper, Mirroring};

const PRG_WINDOW_START: u16 = 0x8000;
const CHR_RAM_SIZE: usize = 8 * 1024;
const CHR_MASK: u16 = 0x1FFF;

pub struct Mapper0 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    /// True when the cartridge supplies no CHR-ROM and we use writable CHR-RAM instead.
    chr_ram: bool,
    #[allow(dead_code)]
    mirroring: Mirroring,
}

impl Mapper0 {
    pub fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let (chr, chr_ram) = if chr_rom.is_empty() {
            (vec![0u8; CHR_RAM_SIZE], true)
        } else {
            (chr_rom, false)
        };
        Self { prg_rom, chr, chr_ram, mirroring }
    }
}

impl Mapper for Mapper0 {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        if addr < PRG_WINDOW_START {
            return None;
        }
        let offset = (addr - PRG_WINDOW_START) as usize % self.prg_rom.len();
        Some(self.prg_rom[offset])
    }

    fn cpu_write(&mut self, _addr: u16, _val: u8) {
        // NROM has no registers; writes to PRG window are silently ignored.
    }

    fn ppu_read(&self, addr: u16) -> Option<u8> {
        if addr > CHR_MASK {
            return None;
        }
        Some(self.chr[(addr & CHR_MASK) as usize])
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if self.chr_ram && addr <= CHR_MASK {
            self.chr[(addr & CHR_MASK) as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
}
