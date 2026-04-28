pub mod dmc;
pub mod envelope;
pub mod frame_counter;
pub mod length;
pub mod mixer;
pub mod noise;
pub mod pulse;
pub mod triangle;

use dmc::Dmc;
use frame_counter::FrameCounter;
use mixer::Mixer;
use noise::Noise;
use pulse::Pulse;
use triangle::Triangle;

pub struct Apu {
    pub pulse1: Pulse,
    pub pulse2: Pulse,
    pub triangle: Triangle,
    pub noise: Noise,
    pub dmc: Dmc,
    pub frame_counter: FrameCounter,
    pub mixer: Mixer,
    pub cycles: u64,
    pub sample_rate: f32,
    pub sample_accumulator: f32,
    pub output_accumulator: f32,
    pub output_samples: f32,
    // NES audio filter chain — see https://www.nesdev.org/wiki/APU_Mixer#Emulation
    // Two first-order high-pass filters (90 Hz, 440 Hz) and one first-order low-pass (14 kHz).
    pub hp90_prev_in: f32,
    pub hp90_prev_out: f32,
    pub hp440_prev_in: f32,
    pub hp440_prev_out: f32,
    pub lp14k_prev_out: f32,
    pub filters_initialized: bool,
    pub output_buffer: Option<crate::audio::AudioProducer>,
}

impl Apu {
    pub fn new() -> Self {
        Self {
            pulse1: Pulse::new(false),
            pulse2: Pulse::new(true),
            triangle: Triangle::new(),
            noise: Noise::new(),
            dmc: Dmc::new(),
            frame_counter: FrameCounter::new(),
            mixer: Mixer::new(),
            cycles: 0,
            sample_rate: 44100.0,
            sample_accumulator: 0.0,
            output_accumulator: 0.0,
            output_samples: 0.0,
            hp90_prev_in: 0.0,
            hp90_prev_out: 0.0,
            hp440_prev_in: 0.0,
            hp440_prev_out: 0.0,
            lp14k_prev_out: 0.0,
            filters_initialized: false,
            output_buffer: None,
        }
    }

    pub fn tick(&mut self) {
        // APU clocked once every 2 CPU cycles.
        // The master clock loop in main.rs calls this every 6 master cycles.

        self.pulse1.tick_timer();
        self.pulse2.tick_timer();
        self.noise.tick_timer();
        self.dmc.tick_timer();

        let (quarter, half, _irq) = self.frame_counter.tick();
        if quarter {
            self.pulse1.envelope.tick();
            self.pulse2.envelope.tick();
            self.noise.envelope.tick();
            self.triangle.tick_linear();
        }
        if half {
            self.pulse1.length.tick();
            self.pulse2.length.tick();
            self.triangle.length.tick();
            self.noise.length.tick();
            self.pulse1.tick_sweep();
            self.pulse2.tick_sweep();
        }

        self.cycles += 1;

        // Downsampling: integrate output() every APU tick, then on each
        // output-sample boundary divide by the exact fractional number of
        // ticks that elapsed. This is a true box-filter average regardless of
        // whether 20 or 21 ticks fell into a given output sample, eliminating
        // the amplitude jitter caused by integer-only counting.
        // apu_rate / sample_rate ≈ 20.29 ticks per output sample at 44.1 kHz.
        let apu_rate = 1789773.0 / 2.0;
        let sample_step = apu_rate / self.sample_rate;

        self.sample_accumulator += 1.0;
        self.output_accumulator += self.output();
        self.output_samples += 1.0;

        if self.sample_accumulator >= sample_step {
            self.sample_accumulator -= sample_step;
            let s = self.output_accumulator / self.output_samples;
            self.output_accumulator = 0.0;
            self.output_samples = 0.0;

            // NES analog output filter chain.
            // Spec: https://www.nesdev.org/wiki/APU_Mixer#Emulation
            //   HPF1: ~90 Hz   (removes DC + sub-bass)
            //   HPF2: ~440 Hz  (mimics output coupling caps / amp)
            //   LPF : ~14 kHz  (removes high-freq aliasing & hiss)
            // First-order RC filter coefficients computed from the host sample rate:
            //   For HPF: y[n] = a*(y[n-1] + x[n] - x[n-1]),   a = RC/(RC + dt)
            //   For LPF: y[n] = y[n-1] + a*(x[n] - y[n-1]),   a = dt/(RC + dt)
            let dt = 1.0 / self.sample_rate;
            let rc_hp90 = 1.0 / (2.0 * std::f32::consts::PI * 90.0);
            let rc_hp440 = 1.0 / (2.0 * std::f32::consts::PI * 440.0);
            let rc_lp14k = 1.0 / (2.0 * std::f32::consts::PI * 14000.0);
            let a_hp90 = rc_hp90 / (rc_hp90 + dt);
            let a_hp440 = rc_hp440 / (rc_hp440 + dt);
            let a_lp14k = dt / (rc_lp14k + dt);

            // Seed filter history with the first sample to avoid a startup transient pop.
            if !self.filters_initialized {
                self.hp90_prev_in = s;
                self.hp90_prev_out = 0.0;
                self.hp440_prev_in = s;
                self.hp440_prev_out = 0.0;
                self.lp14k_prev_out = s;
                self.filters_initialized = true;
            }

            let x = s;
            let y1 = a_hp90 * (self.hp90_prev_out + x - self.hp90_prev_in);
            self.hp90_prev_in = x;
            self.hp90_prev_out = y1;

            let y2 = a_hp440 * (self.hp440_prev_out + y1 - self.hp440_prev_in);
            self.hp440_prev_in = y1;
            self.hp440_prev_out = y2;

            let y3 = self.lp14k_prev_out + a_lp14k * (y2 - self.lp14k_prev_out);
            self.lp14k_prev_out = y3;

            let out = y3.clamp(-1.0, 1.0);

            if let Some(ref mut prod) = self.output_buffer {
                // Non-blocking push. Pacing is now handled by the wall-clock 60 Hz
                // frame timer in main.rs, so the emulator produces samples at the
                // correct average rate without coupling the master-clock loop to
                // the audio callback. On rare overflow we drop a sample — far less
                // audible than the burst/stall pattern that producer-side blocking
                // (park_timeout / sleep) was creating from coarse OS scheduler wakes.
                let _ = prod.push(out);
            }
        }
    }

    pub fn triangle_tick(&mut self) {
        // Triangle timer is clocked at the CPU rate.
        self.triangle.tick_timer();
    }

    pub fn read_status(&mut self) -> u8 {
        let mut res = 0;
        if self.pulse1.length.value > 0 {
            res |= 0x01;
        }
        if self.pulse2.length.value > 0 {
            res |= 0x02;
        }
        if self.triangle.length.value > 0 {
            res |= 0x04;
        }
        if self.noise.length.value > 0 {
            res |= 0x08;
        }
        if self.dmc.bytes_remaining > 0 {
            res |= 0x10;
        }
        if self.frame_counter.irq_flag {
            res |= 0x40;
        }
        if self.dmc.irq_flag {
            res |= 0x80;
        }

        self.frame_counter.irq_flag = false;
        res
    }

    pub fn write_status(&mut self, val: u8) {
        self.pulse1.enabled = (val & 0x01) != 0;
        self.pulse1.length.set_enabled(self.pulse1.enabled);

        self.pulse2.enabled = (val & 0x02) != 0;
        self.pulse2.length.set_enabled(self.pulse2.enabled);

        self.triangle.enabled = (val & 0x04) != 0;
        self.triangle.length.set_enabled(self.triangle.enabled);

        self.noise.enabled = (val & 0x08) != 0;
        self.noise.length.set_enabled(self.noise.enabled);

        self.dmc.set_enabled((val & 0x10) != 0);
    }

    pub fn write_register(&mut self, addr: u16, val: u8) {
        match addr {
            // Pulse 1
            0x4000 => {
                self.pulse1.duty_idx = (val >> 6) & 3;
                self.pulse1.length.halt = (val & 0x20) != 0;
                self.pulse1.envelope.loop_flag = (val & 0x20) != 0;
                self.pulse1.envelope.constant_flag = (val & 0x10) != 0;
                self.pulse1.envelope.divider_period = val & 0x0F;
            }
            0x4001 => {
                self.pulse1.sweep_enabled = (val & 0x80) != 0;
                self.pulse1.sweep_period = (val >> 4) & 7;
                self.pulse1.sweep_negate = (val & 0x08) != 0;
                self.pulse1.sweep_shift = val & 0x07;
                self.pulse1.sweep_reload = true;
            }
            0x4002 => {
                self.pulse1.timer_period = (self.pulse1.timer_period & 0x0700) | (val as u16);
            }
            0x4003 => {
                self.pulse1.timer_period =
                    (self.pulse1.timer_period & 0x00FF) | (((val & 0x07) as u16) << 8);
                self.pulse1.length.load(val);
                self.pulse1.seq_idx = 0;
                self.pulse1.envelope.start = true;
            }
            // Pulse 2
            0x4004 => {
                self.pulse2.duty_idx = (val >> 6) & 3;
                self.pulse2.length.halt = (val & 0x20) != 0;
                self.pulse2.envelope.loop_flag = (val & 0x20) != 0;
                self.pulse2.envelope.constant_flag = (val & 0x10) != 0;
                self.pulse2.envelope.divider_period = val & 0x0F;
            }
            0x4005 => {
                self.pulse2.sweep_enabled = (val & 0x80) != 0;
                self.pulse2.sweep_period = (val >> 4) & 7;
                self.pulse2.sweep_negate = (val & 0x08) != 0;
                self.pulse2.sweep_shift = val & 0x07;
                self.pulse2.sweep_reload = true;
            }
            0x4006 => {
                self.pulse2.timer_period = (self.pulse2.timer_period & 0x0700) | (val as u16);
            }
            0x4007 => {
                self.pulse2.timer_period =
                    (self.pulse2.timer_period & 0x00FF) | (((val & 0x07) as u16) << 8);
                self.pulse2.length.load(val);
                self.pulse2.seq_idx = 0;
                self.pulse2.envelope.start = true;
            }
            // Triangle
            0x4008 => {
                self.triangle.linear_control = (val & 0x80) != 0;
                self.triangle.length.halt = (val & 0x80) != 0;
                self.triangle.linear_reload_value = val & 0x7F;
            }
            0x400A => {
                self.triangle.timer_period = (self.triangle.timer_period & 0x0700) | (val as u16);
            }
            0x400B => {
                self.triangle.timer_period =
                    (self.triangle.timer_period & 0x00FF) | (((val & 0x07) as u16) << 8);
                self.triangle.length.load(val);
                self.triangle.linear_reload_flag = true;
            }
            // Noise
            0x400C => {
                self.noise.length.halt = (val & 0x20) != 0;
                self.noise.envelope.loop_flag = (val & 0x20) != 0;
                self.noise.envelope.constant_flag = (val & 0x10) != 0;
                self.noise.envelope.divider_period = val & 0x0F;
            }
            0x400E => {
                self.noise.mode = (val & 0x80) != 0;
                self.noise.set_period(val & 0x0F);
            }
            0x400F => {
                self.noise.length.load(val);
                self.noise.envelope.start = true;
            }
            // DMC
            0x4010 => {
                self.dmc.irq_enabled = (val & 0x80) != 0;
                self.dmc.loop_flag = (val & 0x40) != 0;
                self.dmc.set_rate(val & 0x0F);
                if !self.dmc.irq_enabled {
                    self.dmc.irq_flag = false;
                }
            }
            0x4011 => {
                self.dmc.output_level = val & 0x7F;
            }
            0x4012 => {
                self.dmc.sample_address = 0xC000 | ((val as u16) << 6);
            }
            0x4013 => {
                self.dmc.sample_length = ((val as u16) << 4) | 1;
            }
            0x4015 => self.write_status(val),
            0x4017 => self.frame_counter.write(val),
            _ => {}
        }
    }

    pub fn output(&self) -> f32 {
        self.mixer.mix(
            self.pulse1.output(),
            self.pulse2.output(),
            self.triangle.output(),
            self.noise.output(),
            self.dmc.output(),
        )
    }

    pub fn irq(&self) -> bool {
        self.frame_counter.irq_flag || self.dmc.irq_flag
    }
}
