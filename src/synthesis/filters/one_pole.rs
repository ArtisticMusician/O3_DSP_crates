use crate::TWO_PI;

/// A single-pole Infinite Impulse Response (IIR) filter.
#[derive(Debug, Clone, Copy, Default)]
pub struct OnePoleFilter {
    a0: f32, // Input coefficient
    b1: f32, // Feedback coefficient
    z1: f32, // Single-sample delay register
}

impl OnePoleFilter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets lowpass filter coefficients based on normalized cutoff frequency (0.0 to 0.5)
    pub fn set_lowpass(&mut self, cutoff: f32) {
        let x = (-TWO_PI * cutoff.clamp(0.0, 0.5)).exp();
        self.b1 = x;
        self.a0 = 1.0 - x;
    }

    /// Sets highpass filter coefficients based on normalized cutoff frequency (0.0 to 0.5)
    pub fn set_highpass(&mut self, cutoff: f32) {
        let x = (-TWO_PI * cutoff.clamp(0.0, 0.5)).exp();
        // Highpass mapping: y[n] = a0 * x[n] - b1 * y[n-1]
        self.b1 = -x;
        self.a0 = 1.0 - x;
    }

    /// Processes a single input sample. Real-time safe.
    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        let output = (sample * self.a0) + (self.z1 * self.b1);
        self.z1 = output;
        output
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
    }
}
