use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

/// An FM Operator implementing Phase Modulation (DX-style).
#[derive(Debug, Clone)]
pub struct FmOperator {
    sample_rate: f32,
    phase: f32,
    last_output: f32, // Used for self-feedback

    // Parameters with smoothers
    base_freq: ParameterSmoother,
    freq_multiplier: ParameterSmoother,
    feedback_amount: ParameterSmoother,
    modulation_index: ParameterSmoother,
}

impl FmOperator {
    /// Creates a new FM Operator.
    pub fn new(sample_rate: f32) -> Self {
        let default_freq = 440.0;
        let default_mult = 1.0;
        let default_fb = 0.0;
        let default_mod_idx = 1.0;

        let smoothing_time_ms = 5.0; // standard 5ms smoothing

        Self {
            sample_rate,
            phase: 0.0,
            last_output: 0.0,
            base_freq: ParameterSmoother::new(sample_rate, smoothing_time_ms, default_freq),
            freq_multiplier: ParameterSmoother::new(sample_rate, smoothing_time_ms, default_mult),
            feedback_amount: ParameterSmoother::new(sample_rate, smoothing_time_ms, default_fb),
            modulation_index: ParameterSmoother::new(sample_rate, smoothing_time_ms, default_mod_idx),
        }
    }

    /// Resets the internal state (phase and last output).
    pub fn reset(&mut self) {
        self.phase = 0.0;
        self.last_output = 0.0;
    }

    /// Set the base frequency in Hz.
    pub fn set_base_frequency(&mut self, freq: f32) {
        self.base_freq
            .set_target(freq.max(0.1).min(self.sample_rate / 2.0));
    }

    /// Instantly set the base frequency without smoothing.
    pub fn reset_base_frequency(&mut self, freq: f32) {
        self.base_freq
            .reset(freq.max(0.1).min(self.sample_rate / 2.0));
    }

    /// Set the frequency multiplier (ratio).
    pub fn set_frequency_multiplier(&mut self, mult: f32) {
        self.freq_multiplier.set_target(mult.clamp(0.0, 32.0));
    }

    /// Instantly set frequency multiplier.
    pub fn reset_frequency_multiplier(&mut self, mult: f32) {
        self.freq_multiplier.reset(mult.clamp(0.0, 32.0));
    }

    /// Set the self-feedback amount.
    pub fn set_modulation_index(&mut self, index: f32) {
        self.modulation_index.set_target(index.clamp(0.0, 16.0));
    }

    pub fn reset_modulation_index(&mut self, index: f32) {
        self.modulation_index.reset(index.clamp(0.0, 16.0));
    }

    pub fn set_feedback_amount(&mut self, feedback: f32) {
        self.feedback_amount.set_target(feedback.clamp(0.0, 1.0));
    }

    /// Process a single sample with phase modulation input.
    #[inline(always)]
    pub fn next_sample(&mut self, phase_mod_input: f32) -> f32 {
        let freq = self.base_freq.process_sample();
        let mult = self.freq_multiplier.process_sample();
        let fb = self.feedback_amount.process_sample();
        let mod_idx = self.modulation_index.process_sample();

        let actual_freq = (freq * mult).min(self.sample_rate * 0.5); // Clamp product to Nyquist
        let phase_increment = actual_freq / self.sample_rate;

        // Apply phase modulation and self-feedback.
        // phase_mod_input is a bipolar [-1, 1] operator output, not cycles.
        // We use typical feedback scale: fb * 0.5
        let feedback_scale = fb * 0.5;
        let modulated_phase = self.phase + (phase_mod_input * mod_idx) + (self.last_output * feedback_scale);

        // Wrap modulated phase properly to [0.0, 1.0) using conditional subtract/add instead of fract() or while
        let wrapped_phase = modulated_phase - modulated_phase.floor();

        // Compute sine. wrapped_phase is [0.0, 1.0)
        let output = (wrapped_phase * std::f32::consts::TAU).sin();

        // Update phase accumulator with a single conditional subtract
        self.phase += phase_increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Add tiny DC bias to prevent denormals instead of branch
        let out_with_bias = output + 1e-15;
        self.last_output = out_with_bias - 1e-15;

        self.last_output
    }

    /// Process a block of samples.
    /// The `phase_mod_input` slice can be empty (all zeros assumed) or the same length as `output`.
    pub fn process_block(&mut self, phase_mod_input: &[f32], output: &mut [f32]) {
        let len = output.len();
        let pm_len = phase_mod_input.len();

        for i in 0..len {
            let pm = if i < pm_len { phase_mod_input[i] } else { 0.0 };
            output[i] = self.next_sample(pm);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization() {
        let mut op = FmOperator::new(44100.0);
        let out = op.next_sample(0.0);
        assert!(out >= -1.0 && out <= 1.0);
    }

    #[test]
    fn test_reset() {
        let mut op = FmOperator::new(44100.0);
        op.next_sample(0.0);
        op.next_sample(0.0);
        op.reset();
        assert_eq!(op.phase, 0.0);
        assert_eq!(op.last_output, 0.0);

        // First sample at phase 0 should be 0.0 for sine wave
        assert_eq!(op.next_sample(0.0), 0.0);
    }


    #[test]
    fn test_strict_fm_bounds() {
        let sample_rate = 44100.0;
        let mut op = FmOperator::new(sample_rate);
        op.reset_base_frequency(440.0);
        op.reset_frequency_multiplier(32.0); // Extreme multiplier
        op.reset_modulation_index(1.0);

        for _ in 0..10000 {
            let _out = op.next_sample(0.0);
            assert!(op.phase >= 0.0 && op.phase < 1.0, "Phase out of bounds: {}", op.phase);
        }

        // Two-op PM stays finite
        op.reset_base_frequency(440.0);
        op.reset_frequency_multiplier(1.0);
        let mut op2 = FmOperator::new(sample_rate);
        op2.reset_base_frequency(440.0);
        for _ in 0..10000 {
            let mod_val = op2.next_sample(0.0);
            let out = op.next_sample(mod_val);
            assert!(out.is_finite());
            assert!(out >= -1.0 && out <= 1.0);
        }
    }

    #[test]
    fn test_frequency_accuracy() {
        let sample_rate = 44100.0;
        let mut op = FmOperator::new(sample_rate);
        let freq = 100.0;
        op.reset_base_frequency(freq);
        op.reset_frequency_multiplier(1.0);
        op.set_feedback_amount(0.0);

        let expected_period = (sample_rate / freq).round() as usize;

        let mut last_out = op.next_sample(0.0);
        let mut start_idx = 0;
        // Warm up and find first crossing
        for i in 1..2000 {
            let out = op.next_sample(0.0);
            if last_out < 0.0 && out >= 0.0 {
                start_idx = i;
                break;
            }
            last_out = out;
        }

        let mut next_idx = 0;
        // Start looking from start_idx + 10 to avoid jitter
        for _ in start_idx + 1..start_idx + 10 {
            last_out = op.next_sample(0.0);
        }
        for i in start_idx + 10..start_idx + 2000 {
            let out = op.next_sample(0.0);
            if last_out < 0.0 && out >= 0.0 {
                next_idx = i;
                break;
            }
            last_out = out;
        }
        assert_eq!(next_idx - start_idx, expected_period);
    }
}
