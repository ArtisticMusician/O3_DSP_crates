/// Simple fractional interpolator using cubic Hermite interpolation.
/// Provides high-quality sample interpolation with minimal latency.
///
/// Uses a sliding window of 4 samples and applies Hermite polynomial interpolation
/// to estimate values at fractional positions. Ideal for:
/// - Sample rate conversion
/// - Fractional delay lines
/// - Smooth parameter transitions
#[derive(Debug, Clone)]
pub struct HermiteInterpolator {
    buffer: [f32; 4],
}

impl HermiteInterpolator {
    pub fn new() -> Self {
        Self { buffer: [0.0; 4] }
    }

    #[inline(always)]
    pub fn interpolate(y0: f32, y1: f32, y2: f32, y3: f32, frac: f32) -> f32 {
        let c0 = y1;
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 1.5 * (y1 - y2) + 0.5 * (y3 - y0);
        ((c3 * frac + c2) * frac + c1) * frac + c0
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
