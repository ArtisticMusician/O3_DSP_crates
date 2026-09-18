use crate::synthesis::math_utils::{db_to_linear_approx, linear_to_db_approx};

/// Feed-forward stereo peak limiter.
///
/// Instant attack (envelope tracks peak immediately) and exponential release.
/// No lookahead buffer: cheap enough for a voice or master-bus safety stage.
/// All state is stack-sized; construction allocates nothing.
#[derive(Clone, Debug)]
pub struct PeakLimiter {
    sample_rate: f32,
    ceiling_db: f32,
    ceiling_lin: f32,
    release_coef: f32,
    envelope: f32,
}

impl PeakLimiter {
    pub fn new(sample_rate: f32) -> Self {
        let mut lim = Self {
            sample_rate,
            ceiling_db: -0.1,
            ceiling_lin: db_to_linear_approx(-0.1),
            release_coef: 0.0,
            envelope: 0.0,
        };
        lim.set_release_ms(50.0);
        lim
    }

    pub fn set_ceiling_db(&mut self, ceiling_db: f32) {
        self.ceiling_db = ceiling_db.clamp(-24.0, 0.0);
        self.ceiling_lin = db_to_linear_approx(self.ceiling_db);
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        let time_sec = (ms * 0.001).max(1e-4);
        self.release_coef = (-1.0 / (time_sec * self.sample_rate)).exp();
    }

    /// Current gain reduction in dB (positive when limiting).
    #[inline(always)]
    pub fn gain_reduction_db(&self) -> f32 {
        let peak = self.envelope.max(1e-12);
        let gr = self.ceiling_lin / peak;
        if gr < 1.0 {
            -linear_to_db_approx(gr)
        } else {
            0.0
        }
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
    }

    #[inline(always)]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let peak = left.abs().max(right.abs());

        if peak > self.envelope {
            self.envelope = peak;
        } else {
            self.envelope = self.release_coef * self.envelope + (1.0 - self.release_coef) * peak;
        }

        let gain = if self.envelope > self.ceiling_lin {
            self.ceiling_lin / self.envelope
        } else {
            1.0
        };

        (left * gain, right * gain)
    }

    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        self.process_sample(sample, sample).0
    }

    #[inline]
    pub fn process_block(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (out_l, out_r) = self.process_sample(*l, *r);
            *l = out_l;
            *r = out_r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::PeakLimiter;

    #[test]
    fn limits_hot_peak() {
        let mut lim = PeakLimiter::new(48_000.0);
        lim.set_ceiling_db(-1.0);
        let (l, r) = lim.process_sample(4.0, -4.0);
        assert!(l.abs() <= 0.95, "left {l}");
        assert!(r.abs() <= 0.95, "right {r}");
    }

    #[test]
    fn silence_is_unchanged() {
        let mut lim = PeakLimiter::new(48_000.0);
        let (l, r) = lim.process_sample(0.0, 0.0);
        assert_eq!((l, r), (0.0, 0.0));
    }
}
