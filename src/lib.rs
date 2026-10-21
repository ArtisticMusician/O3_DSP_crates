pub mod audio;
pub mod common;
pub mod core;
pub mod memory;
pub mod synthesis;

pub const PI: f32 = std::f32::consts::PI;
pub const TWO_PI: f32 = PI * 2.0;

pub use common::integration::buffer::{SliceProcessorExt, StereoProcessorWrapper};
pub use common::integration::iterator::ProcessorIterator;

#[cfg(feature = "dasp_support")]
pub use common::integration::dasp::{DaspEffectAdapter, DaspGeneratorAdapter};
