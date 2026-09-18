use crate::synthesis::math_utils::{db_to_linear_approx, linear_to_db_approx};

/// Downward expander / noise gate featuring adjustable threshold, ratio,
/// hysteresis to prevent chatter, attack, hold, and release envelopes.
#[derive(Clone, Debug)]
pub struct GateExpander {
    sample_rate: f32,
    threshold_db: f32,
    hysteresis_db: f32,
    ratio: f32,

    attack_coef: f32,
    release_coef: f32,
    fast_coef: f32,

    hold_ms: f32,
    hold_samples: usize,
    hold_counter: usize,

    envelope: f32,
    is_open: bool,

    gain: f32,
}

impl GateExpander {
    pub fn new(sample_rate: f32) -> Self {
        let mut gate = Self {
            sample_rate,
            threshold_db: -60.0,
            hysteresis_db: 3.0,
            ratio: 10.0, // 10:1 ratio acts close to a gate, 1:1 is no effect

            attack_coef: 0.0,
            release_coef: 0.0,
            fast_coef: (-1.0 / (0.001 * sample_rate)).exp(),

            hold_ms: 10.0,
            hold_samples: 0,
            hold_counter: 0,

            envelope: 0.0,
            is_open: false,

            gain: 0.0,
        };
        gate.set_attack_ms(1.0);
        gate.set_release_ms(100.0);
        gate.set_hold_ms(10.0);
        gate
    }

    pub fn reset(&mut self) {
        self.envelope = 0.0;
        self.is_open = false;
        self.hold_counter = 0;
        self.gain = 0.0;
    }

    pub fn set_threshold_db(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db.clamp(-144.0, 0.0);
    }

    pub fn set_hysteresis_db(&mut self, hysteresis_db: f32) {
        self.hysteresis_db = hysteresis_db.max(0.0);
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        self.ratio = ratio.clamp(1.0, 100.0);
    }

    pub fn set_attack_ms(&mut self, ms: f32) {
        let time_sec = (ms * 0.001).max(1e-4);
        self.attack_coef = (-1.0 / (time_sec * self.sample_rate)).exp();
    }

    pub fn set_release_ms(&mut self, ms: f32) {
        let time_sec = (ms * 0.001).max(1e-4);
        self.release_coef = (-1.0 / (time_sec * self.sample_rate)).exp();
    }

    pub fn set_hold_ms(&mut self, ms: f32) {
        self.hold_ms = ms.max(0.0);
        self.hold_samples = (self.hold_ms * 0.001 * self.sample_rate).ceil() as usize;
    }

    #[inline(always)]
    pub fn process_sample(&mut self, left: f32, right: f32) -> (f32, f32) {
        let peak = left.abs().max(right.abs());

        if peak > self.envelope {
            self.envelope = peak;
        } else {
            self.envelope = self.fast_coef * self.envelope + (1.0 - self.fast_coef) * peak;
        }

        let envelope_db = linear_to_db_approx(self.envelope);

        let open_threshold = self.threshold_db;
        let close_threshold = self.threshold_db - self.hysteresis_db;

        // Hysteresis logic
        if self.is_open {
            if envelope_db < close_threshold {
                if self.hold_counter >= self.hold_samples {
                    self.is_open = false;
                } else {
                    self.hold_counter += 1;
                }
            } else {
                self.hold_counter = 0; // Reset hold if we go back above close threshold
            }
        } else {
            if envelope_db >= open_threshold {
                self.is_open = true;
                self.hold_counter = 0;
            }
        }

        // Calculate target gain
        let target_gain_db = if self.is_open {
            0.0
        } else {
            // We are below the close threshold (or hasn't opened yet)
            // Downward expansion calculation
            if envelope_db < open_threshold {
                let diff = open_threshold - envelope_db;
                let gr_db = diff * (1.0 - self.ratio); // Negative value for gain reduction
                gr_db
            } else {
                0.0
            }
        };

        let target_gain_lin = db_to_linear_approx(target_gain_db);

        if target_gain_lin > self.gain {
            self.gain = self.attack_coef * self.gain + (1.0 - self.attack_coef) * target_gain_lin;
        } else {
            self.gain = self.release_coef * self.gain + (1.0 - self.release_coef) * target_gain_lin;
        }

        let gain = self.gain;
        (left * gain, right * gain)
    }

    #[inline(always)]
    pub fn process(&mut self, sample: f32) -> f32 {
        self.process_sample(sample, sample).0
    }

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
    use super::GateExpander;

    #[test]
    fn allows_loud_signal() {
        let mut gate = GateExpander::new(48_000.0);
        gate.set_threshold_db(-10.0);
        gate.set_ratio(10.0);

        // Initial state is closed, let's open it
        // A signal of 1.0 is 0 dB, which is > -10.0 dB
        for _ in 0..1000 {
            gate.process_sample(1.0, 1.0);
        }

        let (l, r) = gate.process_sample(1.0, 1.0);
        assert!((l - 1.0).abs() < 1e-2);
        assert!((r - 1.0).abs() < 1e-2);
    }

    #[test]
    fn starts_closed() {
        let mut gate = GateExpander::new(48_000.0);
        gate.set_threshold_db(-10.0);
        gate.set_ratio(100.0);

        let (l, r) = gate.process_sample(0.01, 0.01);
        assert!(l.abs() < 0.001);
        assert!(r.abs() < 0.001);
    }

    #[test]
    fn gates_quiet_signal() {
        let mut gate = GateExpander::new(48_000.0);
        gate.set_threshold_db(-10.0);
        gate.set_ratio(100.0);

        for _ in 0..1000 {
            gate.process_sample(0.01, 0.01);
        }

        let (l, r) = gate.process_sample(0.01, 0.01);
        assert!(l.abs() < 0.001);
        assert!(r.abs() < 0.001);
    }

    #[test]
    fn ratio_1_is_unity() {
        let mut gate = GateExpander::new(48_000.0);
        gate.set_threshold_db(-10.0);
        gate.set_ratio(1.0);

        for _ in 0..1000 {
            gate.process_sample(0.01, 0.01);
        }

        let (l, r) = gate.process_sample(0.01, 0.01);
        assert!((l - 0.01).abs() < 1e-4);
        assert!((r - 0.01).abs() < 1e-4);
    }

    #[test]
    fn hysteresis() {
        let mut gate = GateExpander::new(48_000.0);
        gate.set_threshold_db(-10.0);
        gate.set_hysteresis_db(5.0);
        gate.set_hold_ms(10.0);
        gate.set_ratio(100.0);

        // Open gate fully
        for _ in 0..1000 {
            gate.process_sample(1.0, 1.0);
        }

        // Drop into hysteresis region (-12dB is between -10 and -15)
        let signal = 0.25; // approx -12dB
        for _ in 0..100 {
            gate.process_sample(signal, signal);
        }

        let (l, _r) = gate.process_sample(signal, signal);
        assert!((l - signal).abs() < 1e-2, "Gate should still be fully open in hysteresis hold region");
    }
}
