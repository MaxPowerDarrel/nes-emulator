/// Mapper 3 — CNROM. Fixed PRG, switchable 8 KB CHR bank.
///
/// Spec: https://www.nesdev.org/wiki/INES_Mapper_003

use super::{CartridgeError, CpuTiming, Mapper, Mirroring, RomHeader};

pub struct Mapper3 {
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    chr_bank: usize,
    mirroring: Mirroring,
    #[allow(dead_code)]
    submapper: u8,
    #[allow(dead_code)]
    timing: CpuTiming,
}

impl Mapper3 {
    pub fn new(
        header: &RomHeader,
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
    ) -> Result<Self, CartridgeError> {
        if prg_rom.is_empty() {
            return Err(CartridgeError::TooShort);
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            chr_bank: 0,
            mirroring: header.mirroring,
            submapper: header.submapper,
            timing: header.timing,
        })
    }
}

impl Mapper for Mapper3 {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        if addr >= 0x8000 {
            let offset = (addr - 0x8000) as usize % self.prg_rom.len();
            Some(self.prg_rom[offset])
        } else {
            None
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            let num_banks = (self.chr_rom.len() / 8192).max(1);
            self.chr_bank = (val as usize) & 0x03 & (num_banks - 1);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if addr < 0x2000 && !self.chr_rom.is_empty() {
            let offset = self.chr_bank * 8192 + (addr & 0x1FFF) as usize;
            Some(self.chr_rom[offset % self.chr_rom.len()])
        } else {
            None
        }
    }

    fn ppu_write(&mut self, _addr: u16, _val: u8) {
        // CHR-ROM is read-only
    }

    fn mirroring(&self) -> Mirroring { self.mirroring }
    fn submapper(&self) -> u8 { self.submapper }
    fn timing(&self) -> CpuTiming { self.timing }
}