//! Ready-made nih-plug plugins built from the crate's own components.
//!
//! `cargo build` produces `O3DSP.dll` containing both plugins and `libo3dsp.rlib` for anyone who
//! wants the DSP pieces without a plugin host. Adding another plugin means adding a type here and
//! naming it in the two export macros in `lib.rs`.

pub mod reverb;
pub mod synth;

pub use reverb::O3Reverb;
pub use synth::O3Synth;
