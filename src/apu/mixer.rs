pub struct Mixer {
    pulse_table: [f32; 31],
}

impl Mixer {
    pub fn new() -> Self {
        let mut pulse_table = [0.0f32; 31];
        for (i, slot) in pulse_table.iter_mut().enumerate().skip(1) {
            *slot = 95.88 / (8128.0 / (i as f32) + 100.0);
        }
        Self { pulse_table }
    }

    pub fn mix(&self, p1: u8, p2: u8, t: u8, n: u8, d: u8) -> f32 {
        let pulse_out = self.pulse_table[(p1 + p2) as usize];

        let tnd_out = if t == 0 && n == 0 && d == 0 {
            0.0
        } else {
            159.79 / (1.0 / (t as f32 / 8227.0 + n as f32 / 12241.0 + d as f32 / 22638.0) + 100.0)
        };

        pulse_out + tnd_out
    }
}
