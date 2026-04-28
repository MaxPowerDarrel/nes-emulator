/// iNES / NES 2.0 cartridge loader and mapper dispatch.
///
/// Specs:
///   iNES 1.0:  https://www.nesdev.org/wiki/INES
///   NES 2.0:   https://www.nesdev.org/wiki/NES_2.0
///   Mappers:   https://www.nesdev.org/wiki/Mapper
///   Submappers: https://www.nesdev.org/wiki/NES_2.0_submappers
///
/// Header (16 bytes) — see docs/spec-nes2-rom-support.md for full table.

pub mod mapper0;
pub mod mapper1;
pub mod mapper2;
pub mod mapper3;
pub mod mapper4;

use mapper0::Mapper0;
use mapper1::Mapper1;
use mapper2::Mapper2;
use mapper3::Mapper3;
use mapper4::Mapper4;

const INES_MAGIC: &[u8; 4] = b"NES\x1A";
const HEADER_SIZE: usize = 16;
const PRG_BANK_SIZE: usize = 16 * 1024;
const CHR_BANK_SIZE: usize = 8 * 1024;
const TRAINER_SIZE: usize = 512;
const DEFAULT_PRG_RAM_SIZE: usize = 8 * 1024;
const DEFAULT_CHR_RAM_SIZE: usize = 8 * 1024;
/// Sanity cap on PRG/CHR ROM sizes from exponent-mode encoding.
const MAX_ROM_BYTES: usize = 256 * 1024 * 1024;

/// Nametable mirroring mode declared by the cartridge header (or by a mapper at runtime).
///
/// Spec: https://www.nesdev.org/wiki/Mirroring
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    /// Cartridge supplies its own VRAM (flags6 bit 3). No mapper in scope uses it yet.
    FourScreen,
    /// All four nametable slots map to the lower 1 KB bank (MMC1 single-screen mode 0).
    SingleScreenLower,
    /// All four nametable slots map to the upper 1 KB bank (MMC1 single-screen mode 1).
    SingleScreenUpper,
}

/// ROM file format detected from the header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RomFormat {
    INes1,
    Nes2,
}

/// CPU/PPU timing region (NES 2.0 byte 12, low 2 bits).
///
/// Spec: https://www.nesdev.org/wiki/NES_2.0#CPU.2FPPU_Timing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuTiming {
    Ntsc,
    Pal,
    Multi,
    Dendy,
}

/// Identifies which ROM region a size error refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeKind {
    Prg,
    Chr,
}

/// Parsed cartridge header — superset of iNES 1.0 and NES 2.0 fields.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct RomHeader {
    pub format: RomFormat,
    pub mapper: u16,
    pub submapper: u8,
    pub prg_rom_size: usize,
    pub chr_rom_size: usize,
    pub prg_ram_size: usize,
    pub prg_nvram_size: usize,
    pub chr_ram_size: usize,
    pub chr_nvram_size: usize,
    pub mirroring: Mirroring,
    pub has_battery: bool,
    pub has_trainer: bool,
    pub timing: CpuTiming,
}

/// Mapper interface — abstracts the cartridge's memory mapping for both CPU and PPU buses.
pub trait Mapper {
    fn cpu_read(&self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, val: u8);
    /// PPU CHR read. Takes &mut self so mappers (e.g. MMC3) can track A12 transitions.
    fn ppu_read(&mut self, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, addr: u16, val: u8);
    #[allow(dead_code)]
    fn mirroring(&self) -> Mirroring;

    /// Submapper number from the NES 2.0 header (0 for iNES 1.0).
    #[allow(dead_code)]
    fn submapper(&self) -> u8 { 0 }

    /// CPU/PPU timing region. PPU still uses NTSC frame timing regardless — see
    /// docs/spec-nes2-rom-support.md §10.
    #[allow(dead_code)]
    fn timing(&self) -> CpuTiming { CpuTiming::Ntsc }

    /// Called on every PPU CHR address access. Returns true if the mapper is asserting
    /// a CPU IRQ this cycle (MMC3 scanline counter). Default: no-op, no IRQ.
    ///
    /// Spec: https://www.nesdev.org/wiki/MMC3#IRQ_Specifics
    fn poll_irq(&mut self) -> bool { false }

    /// Called once per visible/pre-render scanline by the PPU. Used by mappers
    /// (MMC3) that drive their IRQ counter off the PPU scanline clock in
    /// non-cycle-accurate fetch emulation. Default: no-op.
    fn notify_scanline(&mut self) {}
}

/// Final cartridge produced by `from_bytes` — a built mapper plus enough header
/// metadata for the bus to size its WRAM allocation.
pub struct Cartridge {
    pub mapper: Box<dyn Mapper>,
    pub prg_ram_size: usize,
}

#[derive(Debug)]
pub enum CartridgeError {
    TooShort,
    BadMagic,
    UnsupportedMapper(u16),
    UnsupportedConsoleType(u8),
    RomTooLarge { kind: SizeKind, bytes: usize },
    InvalidSizeEncoding,
}

impl std::fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CartridgeError::TooShort => write!(f, "ROM file too short"),
            CartridgeError::BadMagic => write!(f, "not an iNES ROM (bad magic)"),
            CartridgeError::UnsupportedMapper(m) => write!(f, "unsupported mapper {}", m),
            CartridgeError::UnsupportedConsoleType(c) => {
                write!(f, "unsupported console type {}", c)
            }
            CartridgeError::RomTooLarge { kind, bytes } => {
                let k = match kind { SizeKind::Prg => "PRG", SizeKind::Chr => "CHR" };
                write!(f, "{} ROM size too large: {} bytes", k, bytes)
            }
            CartridgeError::InvalidSizeEncoding => write!(f, "invalid ROM size encoding"),
        }
    }
}

/// Parse an iNES 1.0 or NES 2.0 ROM and dispatch to the matching `Mapper` implementation.
pub fn from_bytes(data: &[u8]) -> Result<Cartridge, CartridgeError> {
    if data.len() < HEADER_SIZE {
        return Err(CartridgeError::TooShort);
    }
    if &data[0..4] != INES_MAGIC {
        return Err(CartridgeError::BadMagic);
    }

    let flags7 = data[7];
    let header = if (flags7 & 0x0C) == 0x08 {
        // NES 2.0 console type: only NES/Famicom (0) is in scope.
        let console_type = flags7 & 0x03;
        if console_type != 0 {
            return Err(CartridgeError::UnsupportedConsoleType(console_type));
        }
        parse_nes2_header(&data[0..HEADER_SIZE])?
    } else {
        parse_ines1_header(&data[0..HEADER_SIZE])
    };

    let prg_start = HEADER_SIZE + if header.has_trainer { TRAINER_SIZE } else { 0 };
    let prg_end = prg_start.checked_add(header.prg_rom_size)
        .ok_or(CartridgeError::InvalidSizeEncoding)?;
    let chr_start = prg_end;
    let chr_end = chr_start.checked_add(header.chr_rom_size)
        .ok_or(CartridgeError::InvalidSizeEncoding)?;
    if data.len() < chr_end {
        return Err(CartridgeError::TooShort);
    }
    let prg_rom = data[prg_start..prg_end].to_vec();
    let chr_rom = data[chr_start..chr_end].to_vec();

    let mapper: Box<dyn Mapper> = match header.mapper {
        0 => Box::new(Mapper0::new(&header, prg_rom, chr_rom)?),
        1 => Box::new(Mapper1::new(&header, prg_rom, chr_rom)?),
        2 => Box::new(Mapper2::new(&header, prg_rom, chr_rom)?),
        3 => Box::new(Mapper3::new(&header, prg_rom, chr_rom)?),
        4 => Box::new(Mapper4::new(&header, prg_rom, chr_rom)?),
        other => return Err(CartridgeError::UnsupportedMapper(other)),
    };

    // Combined PRG-RAM + PRG-NVRAM region; default to 8 KB for legacy iNES 1.0.
    let prg_ram_total = header.prg_ram_size + header.prg_nvram_size;
    let prg_ram_size = if header.format == RomFormat::INes1 && prg_ram_total == 0 {
        DEFAULT_PRG_RAM_SIZE
    } else {
        prg_ram_total
    };

    Ok(Cartridge { mapper, prg_ram_size })
}

fn parse_ines1_header(h: &[u8]) -> RomHeader {
    let byte4 = h[4] as usize;
    let byte5 = h[5] as usize;
    let flags6 = h[6];
    let flags7 = h[7];

    let mapper = u16::from((flags7 & 0xF0) | (flags6 >> 4));
    let prg_rom_size = byte4 * PRG_BANK_SIZE;
    let chr_rom_size = byte5 * CHR_BANK_SIZE;
    let chr_ram_size = if chr_rom_size == 0 { DEFAULT_CHR_RAM_SIZE } else { 0 };

    RomHeader {
        format: RomFormat::INes1,
        mapper,
        submapper: 0,
        prg_rom_size,
        chr_rom_size,
        prg_ram_size: DEFAULT_PRG_RAM_SIZE,
        prg_nvram_size: 0,
        chr_ram_size,
        chr_nvram_size: 0,
        mirroring: parse_mirroring(flags6),
        has_battery: flags6 & 0x02 != 0,
        has_trainer: flags6 & 0x04 != 0,
        timing: CpuTiming::Ntsc,
    }
}

fn parse_nes2_header(h: &[u8]) -> Result<RomHeader, CartridgeError> {
    let byte4 = h[4];
    let byte5 = h[5];
    let flags6 = h[6];
    let flags7 = h[7];
    let byte8 = h[8];
    let byte9 = h[9];
    let byte10 = h[10];
    let byte11 = h[11];
    let byte12 = h[12];

    let mapper = (((byte8 & 0x0F) as u16) << 8)
        | ((flags7 & 0xF0) as u16)
        | ((flags6 >> 4) as u16);
    let submapper = byte8 >> 4;

    let prg_rom_size = decode_rom_size(byte4, byte9 & 0x0F, PRG_BANK_SIZE, SizeKind::Prg)?;
    let chr_rom_size = decode_rom_size(byte5, (byte9 & 0xF0) >> 4, CHR_BANK_SIZE, SizeKind::Chr)?;

    let prg_ram_size = ram_size(byte10 & 0x0F);
    let prg_nvram_size = ram_size((byte10 & 0xF0) >> 4);
    let chr_ram_size = ram_size(byte11 & 0x0F);
    let chr_nvram_size = ram_size((byte11 & 0xF0) >> 4);

    let timing = match byte12 & 0x03 {
        0 => CpuTiming::Ntsc,
        1 => CpuTiming::Pal,
        2 => CpuTiming::Multi,
        _ => CpuTiming::Dendy,
    };

    Ok(RomHeader {
        format: RomFormat::Nes2,
        mapper,
        submapper,
        prg_rom_size,
        chr_rom_size,
        prg_ram_size,
        prg_nvram_size,
        chr_ram_size,
        chr_nvram_size,
        mirroring: parse_mirroring(flags6),
        has_battery: flags6 & 0x02 != 0,
        has_trainer: flags6 & 0x04 != 0,
        timing,
    })
}

fn parse_mirroring(flags6: u8) -> Mirroring {
    if flags6 & 0x08 != 0 {
        Mirroring::FourScreen
    } else if flags6 & 0x01 != 0 {
        Mirroring::Vertical
    } else {
        Mirroring::Horizontal
    }
}

/// NES 2.0 §3 — linear or exponent-multiplier ROM size decoding.
fn decode_rom_size(
    lsb: u8,
    msb_nibble: u8,
    bank_size: usize,
    kind: SizeKind,
) -> Result<usize, CartridgeError> {
    if msb_nibble == 0x0F {
        // Exponent-multiplier: size = (1 << exponent) * (multiplier * 2 + 1)
        let exponent = ((lsb >> 2) & 0x3F) as u32;
        let multiplier = (lsb & 0x03) as usize;
        if exponent >= usize::BITS {
            return Err(CartridgeError::RomTooLarge { kind, bytes: usize::MAX });
        }
        let base = 1usize << exponent;
        let bytes = base.checked_mul(multiplier * 2 + 1)
            .ok_or(CartridgeError::RomTooLarge { kind, bytes: usize::MAX })?;
        if bytes > MAX_ROM_BYTES {
            return Err(CartridgeError::RomTooLarge { kind, bytes });
        }
        Ok(bytes)
    } else {
        let units = ((msb_nibble as usize) << 8) | (lsb as usize);
        let bytes = units.checked_mul(bank_size)
            .ok_or(CartridgeError::RomTooLarge { kind, bytes: usize::MAX })?;
        if bytes > MAX_ROM_BYTES {
            return Err(CartridgeError::RomTooLarge { kind, bytes });
        }
        Ok(bytes)
    }
}

/// NES 2.0 RAM/NVRAM size from a 4-bit shift count: 0 ⇒ 0; otherwise 64 << shift.
fn ram_size(shift: u8) -> usize {
    if shift == 0 { 0 } else { 64usize << shift }
}
