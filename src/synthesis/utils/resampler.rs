/// Simple Resampler (interpolator) for basic polyphase or fractional delay.
/// Uses Hermite cubic interpolation for high quality.
#[derive(Debug, Clone)]
pub struct Resampler {
    buffer: [f32; 4],
}

impl Resampler {
    pub fn new() -> Self {
        Self {
            buffer: [0.0; 4],
        }
    }

    /// Push a new sample into the history buffer.
    #[inline(always)]
    pub fn push_sample(&mut self, sample: f32) {
        self.buffer[0] = self.buffer[1];
        self.buffer[1] = self.buffer[2];
        self.buffer[2] = self.buffer[3];
        self.buffer[3] = sample;
    }

    /// Get an interpolated sample at fractional offset `t` (0.0 <= t < 1.0).
    /// t = 0.0 returns the sample at index 1 (the 'current' sample).
    /// t = 1.0 returns the sample at index 2 (the 'next' sample).
    #[inline(always)]
    pub fn process_sample(&self, t: f32) -> f32 {
        let y0 = self.buffer[0];
        let y1 = self.buffer[1];
        let y2 = self.buffer[2];
        let y3 = self.buffer[3];

        // Hermite interpolation
        let c0 = y1;
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 1.5 * (y1 - y2) + 0.5 * (y3 - y0);

        ((c3 * t + c2) * t + c1) * t + c0
    }
}
