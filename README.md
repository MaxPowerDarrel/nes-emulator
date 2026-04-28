# nes-emulator

A cycle-accurate NES (Nintendo Entertainment System) emulator written in Rust, built as a hardware learning exercise. Correctness and faithful adherence to the original hardware specifications take priority over raw performance or shipping speed.

## Goals

- Cycle-accurate emulation of the MOS 6502 CPU, the PPU, and the APU sharing a single master clock
- Pure-Rust implementation with no external game-logic dependencies
- Spec-driven development — every behavior is grounded in the NESDev wiki, datasheets, or other reference documents
- Educational clarity: understanding *why* the hardware behaves as it does is a primary goal

## Status

| Milestone | Description | Status |
|-----------|-------------|--------|
| 1 | CPU — all official 6502 opcodes, passes `nestest.nes` | ✅ |
| 2 | Cartridge — iNES header parser, Mapper 0 (NROM) | ✅ |
| 3 | PPU — static background frame rendering | ✅ |
| 4 | PPU — full scanline rendering, NMI, sprite 0 hit | ✅ |
| 5 | Input — standard controller (`$4016`/`$4017`) | ✅ |
| 6 | First playable game — Super Mario Bros. 1 | ✅ |
| 7 | Mappers 1–4 (MMC1, UxROM, CNROM, MMC3) | 🚧 |
| 8 | APU — audio synthesis (mono via `cpal`) | ✅ |

## Tech Stack

- **Language**: Rust (stable, edition 2024)
- **Windowing**: [`winit`](https://crates.io/crates/winit) 0.28
- **Rendering**: [`pixels`](https://crates.io/crates/pixels) 0.13 (pure-Rust framebuffer, native 256×240 NES resolution)
- **Audio**: [`cpal`](https://crates.io/crates/cpal) 0.15 + [`ringbuf`](https://crates.io/crates/ringbuf) 0.3

## Architecture

```
src/
  main.rs        # Window creation, event loop, master clock
  bus.rs         # Memory bus — routes reads/writes to CPU RAM, PPU, APU, cartridge
  cpu/           # MOS 6502 CPU
    mod.rs
    opcodes.rs
    addressing.rs
  ppu/           # Picture Processing Unit
    mod.rs
  apu/           # Audio Processing Unit
    mod.rs
  cartridge/     # iNES header parser + mapper dispatch
    mod.rs
    mapper0.rs
```

The `Bus` owns the `Cartridge`, `Ppu`, `Apu`, and CPU RAM. The `Cpu` borrows the `Bus`. The main loop owns the `Cpu` and drives the master clock.

### Master clock interleaving

- PPU ticks every master cycle
- CPU ticks every 3 master cycles
- APU ticks every 2 master cycles

The emulator loop reflects this interleaving directly rather than running each component independently per-frame.

## Build & Run

```bash
# Type-check
cargo check

# Debug build
cargo build

# Release build (recommended for actual play)
cargo build --release

# Run with a ROM
cargo run --release -- path/to/rom.nes
```

## Controls

Standard NES controller (Player 1):

| NES Button | Keyboard |
|------------|----------|
| D-Pad      | Arrow keys |
| A          | Z |
| B          | X |
| Start      | Enter |
| Select     | Right Shift |

## Testing

Test ROMs are first-class citizens in this project. The CPU passes `nestest.nes` in log-comparison mode. Blargg's test ROMs are used to verify PPU and timing behavior:

- `nestest.nes` — gold standard 6502 CPU test
- `cpu_timing_test`
- `ppu_vbl_nmi`
- `sprite_0_hit`

## Mapper Support

| # | Name | Example games | Status |
|---|------|---------------|--------|
| 0 | NROM | Donkey Kong, Super Mario Bros. 1 | ✅ |
| 1 | MMC1 | The Legend of Zelda, Metroid | 🚧 |
| 2 | UxROM | Mega Man, Castlevania | 🚧 |
| 3 | CNROM | — | 🚧 |
| 4 | MMC3 | — | 🚧 |

## Project Philosophy

**No spec, no code.** Before any CPU opcode, PPU behavior, mapper logic, or bus behavior is implemented, the relevant hardware specification must be located and cited. When specs conflict or are ambiguous, the ambiguity and the chosen resolution are documented before implementation. See [`CLAUDE.md`](./CLAUDE.md) and [`docs/`](./docs) for milestone specs and design notes.

## References

- [NESDev Wiki](https://www.nesdev.org/wiki/Nesdev_Wiki)
- [6502.org](http://6502.org/) — MOS 6502 reference material
- Blargg's NES test ROMs

## License

This project is for personal and educational use.
