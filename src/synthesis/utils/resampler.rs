use super::hermite_interpolator::HermiteInterpolator;

/// Variable-rate cubic Hermite resampler.
///
/// `step` is the number of *input* samples consumed per *output* sample:
/// - `step = 1.0` → same rate (2-sample interpolator latency)
/// - `step > 1.0` → downsample
/// - `step < 1.0` → upsample
///
/// Construction and `set_*` may allocate nothing after `new()`. The hot path
/// only pulls samples through the caller-supplied closure.
#[derive(Debug, Clone)]
pub struct HermiteResampler {
    interpolator: HermiteInterpolator,
    frac: f32,
    step: f32,
    primed: u8,
}

impl Default for HermiteResampler {
    fn default() -> Self {
        Self::new()
    }
}

impl HermiteResampler {
    pub fn new() -> Self {
        Self {
            interpolator: HermiteInterpolator::new(),
            frac: 0.0,
            step: 1.0,
            primed: 0,
        }
    }

    /// Set the input-per-output step. Clamped to a sane positive range.
    #[inline(always)]
    pub fn set_step(&mut self, step: f32) {
        self.step = step.clamp(1.0 / 256.0, 64.0);
    }

    /// Convenience: `step = input_hz / output_hz`.
    #[inline(always)]
    pub fn set_ratio(&mut self, input_hz: f32, output_hz: f32) {
        let out = output_hz.max(1.0);
        self.set_step(input_hz.max(1.0) / out);
    }

    #[inline(always)]
    pub fn step(&self) -> f32 {
        self.step
    }

    /// Reset history. Does not change the configured step.
    pub fn reset(&mut self) {
        self.interpolator = HermiteInterpolator::new();
        self.frac = 0.0;
        self.primed = 0;
    }

    /// Render one output sample, requesting new input samples as the fractional
    /// read pointer crosses integer boundaries.
    #[inline(always)]
    pub fn next_output<F>(&mut self, mut next_input: F) -> f32
    where
        F: FnMut() -> f32,
    {
        // Prime four taps so Hermite has a valid stencil.
        while self.primed < 4 {
            self.interpolator.push_sample(next_input());
            self.primed += 1;
        }

        while self.frac >= 1.0 {
            self.interpolator.push_sample(next_input());
            self.frac -= 1.0;
        }

        let y = self.interpolator.process_sample(self.frac);
        self.frac += self.step;
        y
    }
}

#[cfg(test)]
mod tests {
    use super::HermiteResampler;

    #[test]
    fn unity_ratio_passes_dc() {
        let mut r = HermiteResampler::new();
        r.set_step(1.0);
        let mut n = 0u32;
        let mut next = || {
            n += 1;
            0.5
        };
        // Flush interpolator latency, then DC should sit near 0.5.
        for _ in 0..8 {
            let _ = r.next_output(&mut next);
        }
        let y = r.next_output(&mut next);
        assert!((y - 0.5).abs() < 1e-5, "dc through resampler: {y}");
    }

    #[test]
    fn downsample_consumes_more_inputs() {
        let mut r = HermiteResampler::new();
        r.set_step(2.0);
        let mut consumed = 0u32;
        let mut next = || {
            consumed += 1;
            0.0
        };
        for _ in 0..10 {
            let _ = r.next_output(&mut next);
        }
        // Prime 4 + ~2 per output after the first few.
        assert!(consumed > 16, "expected downsample to pull extra inputs, got {consumed}");
    }
}
