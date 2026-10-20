pub mod fm_operator;
pub mod granular;
pub mod karplus_strong;
pub mod noise;
pub mod polyblep;
pub mod phase_distortion;
pub mod physical_modeling;
pub mod sine_bank;
pub mod supersaw;
pub mod vector;
pub mod wavetable;

pub use fm_operator::FmOperator;
pub use granular::GranularOscillator;
pub use karplus_strong::KarplusStrong;
pub use noise::{NoiseGenerator, NoiseType};
pub use polyblep::{TableOscillator, Waveform};
pub use phase_distortion::PhaseDistortionOscillator;
pub use physical_modeling::MassSpring;
pub use sine_bank::SineBank;
pub use supersaw::Supersaw;
pub use vector::VectorSynthesis;
pub use wavetable::WavetableOscillator;

use crate::synthesis::spectral::AdditiveOscillator;

#[derive(Debug, Clone)]
pub enum OscillatorModule {
    PolyBlep(TableOscillator),
    Noise(NoiseGenerator),
    Wavetable(WavetableOscillator),
    Fm(FmOperator),
    Granular(GranularOscillator),
    PhaseDistortion(PhaseDistortionOscillator),
    Vector(VectorSynthesis),
    PhysicalModeling(MassSpring),
    KarplusStrong(KarplusStrong),
    Supersaw(Supersaw),
    Additive(AdditiveOscillator),
}

impl OscillatorModule {
    #[inline(always)]
    pub fn next_sample(&mut self, wave_type: Waveform, noise_type: NoiseType) -> f32 {
        match self {
            OscillatorModule::PolyBlep(osc) => osc.next_sample(wave_type),
            OscillatorModule::Noise(osc) => osc.next_sample(noise_type),
            OscillatorModule::Wavetable(osc) => osc.process_sample(),
            OscillatorModule::Fm(osc) => osc.next_sample(0.0), // default empty pm
            OscillatorModule::KarplusStrong(osc) => osc.next_sample(),
            OscillatorModule::Supersaw(osc) => osc.next_sample(), // default mono out
            OscillatorModule::Granular(osc) => osc.process_sample(0.0),
            OscillatorModule::PhaseDistortion(osc) => osc.process_sample(0.0),
            OscillatorModule::Vector(osc) => osc.process_sample(0.0),
            OscillatorModule::PhysicalModeling(osc) => osc.process_sample(0.0),
            OscillatorModule::Additive(osc) => osc.next_sample(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_additive_variant_dispatches_to_its_oscillator() {
        let mut oscillator = AdditiveOscillator::new(48_000.0);
        oscillator.set_frequency(440.0);
        oscillator.reset_frequency(440.0);
        let mut module = OscillatorModule::Additive(oscillator);

        let output: Vec<f32> = (0..4_800)
            .map(|_| module.next_sample(Waveform::Sine, NoiseType::White))
            .collect();
        let rms = (output.iter().map(|s| s * s).sum::<f32>() / output.len() as f32).sqrt();

        assert!(rms > 0.05, "the additive arm produced silence, RMS {rms}");
        assert!(output.iter().all(|s| s.is_finite()), "additive output went non-finite");
    }
}
