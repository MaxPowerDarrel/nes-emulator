/// Mapper 4 — MMC3 (TxROM). Switchable PRG/CHR banks with scanline IRQ counter.
///
/// Spec: https://www.nesdev.org/wiki/MMC3

use super::{CartridgeError, CpuTiming, Mapper, Mirroring, RomHeader};

pub struct Mapper4 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    chr_ram: bool,
    prg_ram: Vec<u8>,
    prg_ram_enabled: bool,
    prg_ram_write_protect: bool,

    /// Last $8000 write (bank select register).
    bank_select: u8,
    /// R0–R7 bank registers.
    banks: [u8; 8],

    mirroring: Mirroring,
    /// True if flags6 bit 3 set — four-screen VRAM; ignore $A000 mirroring register.
    header_four_screen: bool,

    irq_latch: u8,
    irq_counter: u8,
    irq_enabled: bool,
    irq_reload: bool,
    irq_pending: bool,
    /// Previous A12 state — retained for spec reference; unused in scanline-driven mode.
    #[allow(dead_code)]
    a12_prev: bool,

    #[allow(dead_code)]
    submapper: u8,
    #[allow(dead_code)]
    timing: CpuTiming,
}

impl Mapper4 {
    pub fn new(
        header: &RomHeader,
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
    ) -> Result<Self, CartridgeError> {
        if prg_rom.is_empty() {
            return Err(CartridgeError::TooShort);
        }
        let (chr, chr_ram) = if chr_rom.is_empty() {
            let size = if header.chr_ram_size > 0 { header.chr_ram_size } else { 8 * 1024 };
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
            prg_ram_enabled: false,
            prg_ram_write_protect: false,
            bank_select: 0,
            banks: [0u8; 8],
            mirroring: header.mirroring,
            header_four_screen: header.mirroring == Mirroring::FourScreen,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_reload: false,
            irq_pending: false,
            a12_prev: false,
            submapper: header.submapper,
            timing: header.timing,
        })
    }

    fn prg_mode(&self) -> u8 { (self.bank_select >> 6) & 0x01 }
    fn chr_mode(&self) -> u8 { (self.bank_select >> 7) & 0x01 }

    fn num_prg_banks_8k(&self) -> usize { self.prg_rom.len() / 8192 }

    fn prg_addr(&self, addr: u16) -> usize {
        let n = self.num_prg_banks_8k();
        let last = n.saturating_sub(1);
        let second_last = n.saturating_sub(2);
        let (bank, offset) = match addr {
            0x8000..=0x9FFF => {
                let b = if self.prg_mode() == 0 {
                    (self.banks[6] as usize) & (n.saturating_sub(1))
                } else {
                    second_last
                };
                (b, (addr - 0x8000) as usize)
            }
            0xA000..=0xBFFF => {
                let b = (self.banks[7] as usize) & (n.saturating_sub(1));
                (b, (addr - 0xA000) as usize)
            }
            0xC000..=0xDFFF => {
                let b = if self.prg_mode() == 0 {
                    second_last
                } else {
                    (self.banks[6] as usize) & (n.saturating_sub(1))
                };
                (b, (addr - 0xC000) as usize)
            }
            0xE000..=0xFFFF => (last, (addr - 0xE000) as usize),
            _ => unreachable!(),
        };
        bank * 8192 + offset
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let chr_len = self.chr.len().max(1);
        if self.chr_mode() == 0 {
            // 2 KB banks at $0000/$0800; 1 KB banks at $1000/$1400/$1800/$1C00
            match addr {
                0x0000..=0x07FF => {
                    let bank = (self.banks[0] & !1) as usize;
                    (bank * 1024 + (addr & 0x07FF) as usize) % chr_len
                }
                0x0800..=0x0FFF => {
                    let bank = (self.banks[1] & !1) as usize;
                    (bank * 1024 + (addr & 0x07FF) as usize) % chr_len
                }
                0x1000..=0x13FF => {
                    let bank = self.banks[2] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                0x1400..=0x17FF => {
                    let bank = self.banks[3] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                0x1800..=0x1BFF => {
                    let bank = self.banks[4] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                0x1C00..=0x1FFF => {
                    let bank = self.banks[5] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                _ => 0,
            }
        } else {
            // CHR mode 1: swap halves — 1 KB banks at $0000–$0FFF, 2 KB banks at $1000–$1FFF
            match addr {
                0x0000..=0x03FF => {
                    let bank = self.banks[2] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                0x0400..=0x07FF => {
                    let bank = self.banks[3] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                0x0800..=0x0BFF => {
                    let bank = self.banks[4] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                0x0C00..=0x0FFF => {
                    let bank = self.banks[5] as usize;
                    (bank * 1024 + (addr & 0x03FF) as usize) % chr_len
                }
                0x1000..=0x17FF => {
                    let bank = (self.banks[0] & !1) as usize;
                    (bank * 1024 + (addr & 0x07FF) as usize) % chr_len
                }
                0x1800..=0x1FFF => {
                    let bank = (self.banks[1] & !1) as usize;
                    (bank * 1024 + (addr & 0x07FF) as usize) % chr_len
                }
                _ => 0,
            }
        }
    }

    /// Clock the IRQ counter. On real hardware this is driven by A12 rising edges
    /// during PPU sprite fetches (~once per scanline when BG=$0000, sprites=$1000).
    /// Since our PPU renders per-pixel rather than modelling the real fetch order,
    /// A12 toggles many times per scanline and can't drive the counter directly.
    /// Instead, the PPU calls this once per visible/pre-render scanline.
    ///
    /// Spec: https://www.nesdev.org/wiki/MMC3#IRQ_Specifics
    fn clock_scanline_counter(&mut self) {
        if self.irq_reload || self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
        }
        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_pending = true;
        }
    }
}

impl Mapper for Mapper4 {
    fn cpu_read(&self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => {
                if !self.prg_ram_enabled || self.prg_ram.is_empty() {
                    Some(0xFF)
                } else {
                    let idx = ((addr - 0x6000) as usize) % self.prg_ram.len();
                    Some(self.prg_ram[idx])
                }
            }
            0x8000..=0xFFFF => {
                let offset = self.prg_addr(addr);
                Some(self.prg_rom[offset])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled && !self.prg_ram_write_protect && !self.prg_ram.is_empty() {
                    let idx = ((addr - 0x6000) as usize) % self.prg_ram.len();
                    self.prg_ram[idx] = val;
                }
            }
            0x8000..=0x9FFF => {
                if addr & 0x01 == 0 {
                    // $8000 even — bank select
                    self.bank_select = val;
                } else {
                    // $8001 odd — bank data
                    let reg = (self.bank_select & 0x07) as usize;
                    self.banks[reg] = val;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 0x01 == 0 {
                    // $A000 even — mirroring (ignored for four-screen)
                    if !self.header_four_screen {
                        self.mirroring = if val & 0x01 == 0 {
                            Mirroring::Vertical
                        } else {
                            Mirroring::Horizontal
                        };
                    }
                } else {
                    // $A001 odd — PRG RAM protect
                    self.prg_ram_enabled = val & 0x80 != 0;
                    self.prg_ram_write_protect = val & 0x40 != 0;
                }
            }
            0xC000..=0xDFFF => {
                if addr & 0x01 == 0 {
                    self.irq_latch = val;
                } else {
                    self.irq_reload = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 0x01 == 0 {
                    // $E000 — IRQ disable and acknowledge
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    // $E001 — IRQ enable
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if addr < 0x2000 {
            let offset = self.chr_addr(addr);
            Some(self.chr[offset])
        } else {
            None
        }
    }

    fn ppu_write(&mut self, addr: u16, val: u8) {
        if addr < 0x2000 && self.chr_ram {
            let offset = self.chr_addr(addr);
            self.chr[offset] = val;
        }
    }

    fn notify_scanline(&mut self) {
        self.clock_scanline_counter();
    }

    fn mirroring(&self) -> Mirroring { self.mirroring }

    fn poll_irq(&mut self) -> bool {
        let pending = self.irq_pending;
        self.irq_pending = false;
        pending
    }

    fn submapper(&self) -> u8 { self.submapper }
    fn timing(&self) -> CpuTiming { self.timing }
}