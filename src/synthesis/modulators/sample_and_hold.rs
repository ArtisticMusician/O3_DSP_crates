use crate::synthesis::utils::ParameterSmoother;

/// Sample & Hold modulator.
/// Freezes the input signal at a sample rate determined by the update rate.
/// Creates stepped/quantized modulation signals.
///
/// Use cases:
/// - LFO-like modulation with discrete steps
/// - Random modulation sources
/// - Quantized control signals
#[derive(Debug, Clone)]
pub struct SampleAndHold {
    sample_rate: f32,

    // Update rate (how often to sample new values)

    // Current held value
    held_value: f32,

    // Input buffer for sampling
    input_buffer: Vec<f32>,
    buffer_write: usize,
    buffer_mask: usize,

    // Random seed (for deterministic randomness)
    rng_state: u32,

    // Smoothed parameters
    update_rate_smoother: ParameterSmoother,
    input_gain_smoother: ParameterSmoother,
    output_gain_smoother: ParameterSmoother,
}

impl Default for SampleAndHold {
    fn default() -> Self {
        Self::new(48000.0, 10.0) // 48kHz, 10Hz update rate
    }
}

impl SampleAndHold {
    /// Create a new Sample & Hold modulator.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `update_rate_hz` - How often to sample new values (0 = continuous)
    pub fn new(sample_rate: f32, update_rate_hz: f32) -> Self {
        let buffer_size = (update_rate_hz / sample_rate).max(1.0) as usize;
        let mask = buffer_size - 1;

        Self {
            sample_rate,
            held_value: 0.0,
            input_buffer: vec![0.0; buffer_size],
            buffer_write: 0,
            buffer_mask: mask,
            rng_state: 0xACE1_B2A7,
            update_rate_smoother: ParameterSmoother::new(sample_rate, 10.0, update_rate_hz),
            input_gain_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
            output_gain_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
        }
    }

    /// Set update rate (smoothed)
    pub fn set_update_rate(&mut self, rate_hz: f32) {
        self.update_rate_smoother.set_target(rate_hz.max(0.1).min(self.sample_rate * 0.5));
    }

    /// Set input gain (smoothed)
    pub fn set_input_gain(&mut self, gain: f32) {
        self.input_gain_smoother.set_target(gain.max(-24.0).min(24.0));
    }

    /// Set output gain (smoothed)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.max(-24.0).min(24.0));
    }

    /// Set random seed
    pub fn set_seed(&mut self, seed: u32) {
        self.rng_state = seed;
    }

    /// Process a single sample
    #[inline(always)]
    fn sample_input(&mut self, input: f32) -> f32 {
        let update_rate = self.update_rate_smoother.process_sample();
        let input_gain = self.input_gain_smoother.process_sample();

        if update_rate > 1.0 {
            // Sample new value from input
            self.input_buffer[self.buffer_write] = input * input_gain;
            self.buffer_write = (self.buffer_write + 1) & self.buffer_mask;
        }

        input
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let _update_rate = self.update_rate_smoother.process_sample();
        let output_gain = self.output_gain_smoother.process_sample();

        // Sample input if update rate is high enough
        self.sample_input(input);

        // Output the held value
        let held = self.input_buffer[self.buffer_write];
        let output = held * output_gain;

        self.held_value = output;
        output
    }

    /// Process a block of samples
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sample_and_hold() {
        let mut modulator = SampleAndHold::new(48000.0, 10.0);
        
        let mut held = 0.0;
        for _ in 0..(48_000 / 100 * 4) {
            held = modulator.process_sample(1.0);
        }

        assert!(held.is_finite(), "modulator produced {held}");
        assert!((held - 1.0).abs() < 1e-3, "should hold the sampled input: {held}");
    }

    #[test]
    fn test_update_rate() {
        let sample_rate = 48_000.0;

        // A fast modulator samples every 48 samples and catches the input almost at once;
        // a 1 Hz one samples once a second, so within 10 ms it is still holding silence.
        let mut fast = SampleAndHold::new(sample_rate, 1000.0);
        let mut slow = SampleAndHold::new(sample_rate, 1.0);

        let mut fast_value = 0.0;
        let mut slow_value = 0.0;
        for _ in 0..(sample_rate as usize / 100) {
            fast_value = fast.process_sample(1.0);
            slow_value = slow.process_sample(1.0);
        }

        assert!(
            fast_value > slow_value + 0.5,
            "fast modulator should have tracked the input: {fast_value} vs {slow_value}"
        );
    }
}