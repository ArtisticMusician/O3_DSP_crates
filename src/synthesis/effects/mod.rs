pub mod bitcrusher;
pub mod chorus;
pub mod chorus_flanger;
pub mod delay;
pub mod phase_shifter;
pub mod reverb_fdn;
pub mod ring_modulator;
pub mod saturation;
pub mod schroeder_reverb;
pub mod stereo_width;
pub mod wavefolder;

pub use bitcrusher::Bitcrusher;
pub use chorus::Chorus;
pub use chorus_flanger::ChorusFlanger;
pub use delay::MonoFeedbackDelay;
pub use phase_shifter::{Phaser, PhaserModulation};
pub use reverb_fdn::ReverbFdn;
pub use ring_modulator::{CarrierShape, RingModulator};
pub use saturation::{Saturation, SaturationMode};
pub use schroeder_reverb::SchroederReverb;
pub use stereo_width::StereoWidth;
pub use wavefolder::{Wavefolder, WavefolderType};

use crate::synthesis::utils::Processor;

/// One mono effect, selected at runtime.
///
/// The same role `FilterModule` and `OscillatorModule` play for their families: one type the
/// routing code can hold without knowing which effect it is. Every effect here takes and returns a
/// single sample, which is what lets two of them sit behind a stereo stage.
///
/// The stereo-only effects (`ReverbFdn`, `ChorusFlanger`, `StereoWidth`) are not variants here
/// because they need both channels at once; they have [`StereoEffectModule`].
#[derive(Debug, Clone)]
pub enum EffectModule {
    Bitcrusher(Bitcrusher),
    Chorus(Chorus),
    Delay(MonoFeedbackDelay),
    Phaser(Phaser),
    RingModulator(RingModulator),
    Saturation(Saturation),
    SchroederReverb(SchroederReverb),
    Wavefolder(Wavefolder),
}

impl Processor for EffectModule {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        match self {
            EffectModule::Bitcrusher(effect) => effect.process(sample),
            EffectModule::Chorus(effect) => effect.process_sample(sample),
            EffectModule::Delay(effect) => effect.process_sample(sample),
            EffectModule::Phaser(effect) => effect.process_sample(sample),
            EffectModule::RingModulator(effect) => effect.process(sample),
            EffectModule::Saturation(effect) => effect.process_sample(sample),
            EffectModule::SchroederReverb(effect) => effect.process_sample(sample),
            EffectModule::Wavefolder(effect) => effect.process_sample(sample),
        }
    }
}

/// One effect that only exists as a stereo pair.
#[derive(Debug, Clone)]
pub enum StereoEffectModule {
    ChorusFlanger(ChorusFlanger),
    ReverbFdn(ReverbFdn),
    StereoWidth(StereoWidth),
}

impl StereoEffectModule {
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        match self {
            StereoEffectModule::ChorusFlanger(effect) => effect.process(left, right),
            StereoEffectModule::ReverbFdn(effect) => effect.process(left, right),
            StereoEffectModule::StereoWidth(effect) => effect.process_stereo(left, right),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(buffer: &[f32]) -> f32 {
        (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32).sqrt()
    }

    fn drive(effect: &mut EffectModule, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|index| {
                let t = index as f32 / 48_000.0;
                effect.process_sample((std::f32::consts::TAU * 220.0 * t).sin() * 0.5)
            })
            .collect()
    }

    #[test]
    fn every_mono_variant_passes_audio_without_producing_silence_or_nan() {
        let modules = || {
            vec![
                EffectModule::Bitcrusher(Bitcrusher::new(48_000.0)),
                EffectModule::Chorus(Chorus::new(48_000.0)),
                EffectModule::Delay(MonoFeedbackDelay::new(48_000.0, 250.0)),
                EffectModule::Phaser(Phaser::new(48_000.0, 4)),
                EffectModule::RingModulator(RingModulator::new(48_000.0)),
                EffectModule::Saturation(Saturation::new()),
                EffectModule::SchroederReverb(SchroederReverb::new(48_000.0)),
                EffectModule::Wavefolder(Wavefolder::new(48_000.0, 2.0, WavefolderType::Sine)),
            ]
        };

        for mut effect in modules() {
            let output = drive(&mut effect, 4_800);

            assert!(
                output.iter().all(|sample| sample.is_finite()),
                "an effect variant produced a non-finite sample"
            );
            assert!(rms(&output) > 1e-4, "an effect variant produced silence");
        }
    }

    #[test]
    fn every_stereo_variant_takes_and_returns_two_channels() {
        let mut modules = vec![
            StereoEffectModule::ChorusFlanger(ChorusFlanger::new(48_000.0)),
            StereoEffectModule::ReverbFdn(ReverbFdn::new(48_000.0)),
            StereoEffectModule::StereoWidth(StereoWidth::new(48_000.0)),
        ];

        for effect in modules.iter_mut() {
            let (left, right) = effect.process_stereo(0.5, -0.5);
            assert!(left.is_finite() && right.is_finite(), "non-finite stereo output");
        }
    }
}
