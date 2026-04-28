/// NES standard controller — serial shift-register protocol.
///
/// Spec: https://www.nesdev.org/wiki/Standard_controller
///
/// Button bit layout (shift-register order, first shifted out first):
///   bit 0 = A, 1 = B, 2 = Select, 3 = Start, 4 = Up, 5 = Down, 6 = Left, 7 = Right
pub const BTN_A: u8 = 0b0000_0001;
pub const BTN_B: u8 = 0b0000_0010;
pub const BTN_SELECT: u8 = 0b0000_0100;
pub const BTN_START: u8 = 0b0000_1000;
pub const BTN_UP: u8 = 0b0001_0000;
pub const BTN_DOWN: u8 = 0b0010_0000;
pub const BTN_LEFT: u8 = 0b0100_0000;
pub const BTN_RIGHT: u8 = 0b1000_0000;

pub struct Controller {
    /// Live button state — updated by the host on key events.
    pub buttons: u8,
    /// Shift register — latched from `buttons` on the strobe falling edge.
    shift: u8,
    /// Current state of the strobe line (bit 0 of the last write to $4016).
    strobe: bool,
    /// Number of bits already shifted out since the last latch.
    shift_count: u8,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            buttons: 0,
            shift: 0,
            strobe: false,
            shift_count: 0,
        }
    }

    #[allow(dead_code)]
    pub fn set_button(&mut self, mask: u8, pressed: bool) {
        if pressed {
            self.buttons |= mask;
        } else {
            self.buttons &= !mask;
        }
    }

    /// Called on a CPU write to $4016. Bit 0 drives the strobe line on both controllers.
    ///
    /// Rising edge: strobe goes high — controller continuously reloads from live buttons.
    /// Falling edge (1→0): latch current button state into the shift register.
    pub fn write_strobe(&mut self, val: u8) {
        let new_strobe = (val & 1) != 0;
        if self.strobe && !new_strobe {
            // Falling edge — latch buttons into shift register.
            self.shift = self.buttons;
            self.shift_count = 0;
        }
        self.strobe = new_strobe;
    }

    /// Called on a CPU read of $4016 (controller 1) or $4017 (controller 2).
    ///
    /// While strobe is high: return current A-button state continuously (bit 0 only).
    /// While strobe is low: shift out one bit per read, LSB first.
    ///   Reads 0–7 return the latched button bits in shift-register order.
    ///   Reads 8+ return 1 (open-bus pull-up behavior).
    pub fn read(&mut self) -> u8 {
        if self.strobe {
            return self.buttons & 1;
        }

        if self.shift_count >= 8 {
            return 1; // open-bus pull-up after all 8 bits have been shifted out
        }

        let bit = self.shift & 1;
        self.shift >>= 1;
        self.shift_count += 1;
        bit
    }
}