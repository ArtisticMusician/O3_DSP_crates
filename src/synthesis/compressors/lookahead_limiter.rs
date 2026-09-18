use crate::synthesis::math_utils::{db_to_linear_approx, linear_to_db_approx};

/// True lookahead brickwall stereo peak limiter.
#[derive(Clone, Debug)]
pub struct LookaheadLimiter {
    sample_rate: f32,
    ceiling_db: f32,
    ceiling_lin: f32,
    release_coef: f32,

    lookahead_ms: f32,
    lookahead_samples: usize,

    delay_l: Vec<f32>,
    delay_r: Vec<f32>,
    write_idx: usize,

    envelope: f32,
    applied_gain: f32,
}

impl LookaheadLimiter {
    pub fn new(sample_rate: f32) -> Self {
        let max_lookahead_ms = 10.0;
        let max_samples = (max_lookahead_ms * 0.001 * sample_rate).ceil() as usize;
        // Preallocate length + 1 to easily store 0ms to 10ms of delay safely.
        let buf_len = max_samples + 1;

        let mut lim = Self {
            sample_rate,
            ceiling_db: -0.1,
            ceiling_lin: db_to_linear_approx(-0.1),
            release_coef: 0.0,

            lookahead_ms: 5.0,
            lookahead_samples: 0,

            delay_l: vec![0.0; buf_len],
            delay_r: vec![0.0; buf_len],
            write_idx: 0,

            envelope: 0.0,
            applied_gain: 1.0,
        };
        lim.set_lookahead_ms(5.0);
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

    pub fn set_lookahead_ms(&mut self, ms: f32) {
        self.lookahead_ms = ms.clamp(0.0, 10.0);
        self.lookahead_samples = (self.lookahead_ms * 0.001 * self.sample_rate).ceil() as usize;
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
        self.applied_gain = 1.0;
        self.delay_l.fill(0.0);
        self.delay_r.fill(0.0);
        self.write_idx = 0;
    }

    #[inline(always)]
    pub fn gain_reduction_db(&self) -> f32 {
        if self.applied_gain < 1.0 {
            -linear_to_db_approx(self.applied_gain)
        } else {
            0.0
        }
    }

    #[inline(always)]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        // 1. Write incoming L/R into pre-allocated circular delay
        self.delay_l[self.write_idx] = left;
        self.delay_r[self.write_idx] = right;

        // 2. Maintain a sliding-window maximum over the last N samples
        let mut max_peak = 0.0f32;
        let mut idx = self.write_idx;
        let len = self.delay_l.len();

        for _ in 0..=self.lookahead_samples {
            let l_abs = self.delay_l[idx].abs();
            let r_abs = self.delay_r[idx].abs();
            let p = if l_abs > r_abs { l_abs } else { r_abs };
            if p > max_peak {
                max_peak = p;
            }

            if idx == 0 {
                idx = len - 1;
            } else {
                idx -= 1;
            }
        }

        // 3. Envelope: instant attack to window max; exponential release
        if max_peak > self.envelope {
            self.envelope = max_peak;
        } else {
            self.envelope = self.release_coef * self.envelope + (1.0 - self.release_coef) * max_peak;
        }

        // 4. Calculate gain
        let gain = if self.envelope > self.ceiling_lin {
            self.ceiling_lin / self.envelope
        } else {
            1.0
        };
        self.applied_gain = gain;

        // 5. Apply that gain to the delayed sample
        let mut read_idx = self.write_idx;
        for _ in 0..self.lookahead_samples {
            if read_idx == 0 {
                read_idx = len - 1;
            } else {
                read_idx -= 1;
            }
        }

        let out_l = self.delay_l[read_idx] * gain;
        let out_r = self.delay_r[read_idx] * gain;

        // 6. Wrap index
        self.write_idx += 1;
        if self.write_idx >= len {
            self.write_idx = 0;
        }

        (out_l, out_r)
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
    use super::LookaheadLimiter;

    #[test]
    fn impulse_respects_ceiling_and_lookahead() {
        let mut lim = LookaheadLimiter::new(48_000.0);
        lim.set_ceiling_db(-1.0);
        lim.set_lookahead_ms(5.0);
        let lookahead_samples = lim.lookahead_samples;

        let mut max_l = 0.0f32;
        let mut max_r = 0.0f32;

        let mut gr_at_impulse = 0.0;

        for i in 0..2000 {
            let input = if i == 200 { 4.0 } else { 0.0 };
            let (l, r) = lim.process_sample(input, -input);
            max_l = max_l.max(l.abs());
            max_r = max_r.max(r.abs());

            if i == 200 + lookahead_samples {
                gr_at_impulse = lim.gain_reduction_db();
            }
        }

        assert!(max_l <= 0.892, "left exceeded ceiling: {max_l}");
        assert!(max_r <= 0.892, "right exceeded ceiling: {max_r}");
        assert!(gr_at_impulse > 0.0, "Gain reduction should have applied by the time impulse exits");
    }

    #[test]
    fn sine_below_ceiling_unchanged_level() {
        let mut lim = LookaheadLimiter::new(48_000.0);
        lim.set_ceiling_db(-1.0);
        lim.set_lookahead_ms(5.0);

        for _ in 0..1000 {
            lim.process_sample(0.0, 0.0);
        }

        let (_l, _r) = lim.process_sample(0.5, -0.5);
        // It's delayed, so we need to process `lookahead_samples` more samples to get it out
        let mut out_l = 0.0;
        let mut out_r = 0.0;
        for _ in 0..lim.lookahead_samples {
            let (next_l, next_r) = lim.process_sample(0.0, 0.0);
            out_l = next_l;
            out_r = next_r;
        }

        assert!((out_l.abs() - 0.5).abs() < 1e-4, "Left output attenuated: {out_l}");
        assert!((out_r.abs() - 0.5).abs() < 1e-4, "Right output attenuated: {out_r}");
    }

    #[test]
    fn zero_lookahead_limits_current_sample() {
        let mut lim = LookaheadLimiter::new(48_000.0);
        lim.set_ceiling_db(-1.0);
        lim.set_lookahead_ms(0.0);

        let (l, r) = lim.process_sample(4.0, -4.0);

        assert!(l.abs() <= 0.892, "left exceeded ceiling: {l}");
        assert!(r.abs() <= 0.892, "right exceeded ceiling: {r}");
    }

    #[test]
    fn silence_stays_silence() {
        let mut lim = LookaheadLimiter::new(48_000.0);
        for _ in 0..500 {
            let (l, r) = lim.process_sample(0.0, 0.0);
            assert_eq!((l, r), (0.0, 0.0));
        }
    }
}
