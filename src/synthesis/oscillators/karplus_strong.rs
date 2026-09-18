extern crate alloc;
use crate::synthesis::utils::parameter_smoother::ParameterSmoother;



/// A physical plucked string model (Karplus-Strong).
#[derive(Debug, Clone)]
pub struct KarplusStrong {
    sample_rate: f32,

    // Circular buffer for the delay line
    delay_line: alloc::vec::Vec<f32>,
    mask: usize,
    seed: u32,
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

        // Minimum frequency of ~20Hz
        let min_freq = 20.0;
        let required_samples = (sample_rate / min_freq).ceil() as usize;
        // Need next power of two for mask
        let capacity = (required_samples + 4).next_power_of_two();

        let mut ks = Self {
            sample_rate,
            delay_line: alloc::vec![0.0; capacity],
            mask: capacity - 1,
            seed: 123456789,
            write_idx: 0,
            last_filter_out: 0.0,

            base_freq: ParameterSmoother::new(sample_rate, smoothing_ms, 220.0),
            // Damping mapping to 1-pole lowpass feedback coefficient.
            // y = 0.5 * (1+damp) * tap + 0.5 * (1-damp) * prev_tap
            // where damp=1.0 is brightest, damp=0.0 is dull.
            damping: ParameterSmoother::new(sample_rate, smoothing_ms, 1.0),
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
        let min_freq = self.sample_rate / ((self.mask) as f32 - 2.0);
        self.base_freq
            .set_target(freq.max(min_freq).min(self.sample_rate / 2.0));
    }

    /// Instantly set the base frequency.
    pub fn reset_frequency(&mut self, freq: f32) {
        let min_freq = self.sample_rate / ((self.mask) as f32 - 2.0);
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
        let delay_samples = self.sample_rate / freq;
        let num_samples_to_fill = (delay_samples as usize).min(self.mask);

        let mut lp_state = 0.0;

        // Fill the region that the next reads will see (write_idx - delay .. write_idx)
        let start_write = (self.write_idx as isize - num_samples_to_fill as isize + self.delay_line.len() as isize) as usize;

        for i in 0..num_samples_to_fill {
            self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let raw_noise = (self.seed as f32 / u32::MAX as f32) * 2.0 - 1.0;

            lp_state += 0.5 * (raw_noise - lp_state);

            let idx = (start_write + i) & self.mask;
            self.delay_line[idx] = lp_state * vel;
        }
    }

    /// Advance one sample with an optional external excitation signal
    #[inline(always)]
    pub fn tick_with_excitation(&mut self, external: f32) -> f32 {
        let freq = self.base_freq.process_sample();
        let damp_coef = self.damping.process_sample();

        let mut delay_samples = self.sample_rate / freq;
        delay_samples = delay_samples.clamp(2.0, (self.delay_line.len() - 2) as f32);

        // Single wrap of float read index
        let mut read_idx_f = self.write_idx as f32 - delay_samples;
        if read_idx_f < 0.0 {
            read_idx_f += self.delay_line.len() as f32;
        }

        let idx0 = (read_idx_f as usize) & self.mask;
        let idx1 = (idx0 + 1) & self.mask;
        let frac = read_idx_f.fract();

        let s0 = self.delay_line[idx0];
        let s1 = self.delay_line[idx1];
        let delayed_sample = s0 + frac * (s1 - s0);

        // Loop filter: y = 0.5 * (1+damp) * tap + 0.5 * (1-damp) * prev_tap
        // damp=1.0 is brightest, damp=0.0 is dull.
        let filter_out = 0.5 * (1.0 + damp_coef) * delayed_sample + 0.5 * (1.0 - damp_coef) * self.last_filter_out;

        // Add tiny bias to prevent denormals
        let safe_filter_out = filter_out + 1e-15;
        self.last_filter_out = safe_filter_out - 1e-15;

        // Output waveguide sample
        let out_sample = self.last_filter_out;

        // Write feedback to delay line (with 0.999 fixed decay for realism)
        let feedback_sample = self.last_filter_out * 0.999 + external;

        // Cheap saturation to prevent blowing up if external is crazy
        let saturated = feedback_sample / (1.0 + feedback_sample.abs());

        self.delay_line[self.write_idx] = saturated;
        self.write_idx = (self.write_idx + 1) & self.mask;

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
    fn test_strict_ks_decay() {
        let mut ks = KarplusStrong::new(44100.0);
        ks.reset_frequency(220.0);
        ks.reset_damping(1.0);
        ks.trigger(1.0);

        let mut peak = 0.0f32;
        for _ in 0..1000 {
            let s = ks.next_sample().abs();
            if s > peak { peak = s; }
        }

        // Skip ahead ~2 seconds
        for _ in 0..88200 {
            ks.next_sample();
        }

        // Calculate RMS over the next 1000 samples
        let mut sum_sq = 0.0;
        for _ in 0..1000 {
            let s = ks.next_sample();
            sum_sq += s * s;
        }
        let rms = (sum_sq / 1000.0).sqrt();
        assert!(rms < 0.05 * peak, "RMS {} should be < {} (5% of peak)", rms, 0.05 * peak);
    }

    #[test]
    fn test_high_sample_rate() {
        let mut ks = KarplusStrong::new(96000.0);
        ks.reset_frequency(40.0);
        ks.trigger(1.0);
        for _ in 0..4000 {
            ks.next_sample();
        }
    }
}
