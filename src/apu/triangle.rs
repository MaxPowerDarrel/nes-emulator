use super::length::LengthCounter;

const TRIANGLE_SEQUENCE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

pub struct Triangle {
    pub enabled: bool,
    pub timer_period: u16,
    pub timer_value: u16,
    pub seq_idx: u8,
    pub length: LengthCounter,
    pub linear_control: bool,
    pub linear_reload_value: u8,
    pub linear_value: u8,
    pub linear_reload_flag: bool,
}

impl Triangle {
    pub fn new() -> Self {
        Self {
            enabled: false,
            timer_period: 0,
            timer_value: 0,
            seq_idx: 0,
            length: LengthCounter::new(),
            linear_control: false,
            linear_reload_value: 0,
            linear_value: 0,
            linear_reload_flag: false,
        }
    }

    pub fn tick_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = self.timer_period;
            if self.length.value > 0 && self.linear_value > 0 {
                self.seq_idx = (self.seq_idx + 1) % 32;
            }
        } else {
            self.timer_value -= 1;
        }
    }

    pub fn tick_linear(&mut self) {
        if self.linear_reload_flag {
            self.linear_value = self.linear_reload_value;
        } else if self.linear_value > 0 {
            self.linear_value -= 1;
        }

        if !self.linear_control {
            self.linear_reload_flag = false;
        }
    }

    pub fn output(&self) -> u8 {
        TRIANGLE_SEQUENCE[self.seq_idx as usize]
    }
}
