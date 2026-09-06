use crate::synthesis::utils::Processor;

/// First-order DC blocker.
///
/// Transfer: `y[n] = x[n] - x[n-1] + r * y[n-1]`.
/// `r` close to 1.0 (default 0.995) places the zero at DC and the pole just
/// inside the unit circle. Real-time safe, no allocations.
#[derive(Debug, Clone, Copy)]
pub struct DcBlocker {
    r: f32,
    x1: f32,
    y1: f32,
}

impl Default for DcBlocker {
    fn default() -> Self {
        Self::new(0.995)
    }
}

impl DcBlocker {
    pub fn new(r: f32) -> Self {
        Self {
            r: r.clamp(0.9, 0.9999),
            x1: 0.0,
            y1: 0.0,
        }
    }

    /// Typical musical default (~5–20 Hz highpass depending on sample rate).
    pub fn new_default() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.y1 = 0.0;
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = x - self.x1 + self.r * self.y1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

impl Processor for DcBlocker {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        self.process(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::DcBlocker;

    #[test]
    fn rejects_dc_after_settling() {
        let mut flt = DcBlocker::new(0.995);
        let mut y = 0.0;
        for _ in 0..2048 {
            y = flt.process(1.0);
        }
        assert!(y.abs() < 0.02, "residual dc {y}");
    }

    #[test]
    fn passes_impulse_energy() {
        let mut flt = DcBlocker::new(0.995);
        let first = flt.process(1.0);
        assert!(first.abs() > 0.5);
    }
}
