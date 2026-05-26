pub mod lfo;
pub mod mod_matrix;

pub const PI: f32 = std::f32::consts::PI;
pub const TWO_PI: f32 = PI * 2.0;

pub use lfo::{LfoWaveform, LowFrequencyOscillator};
pub use mod_matrix::LfoModulator;