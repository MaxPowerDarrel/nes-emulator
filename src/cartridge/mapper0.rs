/// Mapper 0 — NROM. No bank switching; fixed PRG and CHR windows.
///
/// Spec: https://www.nesdev.org/wiki/NROM
///
/// CPU bus:
///   $8000–$BFFF  PRG-ROM bank 0 (first 16 KB)
///   $C000–$FFFF  PRG-ROM bank 1 (NROM-256), or mirror of bank 0 (NROM-128)
///
/// PPU bus:
///   $0000–$1FFF  CHR-ROM (or CHR-RAM sized from header when chr_rom is empty)

use super::{CartridgeError, CpuTiming, Mapper, Mirroring, RomHeader};

const PRG_WINDOW_START: u16 = 0x8000;
const CHR_MASK: u16 = 0x1FFF;

pub struct Mapper0 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_mask: usize,
    /// True when the cartridge supplies no CHR-ROM and we use writable CHR-RAM instead.
    chr_ram: bool,
    #[allow(dead_code)]
    mirroring: Mirroring,
    #[allow(dead_code)]
    submapper: u8,
    #[allow(dead_code)]
    timing: CpuTiming,
}

impl Mapper0 {
    pub fn new(
        header: &RomHeader,
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
    ) -> Result<Self, CartridgeError> {
        let (chr, chr_ram) = if chr_rom.is_empty() {
            let size = header.chr_ram_size;
            if size == 0 || !size.is_power_of_two() {
                return Err(CartridgeError::InvalidSizeEncoding);
            }
            (vec![0u8; size], true)
        } else {
            (chr_rom, false)
        };
        let chr_mask = chr.len() - 1;
        Ok(Self {
            prg_rom,
            chr,
            chr_mask,
            chr_ram,
            mirroring: header.mirroring,
            submapper: header.submapper,
            timing: header.timing,
        })
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
        Some(self.chr[(addr as usize) & self.chr_mask])
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if self.chr_ram && addr <= CHR_MASK {
            self.chr[(addr as usize) & self.chr_mask] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn submapper(&self) -> u8 { self.submapper }

    fn timing(&self) -> CpuTiming { self.timing }
}
