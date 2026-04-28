mod bus;
mod cartridge;
mod cpu;
mod input;
mod ppu;

use std::env;
use std::error::Error;
use std::fs;

use bus::Bus;
use cpu::Cpu;
use pixels::{Pixels, SurfaceTexture};
use winit::dpi::LogicalSize;
use std::collections::HashSet;
use winit::event::{ElementState, Event, VirtualKeyCode, WindowEvent};
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

    let cart = cartridge::from_bytes(&rom_data)
        .map_err(|e| format!("Cartridge error: {}", e))?;

    let mut bus = Bus::new(cart);
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

    // Track held keys so two keys bound to the same button work independently.
    // Spec: https://www.nesdev.org/wiki/Standard_controller — key mapping §4
    let mut held_keys: HashSet<VirtualKeyCode> = HashSet::new();

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
                event:
                    WindowEvent::KeyboardInput {
                        input:
                            winit::event::KeyboardInput {
                                virtual_keycode: Some(key),
                                state,
                                ..
                            },
                        ..
                    },
                ..
            } => {
                let pressed = state == ElementState::Pressed;
                if pressed {
                    held_keys.insert(key);
                } else {
                    held_keys.remove(&key);
                }

                // Recompute controller 1 button byte from all held keys.
                // Spec: https://www.nesdev.org/wiki/Standard_controller — key mapping §4
                let mut buttons = 0u8;
                for k in &held_keys {
                    let mask = match k {
                        VirtualKeyCode::Z | VirtualKeyCode::K => input::BTN_A,
                        VirtualKeyCode::X | VirtualKeyCode::J => input::BTN_B,
                        VirtualKeyCode::RShift | VirtualKeyCode::U => input::BTN_SELECT,
                        VirtualKeyCode::Return | VirtualKeyCode::I => input::BTN_START,
                        VirtualKeyCode::Up | VirtualKeyCode::W => input::BTN_UP,
                        VirtualKeyCode::Down | VirtualKeyCode::S => input::BTN_DOWN,
                        VirtualKeyCode::Left | VirtualKeyCode::A => input::BTN_LEFT,
                        VirtualKeyCode::Right | VirtualKeyCode::D => input::BTN_RIGHT,
                        _ => 0,
                    };
                    buttons |= mask;
                }
                bus.controller1.buttons = buttons;
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
                // Master clock loop — spec milestone 4 §2.
                //
                // Per-iteration order:
                //   1. Service pending NMI (edge-triggered, set during last tick batch).
                //   2. Step CPU one instruction.
                //   3. Tick PPU `cycles * 3` times to maintain the hardware 3:1 ratio.
                //   4. Service OAM DMA if pending; tick PPU through the 513-cycle stall.
                //
                // Ticking PPU per CPU cycle (not per instruction) keeps game logic and
                // visual timing in sync — otherwise the CPU runs ~3× faster than the PPU
                // and animations appear sped up / janky.
                while !bus.ppu.frame_complete {
                    if bus.ppu.nmi_pending {
                        bus.ppu.nmi_pending = false;
                        cpu.nmi(&mut bus);
                    }

                    let cycles_before = cpu.cycles;
                    cpu.step(&mut bus);
                    let dt = (cpu.cycles - cycles_before) as u32;

                    for _ in 0..(dt * 3) {
                        let nt = &bus.nametable_vram;
                        let mapper = bus.mapper.as_ref();
                        bus.ppu.tick(nt, mapper);
                    }

                    // OAM DMA — spec §7. Hardware stalls CPU 513 cycles (514 on odd CPU
                    // cycle); the PPU keeps clocking through the stall.
                    if bus.oam_dma_pending {
                        bus.oam_dma_pending = false;
                        let page = (bus.oam_dma_page as u16) << 8;
                        let oam_addr = bus.ppu.oam_addr;
                        for i in 0u16..256 {
                            let val = bus.read(page | i);
                            let oam_idx = oam_addr.wrapping_add(i as u8) as usize;
                            bus.ppu.oam[oam_idx] = val;
                        }
                        cpu.cycles += 513;
                        for _ in 0..(513 * 3) {
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