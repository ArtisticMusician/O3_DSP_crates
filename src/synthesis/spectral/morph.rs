use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

use super::spectrum::{HarmonicSpectrum, MAX_PARTIALS};

/// Two-target spectral morph (Spectra Morph Target 1 / 2).
/// Tick `process` at control or audio rate; it only writes N floats.
#[derive(Debug, Clone)]
pub struct SpectrumMorph<const N: usize = MAX_PARTIALS> {
    a: HarmonicSpectrum<N>,
    b: HarmonicSpectrum<N>,
    amount: ParameterSmoother,
}

impl<const N: usize> SpectrumMorph<N> {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            a: HarmonicSpectrum::sine(),
            b: HarmonicSpectrum::saw(),
            amount: ParameterSmoother::new(sample_rate, 10.0, 0.0),
        }
    }

    pub fn set_target_a(&mut self, spectrum: HarmonicSpectrum<N>) {
        self.a = spectrum;
    }

    pub fn set_target_b(&mut self, spectrum: HarmonicSpectrum<N>) {
        self.b = spectrum;
    }

    pub fn set_amount(&mut self, amount: f32) {
        self.amount.set_target(amount.clamp(0.0, 1.0));
    }

    pub fn reset_amount(&mut self, amount: f32) {
        self.amount.reset(amount.clamp(0.0, 1.0));
    }

    pub fn target_a(&self) -> &HarmonicSpectrum<N> {
        &self.a
    }

    pub fn target_b(&self) -> &HarmonicSpectrum<N> {
        &self.b
    }

    /// Advance the amount smoother and write the blended spectrum into `dest`.
    #[inline]
    pub fn process_into(&mut self, dest: &mut HarmonicSpectrum<N>) {
        let t = self.amount.process_sample();
        HarmonicSpectrum::lerp_into(&self.a, &self.b, t, dest);
    }

    pub fn process(&mut self) -> HarmonicSpectrum<N> {
        let t = self.amount.process_sample();
        HarmonicSpectrum::lerp(&self.a, &self.b, t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amount_zero_is_target_a() {
        let mut morph = SpectrumMorph::<8>::new(44100.0);
        morph.set_target_a(HarmonicSpectrum::sine());
        morph.set_target_b(HarmonicSpectrum::saw());
        morph.reset_amount(0.0);
        let out = morph.process();
        assert!((out.partial(0) - 1.0).abs() < 1e-5);
        assert_eq!(out.partial(1), 0.0);
    }

    #[test]
    fn amount_one_is_target_b() {
        let mut morph = SpectrumMorph::<8>::new(44100.0);
        let saw = HarmonicSpectrum::<8>::saw();
        morph.set_target_a(HarmonicSpectrum::sine());
        morph.set_target_b(saw);
        morph.reset_amount(1.0);
        let out = morph.process();
        assert!((out.partial(1) - saw.partial(1)).abs() < 1e-6);
    }
}
