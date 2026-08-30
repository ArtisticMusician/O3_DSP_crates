
/// A generic parameter smoother to avoid zipper noise when modulating parameters.
#[derive(Debug, Clone, Copy)]
pub struct ParameterSmoother {
    target: f32,
    current: f32,
    coef: f32,
}

impl ParameterSmoother {
    /// Creates a new parameter smoother.
    /// `time_ms` is the time it takes to reach approximately 63% of the target value.
    pub fn new(sample_rate: f32, time_ms: f32, initial_value: f32) -> Self {
        let mut smoother = Self {
            target: initial_value,
            current: initial_value,
            coef: 0.0,
        };
        smoother.set_time(sample_rate, time_ms);
        smoother
    }

    /// Update the smoothing time.
    pub fn set_time(&mut self, sample_rate: f32, time_ms: f32) {
        if time_ms <= 0.0 {
            self.coef = 1.0;
        } else {
            // Equivalent to 1.0 - exp(-1.0 / (time_ms * 0.001 * sample_rate))
            // But doing an approximation or exact calculation:
            self.coef = 1.0 - (-1.0 / (time_ms * 0.001 * sample_rate)).exp();
        }
    }

    /// Set a new target value.
    #[inline(always)]
    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Instantly set the current value (e.g. for hard resets).
    #[inline(always)]
    pub fn reset(&mut self, value: f32) {
        self.target = value;
        self.current = value;
    }

    /// Process one sample of smoothing. Returns the smoothed value.
    #[inline(always)]
    pub fn process_sample(&mut self) -> f32 {
        self.current += (self.target - self.current) * self.coef;
        self.current
    }

    /// Process a block of smoothing, writing into the output buffer.
    #[inline]
    pub fn process_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            self.current += (self.target - self.current) * self.coef;
            *out = self.current;
        }
    }

    /// Get the current value without updating.
    #[inline(always)]
    pub fn current_value(&self) -> f32 {
        self.current
    }
}
