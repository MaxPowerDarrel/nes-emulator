pub struct Envelope {
    pub start: bool,
    pub loop_flag: bool,
    pub constant_flag: bool,
    pub divider_period: u8,
    divider: u8,
    decay_level: u8,
}

impl Envelope {
    pub fn new() -> Self {
        Self {
            start: false,
            loop_flag: false,
            constant_flag: false,
            divider_period: 0,
            divider: 0,
            decay_level: 0,
        }
    }

    pub fn tick(&mut self) {
        if self.start {
            self.start = false;
            self.decay_level = 15;
            self.divider = self.divider_period;
        } else {
            if self.divider == 0 {
                self.divider = self.divider_period;
                if self.decay_level > 0 {
                    self.decay_level -= 1;
                } else if self.loop_flag {
                    self.decay_level = 15;
                }
            } else {
                self.divider -= 1;
            }
        }
    }

    pub fn output(&self) -> u8 {
        if self.constant_flag {
            self.divider_period
        } else {
            self.decay_level
        }
    }
}
