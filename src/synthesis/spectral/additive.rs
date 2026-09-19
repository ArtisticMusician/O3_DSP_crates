use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

use super::spectrum::{HarmonicSpectrum, MAX_PARTIALS};

const SINE_LUT_LEN: usize = 2048;
const SINE_LUT_MASK: usize = SINE_LUT_LEN - 1;

fn make_sine_lut() -> [f32; SINE_LUT_LEN] {
    let mut lut = [0.0f32; SINE_LUT_LEN];
    let scale = std::f32::consts::TAU / SINE_LUT_LEN as f32;
    for (i, slot) in lut.iter_mut().enumerate() {
        *slot = (i as f32 * scale).sin();
    }
    lut
}

#[inline(always)]
fn lut_sine(lut: &[f32; SINE_LUT_LEN], phase: f32) -> f32 {
    let pos = phase * SINE_LUT_LEN as f32;
    let i0 = pos as usize;
    let frac = pos - i0 as f32;
    let s0 = lut[i0 & SINE_LUT_MASK];
    let s1 = lut[(i0 + 1) & SINE_LUT_MASK];
    s0 + (s1 - s0) * frac
}

/// Bank of band-limited partials driven by a `HarmonicSpectrum`.
/// Two of these are Generator 1 / Generator 2.
#[derive(Debug, Clone)]
pub struct AdditiveOscillator<const N: usize = MAX_PARTIALS> {
    sample_rate: f32,
    nyquist: f32,
    phases: [f32; N],
    spectrum: HarmonicSpectrum<N>,
    sine_lut: [f32; SINE_LUT_LEN],
    freq: ParameterSmoother,
    stretch: ParameterSmoother,
    level: ParameterSmoother,
}

impl<const N: usize> AdditiveOscillator<N> {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(1.0);
        Self {
            sample_rate: sr,
            nyquist: sr * 0.49,
            phases: [0.0; N],
            spectrum: HarmonicSpectrum::saw(),
            sine_lut: make_sine_lut(),
            freq: ParameterSmoother::new(sr, 5.0, 220.0),
            stretch: ParameterSmoother::new(sr, 10.0, 1.0),
            level: ParameterSmoother::new(sr, 10.0, 0.8),
        }
    }

    pub fn reset(&mut self) {
        self.phases.fill(0.0);
    }

    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq.set_target(freq_hz.clamp(0.1, self.nyquist));
    }

    pub fn reset_frequency(&mut self, freq_hz: f32) {
        self.freq.reset(freq_hz.clamp(0.1, self.nyquist));
    }

    /// 1.0 = harmonic series. Values above/below stretch partial spacing.
    pub fn set_stretch(&mut self, stretch: f32) {
        self.stretch.set_target(stretch.clamp(0.25, 4.0));
    }

    pub fn set_level(&mut self, level: f32) {
        self.level.set_target(level.clamp(0.0, 1.0));
    }

    pub fn set_spectrum(&mut self, spectrum: HarmonicSpectrum<N>) {
        self.spectrum = spectrum;
    }

    pub fn spectrum(&self) -> &HarmonicSpectrum<N> {
        &self.spectrum
    }

    pub fn spectrum_mut(&mut self) -> &mut HarmonicSpectrum<N> {
        &mut self.spectrum
    }

    #[inline(always)]
    pub fn next_sample(&mut self) -> f32 {
        let f0 = self.freq.process_sample();
        let stretch = self.stretch.process_sample();
        let level = self.level.process_sample();
        let amps = self.spectrum.amps();

        let mut mix = 0.0;
        for k in 0..N {
            let amp = amps[k];
            if amp.abs() < 1e-6 {
                continue;
            }
            let partial_hz = f0 * (k as f32 + 1.0) * stretch;
            if partial_hz >= self.nyquist {
                continue;
            }
            let inc = partial_hz / self.sample_rate;
            let phase = self.phases[k] + inc;
            let phase = if phase >= 1.0 { phase - 1.0 } else { phase };
            self.phases[k] = phase;
            mix += amp * lut_sine(&self.sine_lut, phase);
        }

        mix * level
    }

    #[inline]
    pub fn process_block(&mut self, output: &mut [f32]) {
        for s in output.iter_mut() {
            *s = self.next_sample();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_is_finite_and_bounded() {
        let mut osc = AdditiveOscillator::<64>::new(44100.0);
        osc.set_spectrum(HarmonicSpectrum::sine());
        osc.reset_frequency(220.0);
        osc.set_level(1.0);
        for _ in 0..2048 {
            let y = osc.next_sample();
            assert!(y.is_finite());
            assert!(y.abs() <= 1.05);
        }
    }

    #[test]
    fn saw_has_energy() {
        let mut osc = AdditiveOscillator::<64>::new(44100.0);
        osc.set_spectrum(HarmonicSpectrum::saw());
        osc.reset_frequency(110.0);
        let mut peak = 0.0f32;
        for _ in 0..2048 {
            peak = peak.max(osc.next_sample().abs());
        }
        assert!(peak > 0.05);
    }

    #[test]
    fn high_frequency_does_not_alias_into_nan() {
        let mut osc = AdditiveOscillator::<64>::new(44100.0);
        osc.set_spectrum(HarmonicSpectrum::saw());
        osc.reset_frequency(8000.0);
        for _ in 0..512 {
            assert!(osc.next_sample().is_finite());
        }
    }

    #[test]
    fn reset_returns_to_zero_crossing() {
        let mut osc = AdditiveOscillator::<8>::new(44100.0);
        osc.set_spectrum(HarmonicSpectrum::sine());
        osc.reset_frequency(100.0);
        for _ in 0..50 {
            osc.next_sample();
        }
        osc.reset();
        let y = osc.next_sample();
        assert!(y.abs() < 0.05);
    }
}
