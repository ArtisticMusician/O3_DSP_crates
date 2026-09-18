use crate::synthesis::math_utils::linear_to_db_approx;

/// Real-time peak meter tracking instantaneous peak and configurable peak-hold decay (dBFS scale).
#[derive(Clone, Debug)]
pub struct PeakMeter {
    sample_rate: f32,
    decay_rate_db_per_sec: f32,
    peak_linear: f32,
}

impl PeakMeter {
    pub fn new(sample_rate: f32) -> Self {
        let mut meter = Self {
            sample_rate,
            decay_rate_db_per_sec: 20.0,
            peak_linear: 0.0,
        };
        meter.set_decay_rate(20.0);
        meter
    }

    pub fn reset(&mut self) {
        self.peak_linear = 0.0;
    }

    pub fn set_decay_rate(&mut self, db_per_sec: f32) {
        self.decay_rate_db_per_sec = db_per_sec.max(0.0);
    }

    #[inline(always)]
    pub fn process_sample(&mut self, sample: f32) {
        let abs_sample = sample.abs();

        if abs_sample >= self.peak_linear {
            self.peak_linear = abs_sample;
        } else {
            // Decay peak_linear to match db_per_sec
            // To decay in dB linearly, we could do log conversions each sample,
            // but for performance we can calculate an exponential decay factor
            // that approximates linear dB decay at the current level.
            // A more exact but slightly more expensive way is:
            // current_db = linear_to_db(self.peak_linear)
            // current_db -= self.decay_rate_db_per_sec / self.sample_rate
            // self.peak_linear = db_to_linear(current_db)

            // For true linear dB decay:
            let decay_factor =
                10.0_f32.powf(-self.decay_rate_db_per_sec / (20.0 * self.sample_rate));
            self.peak_linear *= decay_factor;

            // Prevent denormals
            if self.peak_linear < 1e-6 {
                self.peak_linear = 0.0;
            }
        }
    }

    #[inline(always)]
    pub fn get_peak_db(&self) -> f32 {
        if self.peak_linear <= 1e-6 {
            -144.0 // Below minimum threshold
        } else {
            linear_to_db_approx(self.peak_linear)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PeakMeter;

    #[test]
    fn tracks_peak_correctly() {
        let mut meter = PeakMeter::new(48_000.0);

        // Feed 1.0 peak
        meter.process_sample(1.0);
        assert!((meter.get_peak_db() - 0.0).abs() < 1e-2);

        // Wait a bit, peak should decay
        for _ in 0..48_000 {
            meter.process_sample(0.0);
        }

        // Should have decayed by 20 dB in 1 second
        assert!((meter.get_peak_db() - (-20.0)).abs() < 0.5);
    }
}
