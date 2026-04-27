mod bus;
mod cartridge;
mod cpu;

use std::env;
use std::fs;

use bus::Bus;
use cpu::Cpu;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: nes-emulator <rom.nes> [--nestest]");
        std::process::exit(1);
    }

    let rom_path = &args[1];
    let nestest_mode = args.iter().any(|a| a == "--nestest");

    let rom_data = match fs::read(rom_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to read ROM '{}': {}", rom_path, e);
            std::process::exit(1);
        }
    };

    let mapper = match cartridge::from_bytes(&rom_data) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Cartridge error: {}", e);
            std::process::exit(1);
        }
    };

    let mut bus = Bus::new(mapper);
    let mut cpu = Cpu::new();

    if nestest_mode {
        // Automation mode: force PC to $C000, bypass reset vector.
        // Spec: https://www.nesdev.org/wiki/Emulator_tests#nestest.log
        cpu.pc = 0xC000;
        cpu.p = 0x24;
        cpu.sp = 0xFD;
        cpu.cycles = 7;
    } else {
        cpu.reset(&bus);
    }

    if nestest_mode {
        run_nestest(&mut cpu, &mut bus);
    } else {
        run_forever(&mut cpu, &mut bus);
    }
}

fn run_nestest(cpu: &mut Cpu, bus: &mut Bus) {
    // nestest.nes automation mode:
    //   Results are written to zero-page $02 (official) and $03 (unofficial).
    //   $00 = all tests in that group passed; non-zero = failure code.
    //   The ROM reaches the halt loop at $0001 after all tests complete (pass or fail).
    //
    // Note: $6000 is NOT used by nestest.nes itself. The $6000/$6001 mechanism
    // applies to other Blargg test ROMs (cpu_timing_test, ppu_vbl_nmi, etc.).
    //
    // Ref: https://www.nesdev.org/wiki/Emulator_tests#nestest.nes

    const MAX_CYCLES: u64 = 100_000_000;

    loop {
        // Log *before* executing the instruction (nestest log format).
        // PPU dot and scanline derived from CPU cycle counter.
        // Each CPU cycle = 3 PPU cycles; one scanline = 341 PPU cycles.
        // Spec: docs/spec-milestone-1-cpu.md §8
        let ppu_cycles = cpu.cycles * 3;
        let ppu_dot      = ppu_cycles % 341;
        let ppu_scanline = ppu_cycles / 341;
        let log_line = format!(
            "{}A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} PPU:{:>3},{:>3} CYC:{}",
            cpu.disassemble(bus, cpu.pc),
            cpu.a, cpu.x, cpu.y, cpu.p, cpu.sp,
            ppu_scanline, ppu_dot,
            cpu.cycles,
        );
        println!("{}", log_line);

        cpu.step(bus);

        // The ROM halts by entering an infinite BRK loop starting at $0001.
        // Detect this and check the result codes.
        if cpu.pc == 0x0001 {
            let official   = bus.read(0x0002);
            let unofficial = bus.read(0x0003);
            if official == 0x00 && unofficial == 0x00 {
                eprintln!("\nnestest PASSED ($02=$00, $03=$00)");
                std::process::exit(0);
            } else {
                eprintln!(
                    "\nnestest FAILED: official=${:02X}, unofficial=${:02X}",
                    official, unofficial
                );
                std::process::exit(1);
            }
        }

        if cpu.cycles > MAX_CYCLES {
            eprintln!("nestest: exceeded cycle limit without finishing");
            std::process::exit(1);
        }
    }
}

fn run_forever(cpu: &mut Cpu, bus: &mut Bus) {
    // Simple headless loop for now — PPU/window comes in later milestones.
    loop {
        cpu.step(bus);
    }
}