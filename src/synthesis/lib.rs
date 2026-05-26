pub mod envelope;
pub mod math_utils;
pub mod one_pole;
pub mod oscillator;

pub const PI: f32 = std::f32::consts::PI;
pub const TWO_PI: f32 = PI * 2.0;

pub use envelope::{AdsrEnvelope, AdsrStage};
pub use math_utils::{db_to_linear, linear_to_db};
pub use one_pole::OnePoleFilter;
pub use oscillator::{TableOscillator, Waveform};