# Milestone 8 Spec: APU — Audio Synthesis

**Goal**: Implement the NES Audio Processing Unit (APU) — the five sound channels (Pulse 1, Pulse 2, Triangle, Noise, DMC), the frame counter, the mixer, and the CPU-visible registers at $4000–$4017. Sample the mixer output at the host's audio sample rate and play it through the host speakers. After this milestone, Super Mario Bros 1 plays its title music and in-game sound effects in time with the picture, and the existing CPU/PPU cycle accuracy is preserved.

**Primary references**:
- [NESDev APU](https://www.nesdev.org/wiki/APU) — top-level overview, register map, mixer formula
- [NESDev APU Pulse](https://www.nesdev.org/wiki/APU_Pulse) — duty cycles, sweep unit, length counter
- [NESDev APU Triangle](https://www.nesdev.org/wiki/APU_Triangle) — linear counter, 32-step sequencer
- [NESDev APU Noise](https://www.nesdev.org/wiki/APU_Noise) — 15-bit LFSR, period table
- [NESDev APU DMC](https://www.nesdev.org/wiki/APU_DMC) — delta modulation, sample-fetch DMA, IRQ
- [NESDev APU Frame Counter](https://www.nesdev.org/wiki/APU_Frame_Counter) — 4-step / 5-step modes, IRQ
- [NESDev APU Envelope](https://www.nesdev.org/wiki/APU_Envelope) — shared envelope generator
- [NESDev APU Length Counter](https://www.nesdev.org/wiki/APU_Length_Counter) — load table, halt flag
- [NESDev APU Sweep](https://www.nesdev.org/wiki/APU_Sweep) — pulse sweep unit
- [NESDev APU Mixer](https://www.nesdev.org/wiki/APU_Mixer) — non-linear mixing equation
- [NESDev CPU memory map](https://www.nesdev.org/wiki/CPU_memory_map) — $4000–$4017 routing

**Tech stack addition**:
- [`cpal`](https://docs.rs/cpal) — pure-Rust cross-platform audio output. Chosen because it requires no system game-audio middleware and is the de-facto Rust audio I/O crate paired with `winit`/`pixels`.

---

## 1. Hardware Background

The APU is integrated into the 2A03 CPU die. It is clocked from the same master clock as the CPU but advances at **half the CPU rate** — i.e. once every 2 CPU cycles, or once every 6 master cycles in the project's master-clock scheme (CPU = 3 master cycles, PPU = 1 master cycle, APU = 6 master cycles).

The APU exposes:

| Range          | Purpose |
|----------------|---------|
| $4000–$4003    | Pulse 1 |
| $4004–$4007    | Pulse 2 |
| $4008–$400B    | Triangle |
| $400C–$400F    | Noise |
| $4010–$4013    | DMC |
| $4015          | Status / channel enable (R/W) |
| $4017          | Frame counter (W) — note: same address as Controller 2 read |

$4017 is **write-only for the APU** and **read-only for Controller 2** (already wired in milestone 5). The bus must keep these dispatch paths separate.

### 1.1 Channel timing summary

| Channel  | Timer clock     | Sequencer               | Output range |
|----------|-----------------|-------------------------|--------------|
| Pulse 1/2 | CPU/2 (APU clock) | 8-step duty             | 0–15         |
| Triangle | CPU clock       | 32-step triangle wave   | 0–15         |
| Noise    | CPU/2           | 15-bit LFSR             | 0–15         |
| DMC      | CPU/2           | 1-bit delta, 7-bit DAC  | 0–127        |

Triangle is unique: its timer is clocked at the CPU rate, not half-rate.

---

## 2. New Module Layout

```
src/apu/
  mod.rs          # Apu struct, register dispatch, tick(), sample()
  pulse.rs        # Pulse channel (×2), duty sequencer, sweep
  triangle.rs     # Triangle channel, linear counter
  noise.rs        # Noise channel, LFSR
  dmc.rs          # DMC channel, sample-fetch DMA hook
  envelope.rs     # Shared envelope generator
  length.rs       # Length counter table + helpers
  frame_counter.rs # 4-step / 5-step sequencer, IRQ flag
  mixer.rs        # Non-linear mixer (lookup tables)
src/audio.rs      # cpal output stream + ring buffer
```

Each channel is its own struct so the spec for one component is testable in isolation.

---

## 3. Channel Specifications

### 3.1 Pulse (×2) — `src/apu/pulse.rs`

**Source**: https://www.nesdev.org/wiki/APU_Pulse

Registers (Pulse 1 at $4000–$4003; Pulse 2 at $4004–$4007 — identical layout):

```
$4000 / $4004:  DDLC VVVV
  DD    duty (0=12.5%, 1=25%, 2=50%, 3=25% negated)
  L     length counter halt (also envelope loop)
  C     constant volume flag
  VVVV  envelope period / constant volume

$4001 / $4005:  EPPP NSSS
  E    sweep enable
  PPP  sweep period
  N    negate flag
  SSS  shift count
  Writing this register sets the sweep reload flag.

$4002 / $4006:  TTTT TTTT  (timer low)
$4003 / $4007:  LLLL LTTT  (length load index | timer high)
  Writing $4003/$4007 also restarts the duty sequencer
  (sequence index → 0) and sets the envelope start flag.
```

**Duty waveforms** (1 = high, MSB output first, 8-step sequence):

| Duty | Sequence       |
|------|----------------|
| 0    | 0 1 0 0 0 0 0 0 |
| 1    | 0 1 1 0 0 0 0 0 |
| 2    | 0 1 1 1 1 0 0 0 |
| 3    | 1 0 0 1 1 1 1 1 |

**Sweep target period** = `current_period ± (current_period >> shift)` where Pulse 1 subtracts an extra 1 on negate (one's-complement) and Pulse 2 does not (two's-complement).

**Mute conditions** (output 0): timer period < 8, sweep target period > 0x7FF, length counter == 0, or channel disabled via $4015.

### 3.2 Triangle — `src/apu/triangle.rs`

**Source**: https://www.nesdev.org/wiki/APU_Triangle

```
$4008:  CRRR RRRR
  C     length counter halt + linear counter control
  RRRR RRRR  linear counter reload value (7 bits)

$4009:  unused
$400A:  TTTT TTTT  timer low
$400B:  LLLL LTTT  length load | timer high; sets linear counter reload flag
```

32-step sequence (output 0–15):

```
15 14 13 12 11 10  9  8  7  6  5  4  3  2  1  0
 0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15
```

The sequencer advances **only when both** the length counter and linear counter are non-zero. Timer reload value < 2 freezes the channel at its current step (a real hardware artifact — the channel emits ultrasonic pop). Treat it as muting (output the held step value, do not advance).

### 3.3 Noise — `src/apu/noise.rs`

**Source**: https://www.nesdev.org/wiki/APU_Noise

```
$400C:  --LC VVVV   length halt | constant volume | envelope/volume
$400E:  M--- PPPP   mode flag | period index
$400F:  LLLL L---   length load (writing also sets envelope start flag)
```

Period table (NTSC, in CPU cycles):
`4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068`.

15-bit LFSR initialised to 1. On each timer clock:
- feedback = `bit0 XOR bit1` (mode 0) or `bit0 XOR bit6` (mode 1)
- shift right; insert feedback into bit 14
- channel output is muted when `bit0 == 1` or length counter == 0.

### 3.4 DMC — `src/apu/dmc.rs`

**Source**: https://www.nesdev.org/wiki/APU_DMC

```
$4010:  IL-- RRRR   IRQ enable | loop | rate index
$4011:  -DDD DDDD   direct load (7-bit DAC)
$4012:  AAAA AAAA   sample address  = $C000 + (A * 64)
$4013:  LLLL LLLL   sample length    = (L * 16) + 1 bytes
```

Rate table (NTSC, CPU cycles per output bit): `428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54`.

**Sample fetch DMA**: when the output unit needs a new byte, the APU stalls the CPU for **4 cycles** (3 if the next CPU cycle is a write, 2 if currently in a read–modify–write op — for this milestone, model the simple 4-cycle stall and document the simplification). The DMC reads from CPU address space via the bus.

**IRQ**: when the sample finishes and loop=0 and IRQ enable=1, set the DMC interrupt flag. The CPU's `irq` line is the OR of frame-counter IRQ, DMC IRQ, and mapper IRQ (the last is already wired for MMC3 in milestone 7).

For this milestone, DMC is fully implemented including the DAC output. The 8-bit output unit's `bits_remaining` counter is reloaded with 8 when it reaches 0 (after decrementing if > 0). The DAC level is updated every sample clock based on the shift register's LSB.

### 3.5 Shared subunits

#### Envelope — `src/apu/envelope.rs`

Each invocation of "quarter-frame clock" on a channel that uses an envelope:

```
if start flag:
    start = false
    decay_level = 15
    divider = period
else:
    if divider == 0:
        divider = period
        if decay_level > 0:
            decay_level -= 1
        else if loop_flag:
            decay_level = 15
    else:
        divider -= 1

output = constant_flag ? volume : decay_level
```

#### Length counter — `src/apu/length.rs`

Length load table (32 entries, indexed by upper 5 bits of the length-load byte):

```
10, 254, 20,  2, 40,  4, 80,  6, 160,  8, 60, 10, 14, 12, 26, 14,
12,  16, 24, 18, 48, 20, 96, 22, 192, 24, 72, 26, 16, 28, 32, 30
```

Half-frame clock decrements the counter unless `halt` is set. Writing to $4015 with the corresponding enable bit cleared forces the counter to 0.

---

## 4. Frame Counter — `src/apu/frame_counter.rs`

**Source**: https://www.nesdev.org/wiki/APU_Frame_Counter

Register $4017 (write):

```
MI-- ----
M   mode (0 = 4-step, 1 = 5-step)
I   IRQ inhibit (1 = inhibit)
```

Step schedule (counted in **APU cycles** — each = 2 CPU cycles):

| Mode 0 (4-step), 14914.5 APU cycles | Mode 1 (5-step), 18640.5 APU cycles |
|--------------------------------------|--------------------------------------|
| Step 1: 3728.5  → quarter            | Step 1: 3728.5  → quarter            |
| Step 2: 7456.5  → quarter + half     | Step 2: 7456.5  → quarter + half     |
| Step 3: 11185.5 → quarter            | Step 3: 11185.5 → quarter            |
| Step 4: 14914.5 → quarter + half + IRQ (if not inhibited) | Step 4: 14914.5 → (nothing) |
|                                      | Step 5: 18640.5 → quarter + half     |

- **Quarter-frame** clocks: envelopes (pulse 1, pulse 2, noise) + triangle linear counter.
- **Half-frame** clocks: length counters (all four) + pulse sweep units.
- The IRQ flag is held until cleared by reading $4015.

**$4017 write side-effects**:
- After 3–4 CPU cycles delay, the divider and sequencer reset.
- If mode = 1, immediately clock both quarter and half frame events.
- If IRQ inhibit = 1, clear the frame-counter IRQ flag.

For this milestone, the 3–4 cycle reset delay is documented but implemented as immediate reset (acceptable simplification — it only matters for cycle-tight test ROMs, not games).

---

## 5. Status Register $4015

**Read** (returns and clears flags):

```
IF-D NT21
  I  DMC interrupt flag
  F  frame counter interrupt flag (cleared by this read)
  D  DMC sample bytes remaining > 0
  N  noise length counter > 0
  T  triangle length counter > 0
  2  pulse 2 length counter > 0
  1  pulse 1 length counter > 0
```

**Write** (channel enables; clearing a bit zeroes the corresponding length counter, and for DMC silences the channel and clears the DMC IRQ flag):

```
---D NT21
```

---

## 6. Mixer — `src/apu/mixer.rs`

**Source**: https://www.nesdev.org/wiki/APU_Mixer

Non-linear mixing formula:

```
pulse_out    = 95.88  / (8128.0  / (pulse1 + pulse2) + 100.0)         (0 if both 0)
tnd_out      = 159.79 / (1.0 / (triangle/8227 + noise/12241 + dmc/22638) + 100.0)
                                                                       (0 if all 0)
output       = pulse_out + tnd_out                          (range 0.0..1.0)
```

Implementation: precompute two lookup tables at startup —
`PULSE_TABLE[31]` for `pulse1+pulse2` (each 0..15, sum 0..30), and
`TND_TABLE[203]` for `3*triangle + 2*noise + dmc` (range 0..202). Mixing becomes two table lookups plus an add.

The mixed sample is a `f32` in `[0.0, 1.0]`. Map to `[-1.0, +1.0]` by `2x − 1` for the audio output stage.

---

## 7. APU Tick Integration in `src/main.rs`

The master-clock loop in `main.rs` already alternates CPU/PPU per master cycle. Add APU stepping:

```
on every master cycle:
    ppu.tick()
    if master % 3 == 0: cpu.tick()
    if master % 6 == 0: apu.tick()       // APU clocked once per 2 CPU cycles
    if triangle channel:                  // triangle timer is CPU-rate
        if master % 3 == 0: apu.triangle_tick()
```

`Apu::tick()` advances:
1. Pulse 1, Pulse 2, Noise, DMC timers by one APU cycle.
2. The frame counter by one APU cycle (may emit quarter/half clocks and IRQs).
3. The audio sample accumulator (see §8).

`Apu::triangle_tick()` advances only the triangle timer/sequencer.

The CPU `irq` line each cycle is recomputed as
`apu.frame_irq | apu.dmc_irq | mapper.irq()`.

---

## 8. Audio Output — `src/audio.rs`

The APU runs at ~894 kHz (NTSC CPU/2). Host audio is typically 44.1 kHz or 48 kHz. We need to **downsample**.

### 8.1 Pipeline

1. APU emits one mixer sample per APU cycle into a ring buffer at the APU rate.
2. A sample-rate converter feeds the `cpal` output stream at the host rate.
3. The output stream is mono, `f32`, host's preferred sample rate.

### 8.2 Downsampler

Approach for this milestone: **fractional-step decimation with linear interpolation**.

```
sample_step = APU_RATE / host_rate          // e.g. 894886.5 / 48000 ≈ 18.643
accumulator += 1.0 each APU sample
when accumulator >= sample_step:
    output linearly-interpolated sample to ring buffer
    accumulator -= sample_step
```

A single-pole low-pass filter (cutoff ≈ 14 kHz) precedes the decimator to suppress aliasing — the simplest IIR is sufficient:

```
y[n] = y[n-1] + alpha * (x[n] - y[n-1])
```

Two additional NES-characteristic filters from the NESDev mixer page may be added later (90 Hz HPF, 440 Hz HPF, 14 kHz LPF). For this milestone, the single LPF is acceptable.

### 8.3 cpal stream

```
audio::start() -> (Sender<f32>, Stream)
  - opens default output device, mono, default sample rate
  - background callback drains a ring buffer (e.g. ringbuf crate's
    SPSC, or a Mutex<VecDeque<f32>> behind a try_lock)
  - on underrun, output 0.0 (silence) to avoid blocking the APU thread
```

The `Stream` is held by `main.rs` for the lifetime of the program. The ring buffer producer end is owned by the APU; `Apu` calls `producer.push(sample)` from the master loop — non-blocking, drop on full.

The **producer must never block** the master clock; if the buffer is full (host slow), drop samples. This keeps emulation timing independent of audio drain rate.

### 8.4 Sync model

Audio is **emulator-driven**, not audio-driven. The master clock paces emulation (60 fps frame pacing already exists in `main.rs`); audio simply consumes whatever the APU produces. If the host audio rate drifts, occasional buffer-empty / buffer-full events are tolerated as silence / dropped samples — no resync logic is required for this milestone.

---

## 9. Bus Wiring

`Bus` gains an `apu: Apu` field and routes:

```
read:
  $4015 → apu.read_status()        // also clears frame IRQ
  // $4000–$4014, $4017 are write-only — return open bus

write:
  $4000–$4013 → apu.write_register(addr, val)
  $4015      → apu.write_status(val)
  $4017      → apu.write_frame_counter(val)
              // separate from controller 2 read; the read path stays
              // wired to controller2 (milestone 5)
```

The CPU's IRQ check at the start of each instruction now ORs in `bus.apu.irq()`.

DMC sample fetch: `Apu::dmc_dma_read(&mut Bus, u16) -> u8` reads via the existing bus read path (so PRG-ROM banking via mappers Just Works). The 4-cycle CPU stall is signalled to the master loop via an `Apu::take_stall_cycles() -> u32` accessor, which the loop subtracts from the next CPU tick budget.

---

## 10. Power-On / Reset State

**Source**: https://www.nesdev.org/wiki/APU#Status_($4015)

- All channels disabled (length counters = 0).
- Frame counter mode = 0 (4-step), IRQ inhibit = 0.
- Frame IRQ flag = 0.
- DMC: silenced, sample bytes remaining = 0, IRQ flag = 0, output level = 0.
- Noise LFSR = 1.
- Triangle sequencer position = 0.
- Pulse duty sequencer position = 0.

On **soft reset** ($4015 = 0 written, $4017 unchanged): same as power-on except $4017 is preserved.

---

## 11. Acceptance Criteria

1. **Sound output**: Super Mario Bros 1 plays the title screen music recognisably and in time. In-game jump, coin, and stomp effects fire on the correct frames.
2. **No regressions**: CPU/PPU cycle counts and rendering output for `nestest.nes` and the previously-passing Blargg PPU ROMs are unchanged byte-for-byte.
3. **Frame IRQ**: a CPU program that enables the frame IRQ (write 0x00 to $4017) and waits in a loop sees the IRQ vector taken at the expected cadence (~240 Hz). Reading $4015 clears the flag.
4. **DMC stall**: enabling DMC during a tight CPU loop visibly slows the loop's instruction throughput (observable via cycle counters in tests).
5. **$4015 read**: reflects each channel's length-counter-non-zero state and clears the frame IRQ flag on read.
6. **Channel enable bits** (write $4015): clearing a channel's bit zeroes its length counter; setting it has no immediate audible effect until the next register write that loads the length counter.
7. **Audio thread independence**: closing the window terminates the cpal stream cleanly; emulator does not deadlock on audio buffer full.

### Test ROMs

Run and pass (or document the gap if they touch unimplemented edge cases):

- `apu_test/01-len_ctr.nes` — length counter
- `apu_test/02-len_table.nes` — length-load table
- `apu_test/03-irq_flag.nes` — frame IRQ behavior
- `apu_test/04-jitter.nes` — frame counter jitter (acceptable to fail — depends on $4017 write delay simplification)
- `apu_test/05-len_timing.nes` — length-counter timing
- `apu_test/06-irq_flag_timing.nes` — IRQ timing (acceptable to fail — same reason)
- `dmc_basics.nes` — DMC core behavior
- Blargg `apu_mixer` — mixer formula sanity

Tests #4 and #6 may be deferred; document any failure with a comment in `frame_counter.rs` referencing the simplification in §4.

---

## 12. Out of Scope for This Milestone

- **PAL APU timing** — NTSC only. Period and rate tables are NTSC.
- **Cycle-exact $4017 write delay** (3–4 CPU cycles before reset) — simplified to immediate; revisit if a target game depends on it.
- **Cycle-exact DMC stall** (2/3/4 cycles depending on CPU op) — simplified to a flat 4 cycles.
- **NES-001 mixer non-linearity beyond the lookup tables** — the standard formula is good enough; no per-channel high-pass shaping beyond the single LPF.
- **External cartridge audio** (VRC6, VRC7, MMC5, Namco 163, Sunsoft 5B, FDS) — none of the in-scope mappers (0–4) expose external audio.
- **Audio recording/dumping** — no WAV export.
- **User volume control / mute hotkey** — UI work, deferred.
- **Sample-rate negotiation** — use cpal's default config; do not enumerate device capabilities.
