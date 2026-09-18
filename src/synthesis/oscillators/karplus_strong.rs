use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

const DELAY_SIZE: usize = 4096;

/// A physical plucked string model (Karplus-Strong).
#[derive(Debug, Clone)]
pub struct KarplusStrong {
    sample_rate: f32,

    // Circular buffer for the delay line
    delay_line: [f32; DELAY_SIZE],
    write_idx: usize,

    // 1-pole lowpass filter state
    last_filter_out: f32,

    // Smoothed parameters
    base_freq: ParameterSmoother,
    damping: ParameterSmoother,
}

impl KarplusStrong {
    /// Creates a new KarplusStrong string model.
    pub fn new(sample_rate: f32) -> Self {
        let smoothing_ms = 5.0;
        let mut ks = Self {
            sample_rate,
            delay_line: [0.0; DELAY_SIZE],
            write_idx: 0,
            last_filter_out: 0.0,

            base_freq: ParameterSmoother::new(sample_rate, smoothing_ms, 220.0),
            // Damping mapping directly to 1-pole lowpass feedback coefficient
            damping: ParameterSmoother::new(sample_rate, smoothing_ms, 0.5),
        };
        // Need to reset smoothers to target value immediately so first read doesn't return 0.0 frequency
        ks.reset_frequency(220.0);
        ks.reset_damping(0.5);
        ks
    }

    /// Resets the delay line and internal states.
    pub fn reset(&mut self) {
        self.delay_line.fill(0.0);
        self.write_idx = 0;
        self.last_filter_out = 0.0;
    }

    /// Set the base frequency.
    pub fn set_frequency(&mut self, freq: f32) {
        let min_freq = self.sample_rate / (DELAY_SIZE as f32 - 1.0);
        self.base_freq
            .set_target(freq.max(min_freq).min(self.sample_rate / 2.0));
    }

    /// Instantly set the base frequency.
    pub fn reset_frequency(&mut self, freq: f32) {
        let min_freq = self.sample_rate / (DELAY_SIZE as f32 - 1.0);
        self.base_freq
            .reset(freq.max(min_freq).min(self.sample_rate / 2.0));
    }

    /// Set the damping (0.0 to 0.999).
    /// Higher values mean more high frequencies are preserved (less damping).
    pub fn set_damping(&mut self, damping: f32) {
        self.damping.set_target(damping.clamp(0.0, 0.999));
    }

    /// Instantly set the damping.
    pub fn reset_damping(&mut self, damping: f32) {
        self.damping.reset(damping.clamp(0.0, 0.999));
    }

    /// Triggers an internal burst of noise to excite the string.
    pub fn trigger(&mut self, velocity: f32) {
        let vel = velocity.clamp(0.0, 1.0);

        let freq = self.base_freq.current_value();
        // Calculate the effective delay length in samples
        let delay_samples = self.sample_rate / freq;
        let num_samples_to_fill = (delay_samples as usize).min(DELAY_SIZE);

        // Simple LCG for quick deterministic noise
        let mut seed = 123456789u32;

        for i in 0..num_samples_to_fill {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let raw_noise = (seed as f32 / u32::MAX as f32) * 2.0 - 1.0;

            // simple lowpass for noise burst to prevent extreme harshness
            let mut lp_state = 0.0;
            lp_state += 0.5 * (raw_noise - lp_state);

            // Note: Since we are filling in front of the write pointer,
            // these samples will be read *after* a delay.
            // In a simple delay line, we fill from write_idx + 1 up to write_idx + delay_len,
            // effectively placing them in the buffer to be read in the future.
            // A standard way to excite a delay line is to just dump it directly into the delay line.

            // We want to fill the *past* of the read pointer so the immediate reads pick it up.
            // If we fill starting at write_idx backwards, we fill what will be read in the next `delay_samples` ticks.
            // Because write_idx is where we *write* the *current* sample (and overwrite old data).
            // The read pointer is at `write_idx - delay_samples`.
            // So if we write starting at `write_idx - delay_samples`, the very next read will get it.

            let mut write_pos = self.write_idx as isize - delay_samples as isize + i as isize;
            while write_pos < 0 {
                write_pos += DELAY_SIZE as isize;
            }
            let idx = (write_pos as usize) % DELAY_SIZE;
            self.delay_line[idx] = lp_state * vel;
        }
    }

    /// Advance one sample with an optional external excitation signal
    #[inline(always)]
    pub fn tick_with_excitation(&mut self, external: f32) -> f32 {
        let freq = self.base_freq.process_sample();
        let damp_coef = self.damping.process_sample();

        // Calculate delay in samples
        let delay_samples = self.sample_rate / freq;

        // We read from delay_samples in the past
        let read_idx_f = self.write_idx as f32 - delay_samples;

        // Wrap float read index
        let mut read_idx_f = read_idx_f;
        while read_idx_f < 0.0 {
            read_idx_f += DELAY_SIZE as f32;
        }

        // Linear interpolation
        let idx0 = (read_idx_f as usize) % DELAY_SIZE;
        let idx1 = (idx0 + 1) % DELAY_SIZE;
        let frac = read_idx_f.fract();

        let s0 = self.delay_line[idx0];
        let s1 = self.delay_line[idx1];
        let delayed_sample = s0 + frac * (s1 - s0);

        // 1-pole lowpass filter for damping (feedback filter)
        // the damp_coef is how much feedback we have on the 1-pole filter.
        // We use it as the filter's "history" weight.
        let filter_out = delayed_sample * (1.0 - damp_coef) + self.last_filter_out * damp_coef;

        // In Karplus-Strong, there's also a decay factor to simulate string loss,
        // usually close to 0.99 or 0.999. If damping is the feedback filter,
        // we should apply a slight decay to ensure energy drops over time.
        // The problem description says "Damping: Direct mapping to the 1-pole lowpass feedback coefficient"
        let safe_filter_out = if filter_out.abs() < 1e-15 {
            0.0
        } else {
            filter_out * 0.99
        };
        self.last_filter_out = safe_filter_out;

        // Get output sample before writing back to avoid loop closure issues
        let out_sample = (delayed_sample + external).clamp(-1.0, 1.0);

        // Write feedback to delay line
        let feedback_sample = (safe_filter_out + external).clamp(-1.0, 1.0);

        // Write back to delay line
        self.delay_line[self.write_idx] = feedback_sample;

        // Advance write pointer
        self.write_idx = (self.write_idx + 1) % DELAY_SIZE;

        out_sample
    }

    /// Advance one sample with no external excitation.
    #[inline(always)]
    pub fn next_sample(&mut self) -> f32 {
        self.tick_with_excitation(0.0)
    }

    /// Process a block of samples.
    pub fn process_block(&mut self, output: &mut [f32]) {
        for out in output.iter_mut() {
            *out = self.next_sample();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization() {
        let mut ks = KarplusStrong::new(44100.0);
        let out = ks.next_sample();
        assert!(out >= -1.0 && out <= 1.0);
    }

    #[test]
    fn test_reset() {
        let mut ks = KarplusStrong::new(44100.0);
        ks.trigger(1.0);
        ks.next_sample();
        ks.reset();

        // After reset, delay line is empty, so output should be 0
        for _ in 0..100 {
            assert_eq!(ks.next_sample(), 0.0);
        }
    }

    #[test]
    fn test_trigger_and_decay() {
        let mut ks = KarplusStrong::new(44100.0);
        ks.set_damping(0.5);
        ks.trigger(1.0);

        let mut max_val = 0.0f32;
        for _ in 0..4096 {
            let s = ks.next_sample();
            if s.abs() > max_val {
                max_val = s.abs();
            }
        }

        assert!(max_val > 0.0, "Output is perfectly silent after trigger");
        assert!(max_val <= 1.0);

        // Run for a long time, it should decay
        for _ in 0..88200 {
            ks.next_sample();
        }

        let out = ks.next_sample();
        assert!(
            out.abs() < max_val,
            "Energy should decay over time, initial max: {}, current: {}",
            max_val,
            out.abs()
        );
    }
}
