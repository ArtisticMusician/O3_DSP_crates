pub mod audio;
pub mod common;
pub mod core;
pub mod memory;
pub mod plugins;
pub mod synthesis;

pub const PI: f32 = std::f32::consts::PI;
pub const TWO_PI: f32 = PI * 2.0;

pub use common::integration::iterator::ProcessorIterator;
pub use plugins::{O3Reverb, O3Synth};

#[cfg(feature = "nih_plug_support")]
pub use common::integration::nih_plug::{NihPlugProcessorExt, StereoProcessorWrapper};

#[cfg(feature = "dasp_support")]
pub use common::integration::dasp::{DaspGeneratorAdapter, DaspEffectAdapter};

// One DLL, both plugins. Adding another plugin means naming its type in these two lists.
nih_plug::nih_export_clap!(O3Synth, O3Reverb);
nih_plug::nih_export_vst3!(O3Synth, O3Reverb);
