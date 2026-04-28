pub const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

pub struct LengthCounter {
    pub value: u8,
    pub halt: bool,
    pub enabled: bool,
}

impl LengthCounter {
    pub fn new() -> Self {
        Self {
            value: 0,
            halt: false,
            enabled: false,
        }
    }

    pub fn load(&mut self, index: u8) {
        if self.enabled {
            self.value = LENGTH_TABLE[(index >> 3) as usize];
        }
    }

    pub fn tick(&mut self) {
        if !self.halt && self.value > 0 {
            self.value -= 1;
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.value = 0;
        }
    }
}
