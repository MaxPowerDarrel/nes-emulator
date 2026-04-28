//! Mapper 2 — UxROM. Switchable 16 KB PRG bank at $8000, fixed last bank at $C000.
//!
//! Spec: https://www.nesdev.org/wiki/UxROM

use super::{CartridgeError, CpuTiming, Mapper, Mirroring, RomHeader};

pub struct Mapper2 {
    prg_rom: Vec<u8>,
    chr_ram: [u8; 8192],
    bank_select: usize,
    last_bank: usize,
    mirroring: Mirroring,
    #[allow(dead_code)]
    submapper: u8,
    #[allow(dead_code)]
    timing: CpuTiming,
}

impl Mapper2 {
    pub fn new(
        header: &RomHeader,
        prg_rom: Vec<u8>,
        _chr_rom: Vec<u8>,
    ) -> Result<Self, CartridgeError> {
        if prg_rom.is_empty() {
            return Err(CartridgeError::TooShort);
        }
        let num_banks = prg_rom.len() / 16384;
        Ok(Self {
            last_bank: num_banks.saturating_sub(1),
            prg_rom,
            chr_ram: [0u8; 8192],
            bank_select: 0,
            mirroring: header.mirroring,
            submapper: header.submapper,
            timing: header.timing,
        })
    }
}

impl Mapper for Mapper2 {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let offset = self.bank_select * 16384 + (addr - 0x8000) as usize;
                Some(self.prg_rom[offset])
            }
            0xC000..=0xFFFF => {
                let offset = self.last_bank * 16384 + (addr - 0xC000) as usize;
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            let num_banks = self.prg_rom.len() / 16384;
            let mask = if num_banks > 0 { num_banks - 1 } else { 0 };
            self.bank_select = (val as usize) & mask;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if addr < 0x2000 {
            Some(self.chr_ram[(addr & 0x1FFF) as usize])
        } else {
            None
        }
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            self.chr_ram[(addr & 0x1FFF) as usize] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }
    fn submapper(&self) -> u8 {
        self.submapper
    }
    fn timing(&self) -> CpuTiming {
        self.timing
    }
}
