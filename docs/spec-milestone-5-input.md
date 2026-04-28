# Milestone 5 Spec: Input — Standard Controller ($4016/$4017 Polling)

**Goal**: Wire a standard NES controller to the emulator so that keyboard (and optionally gamepad)
input reaches the running game. The CPU must be able to strobe the controller latch and shift out
button states via $4016/$4017 exactly as hardware does. Super Mario Bros 1 must respond to
directional movement and the A/B/Start/Select buttons.

**Primary references**:
- [NESDev Standard controller](https://www.nesdev.org/wiki/Standard_controller) — latch/shift
  protocol, bit layout, open-bus behavior
- [NESDev Controller reading code](https://www.nesdev.org/wiki/Controller_reading_code) — how games
  actually poll (strobe sequence, button order)
- [NESDev Input devices](https://www.nesdev.org/wiki/Input_devices) — $4016/$4017 address map,
  multi-button games, open bus on bits 3–7
- [NESDev APU registers §Joypad](https://www.nesdev.org/wiki/APU#Joypad_.28.244016-.244017.29) —
  confirms the strobe bit is $4016 bit 0; $4017 is read-only for controller 2

---

## 1. Hardware Background

The NES uses a **serial shift register** protocol for controllers. The CPU interacts via two
registers:

| Address | Direction | Purpose |
|---------|-----------|---------|
| $4016   | Write      | Strobe — bit 0 drives the parallel-load line on both controllers |
| $4016   | Read       | Controller 1 — returns one button bit per read, LSB of data bus |
| $4017   | Read       | Controller 2 — same protocol |

### Button Order (shift register bit order, first shifted out first)

```
Bit position  0    1    2      3       4  5  6  7
Button        A    B    Select Start   Up Down Left Right
```

Games strobe by writing `1` then `0` to $4016. While strobe is high the shift register
continuously reloads (so reading $4016 always returns the current A-button state). After the
falling edge, each read shifts one bit out; reading more than 8 times returns `1` for bits 8–N
(open bus / pull-up behavior).

### Bus Bit Layout on Read

On real hardware the controller data appears on bit 0; bits 1–4 are open bus (usually 0 on
first-party controllers); bits 5–7 reflect the upper address bus. For emulation purposes,
return the button bit in bit 0 and `0` for bits 1–7.

---

## 2. New Module: `src/input.rs`

Add a `Controller` struct that models one standard NES controller.

```
Controller
  buttons: u8        // current pressed state — bit layout matches shift register order
  shift: u8          // 8-bit shift register, loaded on strobe falling edge
  strobe: bool       // current state of the strobe line
```

### Button Bit Masks (defined as constants)

```rust
pub const BTN_A:      u8 = 0b0000_0001;
pub const BTN_B:      u8 = 0b0000_0010;
pub const BTN_SELECT: u8 = 0b0000_0100;
pub const BTN_START:  u8 = 0b0000_1000;
pub const BTN_UP:     u8 = 0b0001_0000;
pub const BTN_DOWN:   u8 = 0b0010_0000;
pub const BTN_LEFT:   u8 = 0b0100_0000;
pub const BTN_RIGHT:  u8 = 0b1000_0000;
```

### Methods

```
Controller::new() -> Controller
Controller::set_button(&mut self, mask: u8, pressed: bool)
Controller::write_strobe(&mut self, val: u8)
  // val & 1 drives the strobe line; falling edge (1→0) latches buttons into shift
Controller::read(&mut self) -> u8
  // if strobe high: return buttons & 1 (A button, continuously)
  // if strobe low:  shift out LSB, return it; fill from MSB with 1s after 8 reads
```

---

## 3. Bus Integration

`Bus` gains two `Controller` fields and exposes them through the existing `read`/`write`
dispatch.

```rust
pub controller1: Controller,
pub controller2: Controller,
```

In `Bus::read`:
```
$4016 → self.controller1.read()
$4017 → self.controller2.read()
```

In `Bus::write`:
```
$4016 → self.controller1.write_strobe(val); self.controller2.write_strobe(val)
        // hardware strobe line is shared; both controllers see the same strobe
```

No other bus addresses change.

---

## 4. winit Event Integration in `main.rs`

Map `winit::event::KeyboardInput` events to controller button state updates. The mapping targets
a standard QWERTY keyboard. Two layouts are supported simultaneously — both are always active,
so the player can mix keys freely.

### Key Mapping (Controller 1)

Both layouts are active at the same time; a button is pressed if **either** mapped key is held.

| NES Button | Arrow layout | WASD layout |
|------------|-------------|-------------|
| Up         | Up arrow    | W           |
| Down       | Down arrow  | S           |
| Left       | Left arrow  | A           |
| Right      | Right arrow | D           |
| A          | Z           | K           |
| B          | X           | J           |
| Select     | Right Shift | U           |
| Start      | Return      | I           |

The WASD layout is designed for one-handed use: left hand on the D-pad (WASD), right hand on
face buttons (J/K) and menu buttons (U/I).

### Integration Point

In the `winit` event loop inside `run_windowed`, handle `Event::WindowEvent { event: WindowEvent::KeyboardInput { input, .. }, .. }`:

```
on KeyboardInput:
  map VirtualKeyCode → button mask (one key may map to the same mask as another)
  let pressed = input.state == ElementState::Pressed
  bus.controller1.set_button(mask, pressed)
```

Because `set_button` sets or clears a single bit, two keys bound to the same button work
correctly: pressing either sets the bit; releasing one only clears the bit if the other is
also released. Implement this by tracking raw key state separately and recomputing `buttons`
from all held keys on each event, **or** by maintaining a held-key count per button — both
are valid; the simpler approach is to keep a `HashSet<VirtualKeyCode>` of held keys and
recompute the full `buttons` byte on every press/release.

Controller 2 is wired up but no keys are mapped by default (it returns 0 for all buttons). This
satisfies the read protocol without requiring a second key map.

---

## 5. Diagonal Input Constraint

The hardware has no restriction on simultaneous opposing directions (Up+Down or Left+Right); games
that care about this handle it in software. The emulator must **not** filter simultaneous
direction keys — pass all button states through faithfully.

---

## 6. Open Bus / Unused Bits

On read, bits 1–7 of the return value must be `0`. Do not set any of those bits from controller
state. This matches first-party controller behavior and avoids confusing games that test those
bits.

---

## 7. Acceptance Criteria

1. Super Mario Bros 1 responds to arrow keys for movement and jump (Z).
2. Start (Return) advances through title screen.
3. No regression in PPU rendering or CPU timing — existing frame output is unchanged.
4. Controller strobe/shift logic matches the NESDev standard controller reference exactly:
   - Reading $4016 eight times after a strobe returns A, B, Select, Start, Up, Down, Left, Right
     (one bit each).
   - Reads 9+ return `1` (open bus pull-up).
   - While strobe is held high, repeated reads return the live A-button state.

---

## 8. Out of Scope for This Milestone

- Gamepad/joystick support via `gilrs` or platform APIs
- Controller 2 key mapping
- NES Zapper (light gun) — $4017 bit 3/4 protocol
- Four-score / multi-tap adapters
- Mid-frame controller state changes (no games tested here require sub-frame input timing)