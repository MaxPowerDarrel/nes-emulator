pub struct FrameCounter {
    pub mode: u8,
    pub irq_inhibit: bool,
    pub irq_flag: bool,
    pub cycle: u32,
}

impl FrameCounter {
    pub fn new() -> Self {
        Self {
            mode: 0,
            irq_inhibit: false,
            irq_flag: false,
            cycle: 0,
        }
    }

    pub fn tick(&mut self) -> (bool, bool, bool) {
        self.cycle += 1;
        let mut quarter = false;
        let mut half = false;
        let mut irq = false;

        if self.mode == 0 {
            // 4-step mode
            match self.cycle {
                7457 => {
                    quarter = true;
                }
                14913 => {
                    quarter = true;
                    half = true;
                }
                22371 => {
                    quarter = true;
                }
                29828 => {
                    // This is ~14914.5 APU cycles = 29829 CPU cycles.
                    // The spec says 14914.5 APU cycles.
                    // 1 APU cycle = 2 CPU cycles.
                }
                29829 => {
                    quarter = true;
                    half = true;
                    if !self.irq_inhibit {
                        self.irq_flag = true;
                        irq = true;
                    }
                    self.cycle = 0;
                }
                _ => {}
            }
        } else {
            // 5-step mode
            match self.cycle {
                7457 => {
                    quarter = true;
                }
                14913 => {
                    quarter = true;
                    half = true;
                }
                22371 => {
                    quarter = true;
                }
                29829 => {
                    // Step 4: nothing
                }
                37281 => {
                    quarter = true;
                    half = true;
                    self.cycle = 0;
                }
                _ => {}
            }
        }

        (quarter, half, irq)
    }

    pub fn write(&mut self, val: u8) {
        self.mode = (val >> 7) & 1;
        self.irq_inhibit = (val & 0x40) != 0;
        if self.irq_inhibit {
            self.irq_flag = false;
        }
        self.cycle = 0;
        // In 5-step mode, reset also triggers quarter and half clocks immediately.
        // For simplicity, we can return these flags or handle them in Apu::write_frame_counter.
    }
}
