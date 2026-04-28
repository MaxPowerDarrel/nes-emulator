use super::envelope::Envelope;
use super::length::LengthCounter;

const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];

pub struct Noise {
    pub enabled: bool,
    pub mode: bool,
    pub timer_period: u16,
    pub timer_value: u16,
    pub length: LengthCounter,
    pub envelope: Envelope,
    pub lfsr: u16,
}

impl Noise {
    pub fn new() -> Self {
        Self {
            enabled: false,
            mode: false,
            timer_period: 0,
            timer_value: 0,
            length: LengthCounter::new(),
            envelope: Envelope::new(),
            lfsr: 1,
        }
    }

    pub fn tick_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = self.timer_period;
            let bit_idx = if self.mode { 6 } else { 1 };
            let feedback = (self.lfsr & 1) ^ ((self.lfsr >> bit_idx) & 1);
            self.lfsr >>= 1;
            self.lfsr |= feedback << 14;
        } else {
            self.timer_value -= 1;
        }
    }

    pub fn output(&self) -> u8 {
        if !self.enabled || self.length.value == 0 || (self.lfsr & 1) == 1 {
            0
        } else {
            self.envelope.output()
        }
    }

    pub fn set_period(&mut self, index: u8) {
        self.timer_period = NOISE_PERIOD_TABLE[(index & 0x0F) as usize];
    }
}
