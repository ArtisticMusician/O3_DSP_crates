pub mod noise;
pub mod polyblep;
pub mod wavetable;

pub use noise::{NoiseGenerator, NoiseType};
pub use polyblep::{TableOscillator, Waveform};
pub use wavetable::WavetableOscillator;

#[derive(Debug, Clone)]
pub enum OscillatorModule {
    PolyBlep(TableOscillator),
    Noise(NoiseGenerator),
    Wavetable(WavetableOscillator),
}

impl OscillatorModule {
    #[inline(always)]
    pub fn next_sample(&mut self, wave_type: Waveform, noise_type: NoiseType) -> f32 {
        match self {
            OscillatorModule::PolyBlep(osc) => osc.next_sample(wave_type),
            OscillatorModule::Noise(osc) => osc.next_sample(noise_type),
            OscillatorModule::Wavetable(osc) => osc.process_sample(),
        }
    }
}
