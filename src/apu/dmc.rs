const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
];

pub struct Dmc {
    pub enabled: bool,
    pub irq_enabled: bool,
    pub loop_flag: bool,
    pub rate_period: u16,
    pub rate_value: u16,
    pub sample_address: u16,
    pub sample_length: u16,
    pub current_address: u16,
    pub bytes_remaining: u16,
    pub sample_buffer: Option<u8>,
    pub shift_register: u8,
    pub bits_remaining: u8,
    pub output_level: u8,
    pub irq_flag: bool,
    pub silence: bool,
    pub dma_request: bool,
}

impl Dmc {
    pub fn new() -> Self {
        Self {
            enabled: false,
            irq_enabled: false,
            loop_flag: false,
            rate_period: DMC_RATE_TABLE[0],
            rate_value: 0,
            sample_address: 0xC000,
            sample_length: 1,
            current_address: 0xC000,
            bytes_remaining: 0,
            sample_buffer: None,
            shift_register: 0,
            bits_remaining: 0,
            output_level: 0,
            irq_flag: false,
            silence: true,
            dma_request: false,
        }
    }

    pub fn tick_timer(&mut self) {
        if self.rate_value == 0 {
            self.rate_value = self.rate_period;
            self.tick_output_unit();
        } else {
            self.rate_value -= 1;
        }
    }

    fn tick_output_unit(&mut self) {
        if !self.silence {
            if (self.shift_register & 1) == 1 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else {
                if self.output_level >= 2 {
                    self.output_level -= 2;
                }
            }
        }

        self.shift_register >>= 1;

        if self.bits_remaining > 0 {
            self.bits_remaining -= 1;
        }

        if self.bits_remaining == 0 {
            self.bits_remaining = 8;
            if let Some(val) = self.sample_buffer {
                self.silence = false;
                self.shift_register = val;
                self.sample_buffer = None;
                self.check_dma();
            } else {
                self.silence = true;
            }
        }
    }

    pub fn check_dma(&mut self) {
        if self.sample_buffer.is_none() && self.bytes_remaining > 0 {
            self.dma_request = true;
        }
    }

    pub fn dma_read(&mut self, val: u8) {
        self.sample_buffer = Some(val);
        self.dma_request = false;
        self.current_address = self.current_address.wrapping_add(1);
        if self.current_address == 0 {
            self.current_address = 0x8000;
        }
        self.bytes_remaining -= 1;
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart();
            } else if self.irq_enabled {
                self.irq_flag = true;
            }
        }
    }

    pub fn restart(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
        self.check_dma();
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.bytes_remaining = 0;
        } else if self.bytes_remaining == 0 {
            self.restart();
        }
        self.irq_flag = false;
    }

    pub fn output(&self) -> u8 {
        self.output_level
    }

    pub fn set_rate(&mut self, index: u8) {
        self.rate_period = DMC_RATE_TABLE[(index & 0x0F) as usize];
    }
}
