use crate::synthesis::math_utils::{db_to_linear_approx, linear_to_db_approx};
use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

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

    hold_ms: f32,
    hold_samples: usize,
    hold_counter: usize,

    envelope: f32,
    is_open: bool,

    gain_smoother: ParameterSmoother,
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

            hold_ms: 10.0,
            hold_samples: 0,
            hold_counter: 0,

            envelope: 0.0,
            is_open: false,

            gain_smoother: ParameterSmoother::new(sample_rate, 5.0, 1.0),
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
        self.gain_smoother.reset(1.0);
    }

    pub fn set_threshold_db(&mut self, threshold_db: f32) {
        self.threshold_db = threshold_db.clamp(-144.0, 0.0);
    }

    pub fn set_hysteresis_db(&mut self, hysteresis_db: f32) {
        self.hysteresis_db = hysteresis_db.max(0.0);
    }

    pub fn set_ratio(&mut self, ratio: f32) {
        // Ratio >= 1.0. High ratios approach hard gating.
        self.ratio = ratio.max(1.0);
    }

    pub fn set_attack_ms(&mut self, ms: f32) {
        // Attack/release are applied via the sidechain envelope or gain smoother.
        // We'll use the sidechain envelope for instant tracking, but the gain smoother
        // to handle the actual gain fade-in/fade-out smoothly.
        // So let's make the gain smoother track attack/release.
        self.gain_smoother.set_time(self.sample_rate, ms.max(0.1));

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

        // For the sidechain, we want near instant tracking of the level
        // so that the threshold calculation is accurate to the current signal,
        // and we apply attack/release to the *gain reduction* instead.
        // However, a tiny bit of smoothing on the peak detection is good to prevent jitter.
        // We'll use a fast 1ms smoothing for the peak envelope itself.
        let fast_coef = (-1.0 / (0.001 * self.sample_rate)).exp();
        if peak > self.envelope {
            self.envelope = peak; // Instant attack for sidechain
        } else {
            self.envelope = fast_coef * self.envelope + (1.0 - fast_coef) * peak; // fast release
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

        // Apply attack/release to the gain envelope
        if target_gain_lin > self.gain_smoother.current_value() {
            // Gain is increasing (gate opening), so we use the attack time.
            // Since we removed set_time from the audio loop, we can just manually apply it.
            let cur = self.gain_smoother.current_value();
            let new_val = self.attack_coef * cur + (1.0 - self.attack_coef) * target_gain_lin;
            self.gain_smoother.reset(new_val);
        } else {
            // Gain is decreasing (gate closing/expanding), use release time
            let cur = self.gain_smoother.current_value();
            let new_val = self.release_coef * cur + (1.0 - self.release_coef) * target_gain_lin;
            self.gain_smoother.reset(new_val);
        }

        let gain = self.gain_smoother.current_value();
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
    fn gates_quiet_signal() {
        let mut gate = GateExpander::new(48_000.0);
        gate.set_threshold_db(-10.0);
        gate.set_ratio(100.0); // Hard gate

        // Wait, default is open because gain_smoother is initialized to 1.0.
        // We must process enough samples to let the release completely close the gate
        // Since release is default 100ms and attack is 1ms.
        gate.set_release_ms(1.0); // Make it fast for the test

        // A signal of 0.01 is -40 dB, well below -10 dB threshold
        // Give it 10,000 samples to fully release
        for _ in 0..10_000 {
            gate.process_sample(0.01, 0.01);
        }

        let (l, r) = gate.process_sample(0.01, 0.01);

        // Should be heavily attenuated
        assert!(l.abs() < 0.001);
        assert!(r.abs() < 0.001);
    }
}
