//! Spectral analysis, and the vocabulary additive synthesis is built from.
//!
//! [`analysis`] turns a block of audio into a list of [`Partial`]s (frequency, amplitude, phase) -
//! a spectral snapshot of that frame. The FFT is the expensive half, so analyse up front (load,
//! record, sample) and keep playback on the audio thread: hand the snapshot to
//! [`crate::synthesis::oscillators::SineBank`], one sine per partial.

pub mod additive;
pub mod analysis;
pub mod fft;
pub mod morph;
pub mod spectrum;

pub use additive::AdditiveOscillator;
pub use analysis::SpectralAnalyser;
pub use fft::{Complex, Fft};
pub use morph::SpectrumMorph;
pub use spectrum::{HarmonicSpectrum, MAX_PARTIALS};

/// One sinusoidal component of a frame.
///
/// This is the vocabulary the analysis and synthesis halves share: analysis produces these, the
/// sine bank consumes them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Partial {
    /// Hz.
    pub frequency: f32,
    /// Linear amplitude, corrected for the analysis window's gain.
    pub amplitude: f32,
    /// Radians, taken from the bin of the peak.
    pub phase: f32,
}

impl Partial {
    pub const fn new(frequency: f32, amplitude: f32, phase: f32) -> Self {
        Self {
            frequency,
            amplitude,
            phase,
        }
    }
}
