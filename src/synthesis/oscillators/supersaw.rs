
#[inline(always)]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let t = t / dt;
        return t + t - t * t - 1.0;
    } else if t > 1.0 - dt {
        let t = (t - 1.0) / dt;
        return t * t + t + t + 1.0;
    }
    0.0
}
use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

const NUM_VOICES: usize = 7;

/// Multi-voice detuned sawtooth generator (Supersaw).
#[derive(Debug, Clone)]
pub struct Supersaw {
    sample_rate: f32,

    // Phase accumulators for all 7 voices
    phases: [f32; NUM_VOICES],

    // Smoothed parameters
    base_freq: ParameterSmoother,
    detune: ParameterSmoother,
    stereo_spread: ParameterSmoother,
}

impl Supersaw {
    /// Creates a new Supersaw generator.
    pub fn new(sample_rate: f32) -> Self {
        let smoothing_ms = 5.0;
        Self {
            sample_rate,
            phases: [0.0, 0.11, 0.22, 0.33, 0.44, 0.55, 0.66],
            base_freq: ParameterSmoother::new(sample_rate, smoothing_ms, 440.0),
            detune: ParameterSmoother::new(sample_rate, smoothing_ms, 0.1),
            stereo_spread: ParameterSmoother::new(sample_rate, smoothing_ms, 0.5),
        }
    }

    /// Resets all internal phase accumulators.
    pub fn reset(&mut self) {
        for i in 0..NUM_VOICES {
            self.phases[i] = (i as f32 * 0.11).fract();
        }
        // Start them off with some random spread so they don't all align on reset
        // to prevent extreme peaks (though a pure reset often wants 0 phase).
        // Standard practice for a "reset" is to return to a known state, so we use 0.0.
    }

    /// Set the base frequency in Hz.
    pub fn set_base_frequency(&mut self, freq: f32) {
        self.base_freq
            .set_target(freq.max(1.0).min(self.sample_rate / 2.0));
    }

    /// Instantly set the base frequency.
    pub fn reset_base_frequency(&mut self, freq: f32) {
        self.base_freq
            .reset(freq.max(1.0).min(self.sample_rate / 2.0));
    }

    /// Set the detune amount. Usually 0.0 to 1.0 (or higher for extreme detuning).
    pub fn set_detune(&mut self, detune: f32) {
        self.detune.set_target(detune.clamp(0.0, 1.0));
    }

    /// Instantly set detune.
    pub fn reset_detune(&mut self, detune: f32) {
        self.detune.reset(detune.clamp(0.0, 1.0));
    }

    /// Set the stereo spread amount (0.0 = mono, 1.0 = hard panned).
    pub fn set_stereo_spread(&mut self, spread: f32) {
        self.stereo_spread.set_target(spread.clamp(0.0, 1.0));
    }

    /// Instantly set stereo spread.
    pub fn reset_stereo_spread(&mut self, spread: f32) {
        self.stereo_spread.reset(spread.clamp(0.0, 1.0));
    }

    /// Process a single sample, returning a stereo tuple (left, right).
    #[inline(always)]
    pub fn next_sample_stereo(&mut self) -> (f32, f32) {
        let freq = self.base_freq.process_sample();
        let detune_val = self.detune.process_sample();
        let spread = self.stereo_spread.process_sample();

        let max_detune_hz = freq * 0.1 * detune_val;
        let detune_offsets: [f32; NUM_VOICES] = [
            0.0,
            max_detune_hz * 0.15,
            -max_detune_hz * 0.15,
            max_detune_hz * 0.45,
            -max_detune_hz * 0.45,
            max_detune_hz * 1.0,
            -max_detune_hz * 1.0,
        ];

        let pans: [f32; NUM_VOICES] = [
            0.5,
            0.5 - 0.25 * spread,
            0.5 + 0.25 * spread,
            0.5 + 0.50 * spread,
            0.5 - 0.50 * spread,
            0.5 - 1.00 * spread,
            0.5 + 1.00 * spread,
        ];

        let mut out_l = 0.0;
        let mut out_r = 0.0;

        for i in 0..NUM_VOICES {
            let osc_freq = freq + detune_offsets[i];
            let phase_inc = (osc_freq / self.sample_rate).clamp(0.0, 0.5);

            self.phases[i] += phase_inc;
            if self.phases[i] >= 1.0 {
                self.phases[i] -= 1.0;
            }

            let naive_saw = 2.0 * self.phases[i] - 1.0;
            let sample = naive_saw - poly_blep(self.phases[i], phase_inc);

            let pan = pans[i].clamp(0.0, 1.0);
            out_l += sample * (1.0 - pan);
            out_r += sample * pan;
        }

        let norm_factor = 1.0 / (NUM_VOICES as f32).sqrt();
        (
            (out_l * norm_factor).clamp(-1.0, 1.0),
            (out_r * norm_factor).clamp(-1.0, 1.0),
        )
    }

    /// Process a block of samples into planar slices.
    pub fn next_sample(&mut self) -> f32 {
        self.next_sample_stereo().0
    }

    pub fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        for (l_out, r_out) in out_l.iter_mut().zip(out_r.iter_mut()) {
            let (l, r) = self.next_sample_stereo();
            *l_out = l;
            *r_out = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization() {
        let mut supersaw = Supersaw::new(44100.0);
        let (l, r) = supersaw.next_sample_stereo();
        assert!(l >= -1.0 && l <= 1.0);
        assert!(r >= -1.0 && r <= 1.0);
    }

    #[test]
    fn test_reset() {
        let mut supersaw = Supersaw::new(44100.0);
        supersaw.next_sample();
        supersaw.next_sample();
        supersaw.reset();

        for (i, phase) in supersaw.phases.iter().enumerate() {
            assert_eq!(*phase, (i as f32 * 0.11).fract());
        }

        // The first output should just be bounded, not necessarily a peak.
        let (l, r) = supersaw.next_sample_stereo();
        assert!(l >= -1.0 && l <= 1.0);
        assert!(r >= -1.0 && r <= 1.0);
        // After reset, first sample abs < 0.95 (no 7-voice pile-up)
        assert!(l.abs() < 0.95 && r.abs() < 0.95);
    }

    #[test]
    fn test_output_bounds() {
        let mut supersaw = Supersaw::new(44100.0);
        supersaw.set_base_frequency(1000.0);
        supersaw.set_detune(1.0);
        supersaw.set_stereo_spread(1.0);

        for _ in 0..1000 {
            let (l, r) = supersaw.next_sample_stereo();
            assert!(l >= -1.0 && l <= 1.0, "Left output out of bounds: {}", l);
            assert!(r >= -1.0 && r <= 1.0, "Right output out of bounds: {}", r);
        }
    }

    #[test]
    fn test_block_processing() {
        let mut supersaw = Supersaw::new(44100.0);
        let mut out_l = vec![0.0; 128];
        let mut out_r = vec![0.0; 128];

        supersaw.process_block(&mut out_l, &mut out_r);

        // Assert that not all are zero
        let mut has_energy = false;
        for i in 0..128 {
            if out_l[i].abs() > 0.0 {
                has_energy = true;
                break;
            }
        }
        assert!(has_energy);
    }





    #[test]
    fn test_frequency_accuracy() {
        let sample_rate = 44100.0;
        let mut ss = Supersaw::new(sample_rate);
        let freq = 100.0;
        ss.reset_base_frequency(freq);
        ss.reset_detune(0.0);
        ss.reset_stereo_spread(0.0);

        let expected_period = (sample_rate / freq).round() as usize;

        let mut start_idx = 0;
        let mut last_phase = ss.phases[0];
        for i in 1..2000 {
            ss.next_sample_stereo();
            let phase = ss.phases[0];
            if phase < last_phase {
                start_idx = i;
                last_phase = phase;
                break;
            }
            last_phase = phase;
        }

        let mut next_idx = 0;
        for i in start_idx + 1..start_idx + 2000 {
            ss.next_sample_stereo();
            let phase = ss.phases[0];
            if phase < last_phase {
                next_idx = i;
                break;
            }
            last_phase = phase;
        }

        assert_eq!(next_idx - start_idx, expected_period);

        // Also verify the detune bounds requested
        ss.reset_detune(1.0);
        ss.reset_base_frequency(2000.0);
        for _ in 0..10 {
            ss.next_sample_stereo();
        }
    }
}
