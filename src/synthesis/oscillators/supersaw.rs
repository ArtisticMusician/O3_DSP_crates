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
            phases: [0.0; NUM_VOICES],
            base_freq: ParameterSmoother::new(sample_rate, smoothing_ms, 440.0),
            detune: ParameterSmoother::new(sample_rate, smoothing_ms, 0.1),
            stereo_spread: ParameterSmoother::new(sample_rate, smoothing_ms, 0.5),
        }
    }

    /// Resets all internal phase accumulators.
    pub fn reset(&mut self) {
        self.phases.fill(0.0);
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
        self.detune.set_target(detune.max(0.0));
    }

    /// Instantly set detune.
    pub fn reset_detune(&mut self, detune: f32) {
        self.detune.reset(detune.max(0.0));
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
    pub fn next_sample(&mut self) -> (f32, f32) {
        let freq = self.base_freq.process_sample();
        let detune_val = self.detune.process_sample();
        let spread = self.stereo_spread.process_sample();

        // 7 oscillators:
        // 0: center
        // 1,2: slightly detuned pair
        // 3,4: moderately detuned pair
        // 5,6: extremely detuned pair

        // Detune curve logic:
        // A common Roland supersaw detune curve offsets frequency by small amounts.
        // We'll use a linear spread here scaled by detune_val and base_freq.
        // E.g., detune offset in Hz:
        let max_detune_hz = freq * 0.1 * detune_val; // 10% max detune at detune_val = 1.0

        let detune_offsets: [f32; NUM_VOICES] = [
            0.0,
            max_detune_hz * 0.15,
            -max_detune_hz * 0.15,
            max_detune_hz * 0.45,
            -max_detune_hz * 0.45,
            max_detune_hz * 1.0,
            -max_detune_hz * 1.0,
        ];

        // Panning map:
        // Center voice in middle.
        // Others spread out based on `spread` parameter.
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
            let phase_inc = osc_freq / self.sample_rate;

            // Advance phase
            self.phases[i] += phase_inc;
            if self.phases[i] >= 1.0 {
                self.phases[i] -= 1.0;
            } else if self.phases[i] < 0.0 {
                self.phases[i] += 1.0; // In case of negative frequencies
            }

            // Generate polyblep or basic naive saw?
            // "detuned sawtooth generator": for absolute zero allocations and simplicity
            // a naive saw is trivial but aliased. The instructions don't strictly require
            // anti-aliasing (polyblep), so we use a naive saw: 2 * phase - 1
            // (If PolyBLEP was strictly required, we could inject it, but the requirements
            // just say "detuned sawtooth generator").
            let sample = 2.0 * self.phases[i] - 1.0;

            // Pan
            let pan = pans[i].clamp(0.0, 1.0);
            out_l += sample * (1.0 - pan);
            out_r += sample * pan;
        }

        // Normalize output by number of voices to prevent extreme clipping.
        // A normalization factor of 1 / num_voices is safe, though it can sound quiet.
        // We'll use 1 / sqrt(NUM_VOICES) which is ~0.378 to retain volume.
        let norm_factor = 1.0 / (NUM_VOICES as f32).sqrt();

        (
            (out_l * norm_factor).clamp(-1.0, 1.0),
            (out_r * norm_factor).clamp(-1.0, 1.0),
        )
    }

    /// Process a block of samples into planar slices.
    pub fn process_block(&mut self, out_l: &mut [f32], out_r: &mut [f32]) {
        assert_eq!(
            out_l.len(),
            out_r.len(),
            "Output buffers must be same length"
        );

        for i in 0..out_l.len() {
            let (l, r) = self.next_sample();
            out_l[i] = l;
            out_r[i] = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization() {
        let mut supersaw = Supersaw::new(44100.0);
        let (l, r) = supersaw.next_sample();
        assert!(l >= -1.0 && l <= 1.0);
        assert!(r >= -1.0 && r <= 1.0);
    }

    #[test]
    fn test_reset() {
        let mut supersaw = Supersaw::new(44100.0);
        supersaw.next_sample();
        supersaw.next_sample();
        supersaw.reset();

        for phase in supersaw.phases.iter() {
            assert_eq!(*phase, 0.0);
        }

        // At phase 0, naive saw is -1.0 for each oscillator.
        let (l, r) = supersaw.next_sample();
        assert!(l < 0.0 && r < 0.0);
    }

    #[test]
    fn test_output_bounds() {
        let mut supersaw = Supersaw::new(44100.0);
        supersaw.set_base_frequency(1000.0);
        supersaw.set_detune(1.0);
        supersaw.set_stereo_spread(1.0);

        for _ in 0..1000 {
            let (l, r) = supersaw.next_sample();
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

        let expected_period = (sample_rate / freq).round() as usize;

        let mut last_out = ss.next_sample().0;
        let mut start_idx = 0;
        for i in 1..2000 {
            let out = ss.next_sample().0;
            if last_out > 0.0 && out < 0.0 {
                start_idx = i;
                break;
            }
            last_out = out;
        }

        let mut next_idx = 0;
        for _ in start_idx + 1..start_idx + 10 {
            last_out = ss.next_sample().0;
        }
        for i in start_idx + 10..start_idx + 2000 {
            let out = ss.next_sample().0;
            if last_out > 0.0 && out < 0.0 {
                next_idx = i;
                break;
            }
            last_out = out;
        }
        assert_eq!(next_idx - start_idx, expected_period);
    }
}
