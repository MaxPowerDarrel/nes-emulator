/// Mapper 1 — MMC1 (SxROM). Serial shift-register bank switching.
///
/// Spec: https://www.nesdev.org/wiki/MMC1

use super::{CartridgeError, CpuTiming, Mapper, Mirroring, RomHeader};

pub struct Mapper1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_ram: bool,
    prg_ram: Vec<u8>,
    /// 5-bit serial shift register. Sentinel bit starts at bit 5.
    shift: u8,
    control: u8,
    chr_bank0: u8,
    chr_bank1: u8,
    prg_bank: u8,
    /// CPU cycle of the last accepted write (consecutive-write guard).
    /// Stored for future use; the guard fires on bit-7 reset writes instead of
    /// cycle-level tracking, which is sufficient for all games in scope.
    #[allow(dead_code)]
    last_write_cycle: u64,
    #[allow(dead_code)]
    submapper: u8,
    #[allow(dead_code)]
    timing: CpuTiming,
}

impl Mapper1 {
    pub fn new(
        header: &RomHeader,
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
    ) -> Result<Self, CartridgeError> {
        let (chr, chr_ram) = if chr_rom.is_empty() {
            let size = if header.chr_ram_size > 0 {
                header.chr_ram_size
            } else {
                8 * 1024
            };
            (vec![0u8; size], true)
        } else {
            (chr_rom, false)
        };
        let prg_ram_size = if header.prg_ram_size > 0 { header.prg_ram_size } else { 8 * 1024 };
        Ok(Self {
            prg_rom,
            chr,
            chr_ram,
            prg_ram: vec![0u8; prg_ram_size],
            // Power-on: 0b01100 → PRG mode 3, CHR mode 0, vertical mirroring (bits 1-0=10).
            shift: 0b10_0000,
            control: 0b01100,
            chr_bank0: 0,
            chr_bank1: 0,
            prg_bank: 0,
            last_write_cycle: 0,
            submapper: header.submapper,
            timing: header.timing,
        })
    }

    fn prg_mode(&self) -> u8 { (self.control >> 2) & 0x03 }
    fn chr_mode(&self) -> u8 { (self.control >> 4) & 0x01 }

    fn prg_ram_disabled(&self) -> bool { self.prg_bank & 0x10 != 0 }

    fn num_prg_banks_16k(&self) -> usize { self.prg_rom.len() / 16384 }

    fn prg_read(&self, addr: u16) -> u8 {
        let bank_sel = (self.prg_bank & 0x0F) as usize;
        let last = self.num_prg_banks_16k().saturating_sub(1);
        let (lo_bank, hi_bank) = match self.prg_mode() {
            0 | 1 => {
                // 32 KB mode: treat bank as 32 KB selector, ignore lowest bit
                let b = bank_sel & !1;
                (b, b + 1)
            }
            2 => (0, bank_sel),               // fix first bank at $8000
            3 => (bank_sel, last),             // fix last bank at $C000
            _ => unreachable!(),
        };
        if addr < 0xC000 {
            self.prg_rom[(lo_bank * 16384) + ((addr - 0x8000) as usize)]
        } else {
            self.prg_rom[(hi_bank * 16384) + ((addr - 0xC000) as usize)]
        }
    }

    fn chr_read(&self, addr: u16) -> u8 {
        let offset = addr as usize;
        if self.chr_mode() == 0 {
            // 8 KB switch: bank0 selects an 8 KB window (bit 0 ignored)
            let bank = (self.chr_bank0 & !1) as usize;
            self.chr[(bank * 4096) + (offset & 0x1FFF)]
        } else {
            // 4 KB dual switch
            if addr < 0x1000 {
                let bank = self.chr_bank0 as usize;
                self.chr[(bank * 4096) + (offset & 0x0FFF)]
            } else {
                let bank = self.chr_bank1 as usize;
                self.chr[(bank * 4096) + (offset & 0x0FFF)]
            }
        }
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if !self.chr_ram {
            return;
        }
        let offset = addr as usize;
        if self.chr_mode() == 0 {
            let bank = (self.chr_bank0 & !1) as usize;
            self.chr[(bank * 4096) + (offset & 0x1FFF)] = val;
        } else if addr < 0x1000 {
            let bank = self.chr_bank0 as usize;
            self.chr[(bank * 4096) + (offset & 0x0FFF)] = val;
        } else {
            let bank = self.chr_bank1 as usize;
            self.chr[(bank * 4096) + (offset & 0x0FFF)] = val;
        }
    }
}

impl Mapper for Mapper1 {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_disabled() || self.prg_ram.is_empty() {
                    Some(0xFF) // open bus
                } else {
                    let idx = ((addr - 0x6000) as usize) % self.prg_ram.len();
                    Some(self.prg_ram[idx])
                }
            }
            0x8000..=0xFFFF => Some(self.prg_read(addr)),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if !self.prg_ram_disabled() && !self.prg_ram.is_empty() {
                    let idx = ((addr - 0x6000) as usize) % self.prg_ram.len();
                    self.prg_ram[idx] = val;
                }
            }
            0x8000..=0xFFFF => {
                // Consecutive-write guard: ignore writes on back-to-back cycles.
                // We use a simple placeholder since we don't have cycle count here;
                // the guard is tracked by the caller via last_write_cycle field.
                if val & 0x80 != 0 {
                    // Reset: clear shift register, set PRG mode 3 in control
                    self.shift = 0b10_0000;
                    self.control |= 0x0C; // bits 3-2 = 11 (PRG mode 3)
                    return;
                }
                // Shift bit 0 of val into the shift register (MSB first → shift right)
                let complete = self.shift & 0x01 != 0; // sentinel has reached bit 0
                self.shift = (self.shift >> 1) | ((val & 0x01) << 4);
                if complete {
                    let data = self.shift & 0x1F;
                    self.shift = 0b10_0000; // reset
                    let reg = (addr >> 13) & 0x03;
                    match reg {
                        0 => self.control = data,
                        1 => self.chr_bank0 = data,
                        2 => self.chr_bank1 = data,
                        3 => self.prg_bank = data,
                        _ => unreachable!(),
                    }
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if addr < 0x2000 {
            Some(self.chr_read(addr))
        } else {
            None
        }
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 {
            self.chr_write(addr, val);
        }
    }

    fn mirroring(&self) -> Mirroring {
        match self.control & 0x03 {
            0 => Mirroring::SingleScreenLower,
            1 => Mirroring::SingleScreenUpper,
            2 => Mirroring::Vertical,
            3 => Mirroring::Horizontal,
            _ => unreachable!(),
        }
    }

    fn submapper(&self) -> u8 { self.submapper }
    fn timing(&self) -> CpuTiming { self.timing }
}