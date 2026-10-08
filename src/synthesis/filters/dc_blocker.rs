use crate::synthesis::utils::{ParameterSmoother, Processor};

/// First-order DC blocker.
///
/// Transfer: `y[n] = x[n] - x[n-1] + r * y[n-1]`.
/// `r` close to 1.0 (default 0.995) places the zero at DC and the pole just
/// inside the unit circle. Real-time safe, no allocations.
#[derive(Debug, Clone, Copy)]
pub struct DcBlocker {
    sample_rate: f32,
    r: f32,
    x1: f32,
    y1: f32,
    cutoff_smoother: ParameterSmoother,
    current_cutoff: f32,
}

impl Default for DcBlocker {
    fn default() -> Self {
        Self::new(48000.0, 20.0)
    }
}

impl DcBlocker {
    pub fn new(sample_rate: f32, cutoff_hz: f32) -> Self {
        let omega = 2.0 * std::f32::consts::PI * cutoff_hz / sample_rate;
        let r = 1.0 - omega / (2.0 * (1.0 + omega));

        Self {
            sample_rate,
            r: r.clamp(0.9, 0.9999),
            x1: 0.0,
            y1: 0.0,
            cutoff_smoother: ParameterSmoother::new(sample_rate, 10.0, cutoff_hz),
            current_cutoff: cutoff_hz,
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

    pub fn set_cutoff(&mut self, cutoff_hz: f32) {
        self.cutoff_smoother.set_target(cutoff_hz.max(1.0).min(self.sample_rate * 0.3));
    }

    pub fn cutoff(&self) -> f32 {
        self.current_cutoff
    }

    #[inline(always)]
    fn update_r(&mut self) {
        let cutoff = self.cutoff_smoother.process_sample();
        self.current_cutoff = cutoff;

        let omega = 2.0 * std::f32::consts::PI * cutoff / self.sample_rate;
        self.r = 1.0 - omega / (2.0 * (1.0 + omega));
    }

    #[inline(always)]
    pub fn process(&mut self, x: f32) -> f32 {
        self.update_r();
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
        let mut flt = DcBlocker::new(48_000.0, 20.0);
        let mut y = 0.0;
        // ~20 Hz gives r ~= 0.9987, so the pole needs ~4 time constants to settle.
        for _ in 0..8192 {
            y = flt.process(1.0);
        }
        assert!(y.abs() < 0.02, "residual dc {y}");
    }

    #[test]
    fn passes_impulse_energy() {
        let mut flt = DcBlocker::new(48_000.0, 20.0);
        let first = flt.process(1.0);
        assert!(first.abs() > 0.5);
    }
}
