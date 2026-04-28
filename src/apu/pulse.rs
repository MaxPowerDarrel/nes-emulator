use super::envelope::Envelope;
use super::length::LengthCounter;

const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];

pub struct Pulse {
    pub is_pulse2: bool,
    pub enabled: bool,
    pub duty_idx: u8,
    pub seq_idx: u8,
    pub timer_period: u16,
    pub timer_value: u16,
    pub length: LengthCounter,
    pub envelope: Envelope,
    pub sweep_enabled: bool,
    pub sweep_period: u8,
    pub sweep_divider: u8,
    pub sweep_negate: bool,
    pub sweep_shift: u8,
    pub sweep_reload: bool,
}

impl Pulse {
    pub fn new(is_pulse2: bool) -> Self {
        Self {
            is_pulse2,
            enabled: false,
            duty_idx: 0,
            seq_idx: 0,
            timer_period: 0,
            timer_value: 0,
            length: LengthCounter::new(),
            envelope: Envelope::new(),
            sweep_enabled: false,
            sweep_period: 0,
            sweep_divider: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_reload: false,
        }
    }

    pub fn tick_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = self.timer_period;
            self.seq_idx = (self.seq_idx + 1) % 8;
        } else {
            self.timer_value -= 1;
        }
    }

    pub fn tick_sweep(&mut self) {
        if self.sweep_divider == 0
            && self.sweep_enabled
            && !self.is_sweep_muting()
            && self.sweep_shift > 0
        {
            self.timer_period = self.calculate_sweep_target();
        }

        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    fn calculate_sweep_target(&self) -> u16 {
        let delta = self.timer_period >> self.sweep_shift;
        if self.sweep_negate {
            if self.is_pulse2 {
                self.timer_period.wrapping_sub(delta)
            } else {
                self.timer_period.wrapping_sub(delta).wrapping_sub(1)
            }
        } else {
            self.timer_period.wrapping_add(delta)
        }
    }

    fn is_sweep_muting(&self) -> bool {
        self.timer_period < 8 || self.calculate_sweep_target() > 0x7FF
    }

    pub fn output(&self) -> u8 {
        if !self.enabled
            || self.length.value == 0
            || DUTY_TABLE[self.duty_idx as usize][self.seq_idx as usize] == 0
            || self.is_sweep_muting()
        {
            0
        } else {
            self.envelope.output()
        }
    }
}
