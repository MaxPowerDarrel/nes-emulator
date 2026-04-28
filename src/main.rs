mod bus;
mod cartridge;
mod cpu;
mod ppu;

use std::env;
use std::error::Error;
use std::fs;

use bus::Bus;
use cpu::Cpu;
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        return Err("Usage: nes-emulator <rom.nes> [--nestest]".into());
    }

    let rom_path = &args[1];
    let nestest_mode = args.iter().any(|a| a == "--nestest");

    let rom_data = fs::read(rom_path)
        .map_err(|e| format!("Failed to read ROM '{}': {}", rom_path, e))?;

    let mapper = cartridge::from_bytes(&rom_data)
        .map_err(|e| format!("Cartridge error: {}", e))?;

    let mut bus = Bus::new(mapper);
    let mut cpu = Cpu::new();

    if nestest_mode {
        // Automation mode: force PC to $C000, bypass reset vector.
        // Spec: https://www.nesdev.org/wiki/Emulator_tests#nestest.log
        cpu.pc = 0xC000;
        cpu.p = 0x24;
        cpu.sp = 0xFD;
        cpu.cycles = 7;
        run_nestest(&mut cpu, &mut bus);
        return Ok(());
    }

    cpu.reset(&mut bus);
    run_windowed(cpu, bus)
}

fn run_windowed(mut cpu: Cpu, mut bus: Bus) -> Result<(), Box<dyn Error>> {
    // winit 0.28 + pixels 0.13 — both use raw-window-handle 0.5.
    // Spec: https://www.nesdev.org/wiki/PPU_rendering — master clock loop

    let event_loop = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title("NES")
        .with_inner_size(LogicalSize::new(256u32 * 3, 240u32 * 3))
        .build(&event_loop)?;

    let mut pixels = {
        let window_size = window.inner_size();
        let surface_texture =
            SurfaceTexture::new(window_size.width, window_size.height, &window);
        Pixels::new(256, 240, surface_texture)?
    };

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Poll;

        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = ControlFlow::Exit;
            }

            Event::WindowEvent {
                event: WindowEvent::Resized(size),
                ..
            } => {
                if pixels.resize_surface(size.width, size.height).is_err() {
                    *control_flow = ControlFlow::Exit;
                }
            }

            Event::RedrawRequested(_) => {
                // Run master clock until the PPU signals one full frame.
                // cpu.step() returns CPU cycles consumed; PPU ticks 3× per cycle.
                while !bus.ppu.frame_complete {
                    let cpu_cycles = cpu.step(&mut bus);
                    for _ in 0..cpu_cycles {
                        {
                            // Split-field borrow: bus.ppu (mut) vs bus.nametable_vram + bus.mapper (shared).
                            let nt = &bus.nametable_vram;
                            let mapper = bus.mapper.as_ref();
                            bus.ppu.tick(nt, mapper);
                        }
                        {
                            let nt = &bus.nametable_vram;
                            let mapper = bus.mapper.as_ref();
                            bus.ppu.tick(nt, mapper);
                        }
                        {
                            let nt = &bus.nametable_vram;
                            let mapper = bus.mapper.as_ref();
                            bus.ppu.tick(nt, mapper);
                        }
                    }
                }
                bus.ppu.frame_complete = false;

                pixels.frame_mut().copy_from_slice(&bus.ppu.framebuffer);
                if pixels.render().is_err() {
                    *control_flow = ControlFlow::Exit;
                }
            }

            Event::MainEventsCleared => {
                window.request_redraw();
            }

            _ => {}
        }
    });
}

fn run_nestest(cpu: &mut Cpu, bus: &mut Bus) {
    // nestest.nes automation mode:
    //   Results are written to zero-page $02 (official) and $03 (unofficial).
    //   $00 = all tests passed; non-zero = failure code.
    //   The ROM halts at $0001 after completion.
    //
    // Ref: https://www.nesdev.org/wiki/Emulator_tests#nestest.nes

    const MAX_CYCLES: u64 = 100_000_000;

    loop {
        let ppu_cycles = cpu.cycles * 3;
        let ppu_dot = ppu_cycles % 341;
        let ppu_scanline = ppu_cycles / 341;
        let log_line = format!(
            "{}A:{:02X} X:{:02X} Y:{:02X} P:{:02X} SP:{:02X} PPU:{:>3},{:>3} CYC:{}",
            cpu.disassemble(bus, cpu.pc),
            cpu.a,
            cpu.x,
            cpu.y,
            cpu.p,
            cpu.sp,
            ppu_scanline,
            ppu_dot,
            cpu.cycles,
        );
        println!("{}", log_line);

        cpu.step(bus);

        if cpu.pc == 0x0001 {
            let official = bus.read(0x0002);
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