# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## Build & Run

```bash
# Check
cargo check

# Build
cargo build

# Run
cargo run

# Run with a ROM
cargo run -- path/to/rom.nes
```

## Project Constitution

### Goal
Build a cycle-accurate NES emulator in Rust as a hardware learning exercise. Correctness and understanding of the hardware take priority over shipping speed.

### Development Philosophy: Spec Driven Development
**No spec, no code.** Every component or behavior implemented must be grounded in a hardware specification before any code is written.

- Before implementing any CPU opcode, PPU behavior, mapper logic, or bus behavior, locate and cite the relevant spec (NESDev wiki, datasheet, or reference document)
- Implementation decisions must be traceable to a spec — if there is no spec citation, do not write the code
- When specs conflict or are ambiguous, document the ambiguity and the resolution chosen before implementing
- This is a learning exercise: understanding *why* the hardware behaves as it does matters as much as getting it to work

### Tech Stack
- **Language**: Rust (stable, edition 2024)
- **Rendering**: `winit` 0.28 + `pixels` 0.13 (pure-Rust framebuffer, 256×240 NES native resolution)
- **Audio**: `cpal` 0.15 + `ringbuf` 0.3 — mono output (APU milestone 8 complete)
- **No external game-logic dependencies** — implement everything from the NES hardware spec

### Accuracy Target
**Cycle-accurate.** The CPU (6502), PPU, and APU share a single master clock:
- PPU ticks every master cycle
- CPU ticks every 3 master cycles
- APU ticks every 6 master cycles (= every 2 CPU cycles); the triangle channel ticks every 3 master cycles (every CPU cycle)

The emulator loop must reflect this interleaving directly, not run components independently per-frame.

### Architecture

```
src/
  main.rs        # Window creation, event loop, master clock
  bus.rs         # Memory bus — routes reads/writes to CPU RAM, PPU, APU, cartridge
  audio.rs       # cpal output stream + ring-buffer plumbing
  input.rs       # Standard controller ($4016/$4017)
  cpu/           # MOS 6502 CPU
    mod.rs
    opcodes.rs
    addressing.rs
  ppu/           # Picture Processing Unit
    mod.rs
    palette.rs
  apu/           # Audio Processing Unit
    mod.rs
    pulse.rs / triangle.rs / noise.rs / dmc.rs
    envelope.rs / length.rs / frame_counter.rs / mixer.rs
  cartridge/     # iNES / NES 2.0 header parser + mapper dispatch
    mod.rs
    mapper0.rs / mapper1.rs / mapper2.rs / mapper3.rs / mapper4.rs
```

**Bus ownership**: The `Bus` owns the `Cartridge`, `Ppu`, `Apu`, and CPU RAM. The `Cpu` borrows the `Bus`. The main loop owns the `Cpu`.

**No shared mutable state via `Rc<RefCell<_>>`** unless absolutely necessary — prefer passing references explicitly or restructuring to avoid it.

### Mapper Scope
Mappers are implemented incrementally. Current status:
- **Mapper 0 (NROM)** ✅ — Donkey Kong, Super Mario Bros 1
- **Mapper 1 (MMC1)** 🚧 — Zelda, Metroid
- **Mapper 2 (UxROM)** 🚧 — Mega Man, Castlevania
- **Mapper 3 (CNROM)** 🚧
- **Mapper 4 (MMC3)** 🚧

### Testing Strategy
Test ROMs are first-class. The CPU must pass `nestest.nes` before PPU work begins. Run Blargg's test ROMs to verify PPU and timing behavior.

- `nestest.nes` — gold standard 6502 CPU test (log-comparison mode)
- Blargg's `cpu_timing_test`, `ppu_vbl_nmi`, `sprite_0_hit` ROMs

### Milestones
1. **CPU** — implement all official 6502 opcodes, pass nestest.nes
2. **Cartridge** — iNES header parser, mapper 0
3. **PPU** — render a static background frame
4. **PPU** — full scanline rendering, NMI, sprite 0 hit
5. **Input** — standard controller ($4016/$4017 polling)
6. **First playable game** — Super Mario Bros 1 boots and is playable
7. **Mappers 1–4** — expand game compatibility
8. **APU** — audio synthesis (completed)

### Version Control
- **New feature = new branch** — never commit feature work directly to `main`
- Branch naming: `feature/<short-description>` (e.g. `feature/cpu-opcodes`, `feature/mapper0`)
- Keep commits focused — one logical change per commit
- **Do not open a PR until the user has verified the feature works**
- Once the user gives the go-ahead, create a PR targeting `main` with a clear description of what was implemented and any spec references used

### Coding Conventions
- No `unwrap()` / `expect()` in emulator core paths — use `?`, `unwrap_or`, or explicit error handling
- Represent hardware registers as `u8`; addresses as `u16`
- Prefer named constants over magic numbers for memory map regions
- Each hardware component (`Cpu`, `Ppu`, `Apu`, `Cartridge`) lives in its own module
- Keep `main.rs` thin — only windowing, input plumbing, and the master clock loop
- File-level documentation uses inner doc comments (`//!`), not `///`. Item-level docs use `///`
- Prefer `u.is_multiple_of(n)` over `u % n == 0` for unsigned divisibility checks
- Cite the relevant NESDev / datasheet URL in a doc comment next to any hardware-modeling code
- Run `cargo clippy` before committing — the tree is expected to be warning-free
