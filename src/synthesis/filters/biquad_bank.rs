use crate::synthesis::filters::biquad::{BiquadFilter, BiquadType};
use crate::synthesis::utils::Processor;

/// A bank of peaking biquads in series - a parametric EQ.
///
/// Peaking bands are cascaded rather than summed: each band applies its own boost/cut and passes
/// everything else through, so the cascade's response is the sum of the individual curves, in dB.
/// (Summing them in parallel would sum N copies of the signal and is not what an EQ is.)
#[derive(Debug, Clone)]
pub struct BiquadBank {
    sample_rate: f32,
    bands: Vec<Band>,
}

#[derive(Debug, Clone, Copy)]
struct Band {
    filter: BiquadFilter,
    freq: f32,
    q: f32,
    gain_db: f32,
}

impl BiquadBank {
    /// Create a bank of `num_bands` peaking bands, spaced evenly across the audible range at
    /// 0 dB (i.e. flat).
    pub fn new(sample_rate: f32, num_bands: usize) -> Self {
        let mut bank = Self {
            sample_rate,
            bands: Vec::with_capacity(num_bands),
        };

        for index in 0..num_bands {
            let freq = default_band_frequency(sample_rate, index, num_bands);
            let mut filter = BiquadFilter::new(sample_rate, BiquadType::Peaking);
            filter.set_parameters(freq, DEFAULT_Q, 0.0);

            bank.bands.push(Band {
                filter,
                freq,
                q: DEFAULT_Q,
                gain_db: 0.0,
            });
        }

        bank
    }

    /// Number of bands.
    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }

    /// Set one band's centre frequency, Q and gain.
    pub fn set_band(&mut self, index: usize, freq_hz: f32, q: f32, gain_db: f32) {
        if let Some(band) = self.bands.get_mut(index) {
            band.freq = freq_hz.clamp(10.0, self.sample_rate * 0.45);
            band.q = q.clamp(0.1, 20.0);
            band.gain_db = gain_db.clamp(-24.0, 24.0);
            band.filter.set_parameters(band.freq, band.q, band.gain_db);
        }
    }

    /// Set one band's gain, keeping its frequency and Q.
    pub fn set_band_gain(&mut self, index: usize, gain_db: f32) {
        if let Some(band) = self.bands.get(index) {
            let (freq, q) = (band.freq, band.q);
            self.set_band(index, freq, q, gain_db);
        }
    }

    /// One band's centre frequency.
    pub fn band_frequency(&self, index: usize) -> Option<f32> {
        self.bands.get(index).map(|band| band.freq)
    }

    /// One band's current gain in dB.
    pub fn band_gain_db(&self, index: usize) -> Option<f32> {
        self.bands.get(index).map(|band| band.gain_db)
    }

    /// Flatten every band to 0 dB.
    pub fn reset_gains(&mut self) {
        for index in 0..self.bands.len() {
            self.set_band_gain(index, 0.0);
        }
    }

    /// Clear the filter state.
    pub fn reset(&mut self) {
        for band in &mut self.bands {
            band.filter.reset();
        }
    }

    /// Process a single sample through every band in series.
    #[inline(always)]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let mut output = input;
        for band in &mut self.bands {
            output = band.filter.process_sample(output);
        }
        output
    }
}

/// A graphic EQ: a [`BiquadBank`] with its bands spaced evenly across the audible range.
#[derive(Debug, Clone)]
pub struct GraphicEq {
    bank: BiquadBank,
}

impl GraphicEq {
    /// Create a graphic EQ with `band_count` bands, logarithmically spaced from
    /// [`MIN_FREQUENCY`] up to a little under Nyquist.
    pub fn new(sample_rate: f32, band_count: usize) -> Self {
        Self {
            bank: BiquadBank::new(sample_rate, band_count),
        }
    }

    pub fn num_bands(&self) -> usize {
        self.bank.num_bands()
    }

    pub fn band_frequency(&self, index: usize) -> Option<f32> {
        self.bank.band_frequency(index)
    }

    pub fn set_band_gain(&mut self, index: usize, gain_db: f32) {
        self.bank.set_band_gain(index, gain_db);
    }

    pub fn band_gain_db(&self, index: usize) -> Option<f32> {
        self.bank.band_gain_db(index)
    }

    pub fn reset_gains(&mut self) {
        self.bank.reset_gains();
    }

    pub fn reset(&mut self) {
        self.bank.reset();
    }

    #[inline(always)]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.bank.process_sample(input)
    }
}

/// Lowest band centre.
pub const MIN_FREQUENCY: f32 = 20.0;

/// Default Q for an octave-ish band; wide enough that neighbouring bands overlap smoothly.
const DEFAULT_Q: f32 = 1.41;

/// Evenly space `index` of `count` bands across the usable range, on a log scale.
fn default_band_frequency(sample_rate: f32, index: usize, count: usize) -> f32 {
    let highest = (sample_rate * 0.45).max(MIN_FREQUENCY * 2.0);
    if count <= 1 {
        return (MIN_FREQUENCY * highest).sqrt();
    }

    let span = (highest / MIN_FREQUENCY).log2();
    let position = index as f32 / (count - 1) as f32;
    MIN_FREQUENCY * 2.0f32.powf(span * position)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// Steady-state RMS of a unit-amplitude sine at `freq` through `bank`.
    fn sine_rms(bank: &mut BiquadBank, freq: f32, sample_rate: f32) -> f32 {
        let increment = freq / sample_rate;
        let mut phase = 0.0f32;

        for _ in 0..(sample_rate as usize / 4) {
            phase = (phase + increment).fract();
            bank.process_sample((TAU * phase).sin());
        }

        let window = 8192;
        let mut energy = 0.0;
        for _ in 0..window {
            phase = (phase + increment).fract();
            let y = bank.process_sample((TAU * phase).sin());
            energy += y * y;
        }
        (energy / window as f32).sqrt()
    }

    /// A unit sine has an RMS of 1/sqrt(2).
    fn reference_rms() -> f32 {
        0.5f32.sqrt()
    }

    #[test]
    fn all_bands_at_zero_db_are_flat() {
        let sample_rate = 48_000.0;
        let mut bank = BiquadBank::new(sample_rate, 8);
        assert_eq!(bank.num_bands(), 8);

        for freq in [50.0, 200.0, 1_000.0, 4_000.0, 12_000.0] {
            let rms = sine_rms(&mut bank, freq, sample_rate);
            assert!(
                (rms - reference_rms()).abs() < 0.01,
                "flat bank changed {freq} Hz: rms {rms}"
            );
        }
    }

    #[test]
    fn boosting_one_band_lifts_its_own_frequency_most() {
        let sample_rate = 48_000.0;
        let mut bank = BiquadBank::new(sample_rate, 8);

        // Boost the band nearest 1 kHz.
        let target = (0..bank.num_bands())
            .min_by(|a, b| {
                let fa = (bank.band_frequency(*a).unwrap() - 1_000.0).abs();
                let fb = (bank.band_frequency(*b).unwrap() - 1_000.0).abs();
                fa.partial_cmp(&fb).unwrap()
            })
            .unwrap();
        let centre = bank.band_frequency(target).unwrap();
        bank.set_band_gain(target, 12.0);

        let at_centre = sine_rms(&mut bank, centre, sample_rate);
        let far_away = sine_rms(&mut bank, 60.0, sample_rate);

        assert!(
            at_centre > reference_rms() * 2.0,
            "+12 dB band should lift its own frequency: {at_centre}"
        );
        assert!(
            far_away < at_centre * 0.5,
            "a band 4+ octaves away should barely move: {far_away} vs {at_centre}"
        );
    }

    #[test]
    fn cutting_one_band_reduces_its_own_frequency() {
        let sample_rate = 48_000.0;
        let mut bank = BiquadBank::new(sample_rate, 5);
        let centre = bank.band_frequency(2).unwrap();
        bank.set_band_gain(2, -12.0);

        let rms = sine_rms(&mut bank, centre, sample_rate);
        assert!(rms < reference_rms() * 0.6, "-12 dB band should cut: {rms}");
    }

    #[test]
    fn empty_bank_is_a_passthrough() {
        let mut bank = BiquadBank::new(48_000.0, 0);
        assert_eq!(bank.num_bands(), 0);

        for i in 0..512 {
            let x = (i as f32 * 0.01).sin();
            assert_eq!(bank.process_sample(x), x);
        }
    }

    #[test]
    fn extreme_settings_stay_finite() {
        let sample_rate = 48_000.0;
        let mut bank = BiquadBank::new(sample_rate, 6);

        for index in 0..bank.num_bands() {
            bank.set_band(index, 20.0 + index as f32 * 8_000.0, 20.0, 24.0);
        }

        for i in 0..20_000 {
            let y = bank.process_sample((i as f32 * 0.03).sin());
            assert!(y.is_finite(), "non-finite output {y}");
        }
    }

    #[test]
    fn graphic_eq_bands_are_log_spaced_and_ordered() {
        let eq = GraphicEq::new(48_000.0, 9);
        assert_eq!(eq.num_bands(), 9);

        let mut previous = 0.0;
        for index in 0..eq.num_bands() {
            let freq = eq.band_frequency(index).unwrap();
            assert!(freq > previous, "bands must ascend: {previous} -> {freq}");
            assert!(freq <= 48_000.0 * 0.45, "band {index} sits past the usable range");
            previous = freq;
        }
    }

    #[test]
    fn graphic_eq_at_zero_db_passes_audio_through() {
        let sample_rate = 48_000.0;
        let mut eq = GraphicEq::new(sample_rate, 5);

        let rms = |eq: &mut GraphicEq, freq: f32| {
            let increment = freq / sample_rate;
            let mut phase = 0.0f32;
            for _ in 0..(sample_rate as usize / 4) {
                phase = (phase + increment).fract();
                eq.process_sample((TAU * phase).sin());
            }
            let window = 8192;
            let mut energy = 0.0;
            for _ in 0..window {
                phase = (phase + increment).fract();
                let y = eq.process_sample((TAU * phase).sin());
                energy += y * y;
            }
            (energy / window as f32).sqrt()
        };

        let measured = rms(&mut eq, 440.0);
        assert!((measured - reference_rms()).abs() < 0.01, "graphic eq coloured the signal: {measured}");
    }
}
