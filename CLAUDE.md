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
- **Language**: Rust (stable)
- **Rendering**: `winit` + `pixels` (pure Rust framebuffer, 256×240 NES native resolution)
- **Audio**: Deferred — APU is out of scope until CPU and PPU are solid
- **No external game-logic dependencies** — implement everything from the NES hardware spec

### Accuracy Target
**Cycle-accurate.** The CPU (6502) and PPU share a single master clock:
- PPU ticks every master cycle
- CPU ticks every 3 master cycles
- APU ticks every 2 master cycles (when implemented)

The emulator loop must reflect this interleaving, not run CPU and PPU independently per-frame.

### Architecture

```
src/
  main.rs        # Window creation, event loop, master clock
  bus.rs         # Memory bus — routes reads/writes to CPU RAM, PPU, cartridge
  cpu/           # MOS 6502 CPU
    mod.rs
    opcodes.rs
    addressing.rs
  ppu/           # Picture Processing Unit
    mod.rs
  cartridge/     # iNES header parser + mapper dispatch
    mod.rs
    mapper0.rs
```

**Bus ownership**: The `Bus` owns the `Cartridge`, `Ppu`, and CPU RAM. The `Cpu` holds a reference to the `Bus`. The main loop owns the `Cpu`.

**No shared mutable state via `Rc<RefCell<_>>`** unless absolutely necessary — prefer passing references explicitly or restructuring to avoid it.

### Mapper Scope
Implement mappers incrementally:
- **Mapper 0 (NROM)**: First — covers Donkey Kong, Super Mario Bros 1
- **Mapper 1 (MMC1)**: Second — covers Zelda, Metroid
- **Mapper 2 (UxROM)**: Third — covers Mega Man, Castlevania
- **Mapper 3 (CNROM)**, **Mapper 4 (MMC3)**: Later

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
8. **APU** — audio synthesis (deferred)

### Version Control
- **New feature = new branch** — never commit feature work directly to `main`
- Branch naming: `feature/<short-description>` (e.g. `feature/cpu-opcodes`, `feature/mapper0`)
- Keep commits focused — one logical change per commit
- **Do not open a PR until the user has verified the feature works**
- Once the user gives the go-ahead, create a PR targeting `main` with a clear description of what was implemented and any spec references used

### Coding Conventions
- No `unwrap()` in emulator core paths — use `?` or explicit error handling
- Represent hardware registers as `u8`; addresses as `u16`
- Prefer named constants over magic numbers for memory map regions
- Each hardware component (`Cpu`, `Ppu`, `Cartridge`) lives in its own module
- Keep `main.rs` thin — only windowing and the master clock loop
