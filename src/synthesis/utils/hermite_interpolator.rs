/// 4-point cubic Hermite interpolator.
#[derive(Debug, Clone, Copy)]
pub struct HermiteInterpolator {
    buffer: [f32; 4],
}

impl HermiteInterpolator {
    pub fn new() -> Self {
        Self { buffer: [0.0; 4] }
    }

    #[inline(always)]
    pub fn push_sample(&mut self, sample: f32) {
        self.buffer[0] = self.buffer[1];
        self.buffer[1] = self.buffer[2];
        self.buffer[2] = self.buffer[3];
        self.buffer[3] = sample;
    }

    /// Interpolate four neighboring samples. `t` in [0, 1) between y1 and y2.
    #[inline(always)]
    pub fn interpolate(y0: f32, y1: f32, y2: f32, y3: f32, t: f32) -> f32 {
        let c0 = y1;
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 1.5 * (y1 - y2) + 0.5 * (y3 - y0);
        ((c3 * t + c2) * t + c1) * t + c0
    }

    #[inline(always)]
    pub fn process_sample(&self, t: f32) -> f32 {
        Self::interpolate(
            self.buffer[0],
            self.buffer[1],
            self.buffer[2],
            self.buffer[3],
            t,
        )
    }
}
