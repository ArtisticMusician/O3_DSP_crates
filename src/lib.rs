pub mod common;
pub mod core;
pub mod memory;
pub mod synthesis;

pub const PI: f32 = std::f32::consts::PI;
pub const TWO_PI: f32 = PI * 2.0;

pub use common::integration::iterator::ProcessorIterator;

#[cfg(feature = "nih_plug_support")]
pub use common::integration::nih_plug::{NihPlugProcessorExt, StereoProcessorWrapper};

#[cfg(feature = "dasp_support")]
pub use common::integration::dasp::{DaspGeneratorAdapter, DaspEffectAdapter};
