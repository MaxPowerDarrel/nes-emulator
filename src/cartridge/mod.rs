/// iNES cartridge loader and mapper dispatch.
///
/// Spec: https://www.nesdev.org/wiki/INES
///       https://www.nesdev.org/wiki/Mapper
///
/// iNES 1.0 header (16 bytes):
///   Bytes 0-3:  "NES\x1A" magic
///   Byte  4:    PRG-ROM size in 16 KB units
///   Byte  5:    CHR-ROM size in 8 KB units (0 = CHR-RAM)
///   Byte  6:    Flags 6 (mapper low nibble, mirroring, battery, trainer, four-screen)
///   Byte  7:    Flags 7 (mapper high nibble, NES 2.0 indicator)
///   Bytes 8-15: Padding (iNES 1.0)

pub mod mapper0;

use mapper0::Mapper0;

const INES_MAGIC: &[u8; 4] = b"NES\x1A";
const HEADER_SIZE: usize = 16;
const PRG_BANK_SIZE: usize = 16 * 1024;
const CHR_BANK_SIZE: usize = 8 * 1024;
const TRAINER_SIZE: usize = 512;

/// Nametable mirroring mode declared by the cartridge header (or by a mapper at runtime).
///
/// Spec: https://www.nesdev.org/wiki/Mirroring
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    /// Cartridge supplies its own VRAM (flags6 bit 3). No mapper in scope uses it yet.
    FourScreen,
}

/// Mapper interface — abstracts the cartridge's memory mapping for both CPU and PPU buses.
///
/// `cpu_read` / `ppu_read` return `None` to indicate "open bus" (the bus should fall back
/// to its own default, typically the last value on the bus or 0).
pub trait Mapper {
    /// CPU bus read for $4020–$FFFF.
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    /// CPU bus write for $4020–$FFFF.
    fn cpu_write(&mut self, addr: u16, val: u8);
    /// PPU bus read for $0000–$1FFF (pattern tables).
    fn ppu_read(&self, addr: u16) -> Option<u8>;
    /// PPU bus write for $0000–$1FFF.
    fn ppu_write(&mut self, addr: u16, val: u8);
    /// Nametable mirroring mode declared by the header. Consumed by the PPU in Milestone 3.
    #[allow(dead_code)]
    fn mirroring(&self) -> Mirroring;
}

#[derive(Debug)]
pub enum CartridgeError {
    TooShort,
    BadMagic,
    Nes2NotSupported,
    UnsupportedMapper(u8),
}

impl std::fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CartridgeError::TooShort => write!(f, "ROM file too short"),
            CartridgeError::BadMagic => write!(f, "not an iNES ROM (bad magic)"),
            CartridgeError::Nes2NotSupported => write!(f, "NES 2.0 ROMs not supported"),
            CartridgeError::UnsupportedMapper(m) => write!(f, "unsupported mapper {}", m),
        }
    }
}

/// Parse an iNES 1.0 ROM and dispatch to the matching `Mapper` implementation.
///
/// Spec: https://www.nesdev.org/wiki/INES
pub fn from_bytes(data: &[u8]) -> Result<Box<dyn Mapper>, CartridgeError> {
    if data.len() < HEADER_SIZE {
        return Err(CartridgeError::TooShort);
    }
    if &data[0..4] != INES_MAGIC {
        return Err(CartridgeError::BadMagic);
    }

    let prg_banks = data[4] as usize;
    let chr_banks = data[5] as usize;
    let flags6 = data[6];
    let flags7 = data[7];

    // NES 2.0 detection: flags7 bits 3-2 == 0b10.
    // Spec: https://www.nesdev.org/wiki/NES_2.0
    if flags7 & 0x0C == 0x08 {
        return Err(CartridgeError::Nes2NotSupported);
    }

    let mapper_id = (flags7 & 0xF0) | (flags6 >> 4);

    let has_trainer = flags6 & 0x04 != 0;
    let prg_start = HEADER_SIZE + if has_trainer { TRAINER_SIZE } else { 0 };
    let prg_end = prg_start + prg_banks * PRG_BANK_SIZE;
    if prg_end > data.len() {
        return Err(CartridgeError::TooShort);
    }
    let prg_rom = data[prg_start..prg_end].to_vec();

    let chr_rom = if chr_banks > 0 {
        let chr_start = prg_end;
        let chr_end = chr_start + chr_banks * CHR_BANK_SIZE;
        if chr_end > data.len() {
            return Err(CartridgeError::TooShort);
        }
        data[chr_start..chr_end].to_vec()
    } else {
        Vec::new()
    };

    let mirroring = if flags6 & 0x08 != 0 {
        Mirroring::FourScreen
    } else if flags6 & 0x01 != 0 {
        Mirroring::Vertical
    } else {
        Mirroring::Horizontal
    };

    match mapper_id {
        0 => Ok(Box::new(Mapper0::new(prg_rom, chr_rom, mirroring))),
        other => Err(CartridgeError::UnsupportedMapper(other)),
    }
}
