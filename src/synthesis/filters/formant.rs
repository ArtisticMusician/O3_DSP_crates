use crate::synthesis::utils::{ParameterSmoother, Processor};
use std::f32::consts::TAU;

/// Number of formants modelled per vowel (F1, F2, F3).
const MAX_FORMANTS: usize = 3;

/// Formant filter - parallel resonators for vowel/voice synthesis.
///
/// Formants are resonant frequencies that shape the timbre of vocal sounds.
/// This implementation uses a bank of parallel band-pass resonators - each normalised to
/// 0 dB peak gain and scaled by its own output gain - so a set of formant frequencies can
/// be morphed to simulate different vowels and speech sounds.
///
/// Common formant frequencies:
/// - Vowels: F1 (80-1000 Hz), F2 (800-3000 Hz), F3 (2500-3500 Hz)
#[derive(Debug, Clone)]
pub struct FormantFilter {
    // Formant resonators (parallel band-pass filters)
    resonators: Vec<FormantResonator>,
}

/// A single formant resonator (parallel band-pass filter, 0 dB peak gain).
#[derive(Debug, Clone)]
struct FormantResonator {
    sample_rate: f32,

    // Normalised biquad coefficients (a0 == 1)
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,

    // Direct-form-II state variables
    w1: f32,
    w2: f32,

    // Current (smoothed) frequency, bandwidth and gain
    freq: f32,
    bandwidth: f32,
    gain: f32,

    // Smoothed control parameters
    freq_smoother: ParameterSmoother,
    bandwidth_smoother: ParameterSmoother,
    gain_smoother: ParameterSmoother,
}

/// Formant bank for one vowel: `[frequency_hz, bandwidth_hz, gain]` per formant.
struct VowelPreset {
    formants: [[f32; MAX_FORMANTS]; 3],
}

/// Approximate vowel formant presets, in the order the `vowel` control sweeps through.
const VOWELS: [VowelPreset; 5] = [
    // A (as in "father")
    VowelPreset {
        formants: [[800.0, 80.0, 1.0], [1150.0, 90.0, 0.63], [2900.0, 120.0, 0.10]],
    },
    // E (as in "bed")
    VowelPreset {
        formants: [[400.0, 60.0, 1.0], [1600.0, 80.0, 0.45], [2700.0, 120.0, 0.10]],
    },
    // I (as in "see")
    VowelPreset {
        formants: [[350.0, 60.0, 1.0], [1700.0, 90.0, 0.35], [2700.0, 120.0, 0.10]],
    },
    // O (as in "law")
    VowelPreset {
        formants: [[450.0, 70.0, 1.0], [800.0, 80.0, 0.50], [2830.0, 120.0, 0.10]],
    },
    // U (as in "boot")
    VowelPreset {
        formants: [[325.0, 60.0, 1.0], [700.0, 70.0, 0.40], [2530.0, 120.0, 0.10]],
    },
];

impl Default for FormantFilter {
    fn default() -> Self {
        Self::new(48000.0, MAX_FORMANTS) // 3 formants
    }
}

impl FormantFilter {
    /// Create a new formant filter with the specified number of resonators.
    ///
    /// The bank starts tuned to a neutral "A" vowel. The count is clamped to the range
    /// this implementation models (1 to 3 formants).
    pub fn new(sample_rate: f32, num_formants: usize) -> Self {
        let count = num_formants.clamp(1, MAX_FORMANTS);

        let resonators = (0..count)
            .map(|i| {
                let [freq, bandwidth, gain] = VOWELS[0].formants[i];
                FormantResonator::new(sample_rate, freq, bandwidth, gain)
            })
            .collect();

        Self { resonators }
    }

    /// Number of resonators in the bank.
    pub fn num_formants(&self) -> usize {
        self.resonators.len()
    }

    /// Set one formant's frequency (Hz), bandwidth (Hz) and output gain.
    /// Parameters are smoothed, so this is safe to call at control rate.
    pub fn set_formant(&mut self, index: usize, freq_hz: f32, bandwidth_hz: f32, gain: f32) {
        if let Some(resonator) = self.resonators.get_mut(index) {
            resonator.set_parameters(freq_hz, bandwidth_hz, gain);
        }
    }

    /// Morph the whole bank across the vowel sequence A - E - I - O - U.
    ///
    /// `vowel` is a position along that sequence: 0.0 is a pure "A", 1.0 a pure "U",
    /// and values in between interpolate smoothly between the neighbouring vowels.
    pub fn set_vowel(&mut self, vowel: f32) {
        let position = vowel.clamp(0.0, 1.0) * (VOWELS.len() - 1) as f32;
        let lower = position.floor() as usize;
        let upper = (lower + 1).min(VOWELS.len() - 1);
        let t = position - lower as f32;

        for (i, resonator) in self.resonators.iter_mut().enumerate() {
            let a = VOWELS[lower].formants[i];
            let b = VOWELS[upper].formants[i];
            resonator.set_parameters(
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            );
        }
    }

    /// Clear the resonator state, e.g. when starting a new note.
    pub fn reset(&mut self) {
        for resonator in &mut self.resonators {
            resonator.reset();
        }
    }
}

impl Processor for FormantFilter {
    #[inline]
    fn process_sample(&mut self, input: f32) -> f32 {
        let mut output = 0.0;

        for resonator in &mut self.resonators {
            output += resonator.process_sample(input);
        }

        output
    }
}

impl FormantResonator {
    fn new(sample_rate: f32, freq: f32, bandwidth: f32, gain: f32) -> Self {
        let mut resonator = Self {
            sample_rate,
            b0: 0.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            w1: 0.0,
            w2: 0.0,
            freq,
            bandwidth,
            gain: 0.0,
            freq_smoother: ParameterSmoother::new(sample_rate, 25.0, freq),
            bandwidth_smoother: ParameterSmoother::new(sample_rate, 25.0, bandwidth),
            gain_smoother: ParameterSmoother::new(sample_rate, 25.0, gain),
        };
        resonator.update_coefficients();
        resonator
    }

    fn set_parameters(&mut self, freq_hz: f32, bandwidth_hz: f32, gain: f32) {
        let ceiling = self.sample_rate * 0.45;
        self.freq_smoother
            .set_target(freq_hz.clamp(20.0, ceiling));
        self.bandwidth_smoother.set_target(bandwidth_hz.max(10.0));
        self.gain_smoother.set_target(gain.max(0.0));
    }

    /// Recompute the normalised band-pass coefficients (RBJ, 0 dB peak gain).
    #[inline]
    fn update_coefficients(&mut self) {
        let freq = self.freq.clamp(20.0, self.sample_rate * 0.45);
        let bandwidth = self.bandwidth.max(10.0);

        let w0 = TAU * freq / self.sample_rate;
        let q = (freq / bandwidth).max(0.1);
        let alpha = w0.sin() / (2.0 * q);
        let a0 = 1.0 + alpha;

        self.b0 = alpha / a0;
        self.b1 = 0.0;
        self.b2 = -alpha / a0;
        self.a1 = -2.0 * w0.cos() / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        self.freq = self.freq_smoother.process_sample();
        self.bandwidth = self.bandwidth_smoother.process_sample();
        self.gain = self.gain_smoother.process_sample();

        self.update_coefficients();

        // Direct form II transposed
        let output = self.b0 * input + self.w1;
        self.w1 = self.b1 * input - self.a1 * output + self.w2;
        self.w2 = self.b2 * input - self.a2 * output;

        output * self.gain
    }

    fn reset(&mut self) {
        self.w1 = 0.0;
        self.w2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state RMS of a sine at `freq` driven through `filter`.
    fn rms_at(filter: &mut FormantFilter, freq: f32, sample_rate: f32) -> f32 {
        let mut phase = 0.0f32;
        let increment = freq / sample_rate;
        let step = |phase: &mut f32| {
            *phase = (*phase + increment).fract();
            (TAU * *phase).sin()
        };

        // Let the smoothers settle, then measure.
        for _ in 0..(sample_rate as usize / 4) {
            filter.process_sample(step(&mut phase));
        }

        let window = 4096;
        let mut sum = 0.0;
        for _ in 0..window {
            let y = filter.process_sample(step(&mut phase));
            sum += y * y;
        }
        (sum / window as f32).sqrt()
    }

    #[test]
    fn default_bank_has_three_formants() {
        assert_eq!(FormantFilter::default().num_formants(), 3);
        assert_eq!(FormantFilter::new(48_000.0, 0).num_formants(), 1);
        assert_eq!(FormantFilter::new(48_000.0, 99).num_formants(), 3);
    }

    #[test]
    fn output_is_finite() {
        let mut filter = FormantFilter::new(48_000.0, 3);
        for i in 0..10_000 {
            let y = filter.process_sample(if i == 0 { 1.0 } else { 0.0 });
            assert!(y.is_finite(), "non-finite output at sample {i}: {y}");
        }
    }

    #[test]
    fn first_formant_resonates_around_its_centre() {
        let sample_rate = 48_000.0;

        // A pure "A" (F1 = 800 Hz) should pass its own formant far more than sub-bass.
        let mut at_formant = FormantFilter::new(sample_rate, 3);
        let mut below = FormantFilter::new(sample_rate, 3);

        let peak = rms_at(&mut at_formant, 800.0, sample_rate);
        let below_peak = rms_at(&mut below, 50.0, sample_rate);

        assert!(
            peak > below_peak * 4.0,
            "F1 response {peak} should dominate sub-bass {below_peak}"
        );
    }

    #[test]
    fn vowel_morph_moves_the_first_formant() {
        let sample_rate = 48_000.0;

        let mut a = FormantFilter::new(sample_rate, 3);
        a.set_vowel(0.0);
        let a_peak = rms_at(&mut a, 800.0, sample_rate);

        let mut u = FormantFilter::new(sample_rate, 3);
        u.set_vowel(1.0);
        let u_peak = rms_at(&mut u, 800.0, sample_rate);

        assert!(
            a_peak > u_peak,
            "\"A\" should emphasise 800 Hz more than \"U\": {a_peak} vs {u_peak}"
        );
    }
}
