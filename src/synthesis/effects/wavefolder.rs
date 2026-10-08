use crate::synthesis::utils::ParameterSmoother;

/// How hard `fold_amount == 1.0` drives the signal into the folder.
const MAX_DRIVE: f32 = 8.0;

/// Wavefolder distortion effect.
/// Creates harsh, rich distortion by folding the input waveform at regular intervals.
///
/// `fold_amount` is a depth control: 0.0 passes the signal through untouched, 1.0 drives it
/// into the folder hard enough to fold repeatedly. The folding shape is chosen by
/// [`WavefolderType`]:
/// - Triangle: classic hard reflection off +/-1
/// - Sine: smooth, rounded folds
/// - Sawtooth: harsh sawtooth wraps
#[derive(Debug, Clone, Copy)]
pub struct Wavefolder {
    // Fold amount (depth)

    // Waveform type
    waveform: WavefolderType,

    // Smoothed parameters
    fold_amount_smoother: ParameterSmoother,
    output_gain_smoother: ParameterSmoother,
}

/// Wavefolder waveform types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WavefolderType {
    Triangle,
    Sine,
    Sawtooth,
}

impl Default for Wavefolder {
    fn default() -> Self {
        Self::new(48000.0, 0.5, WavefolderType::Triangle)
    }
}

impl Wavefolder {
    /// Create a new wavefolder.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `fold_amount` - Folding depth (0.0 = bypass, 1.0 = maximum folding)
    /// * `waveform` - Type of folding waveform
    pub fn new(sample_rate: f32, fold_amount: f32, waveform: WavefolderType) -> Self {
        Self {
            waveform,
            fold_amount_smoother: ParameterSmoother::new(
                sample_rate,
                10.0,
                fold_amount.clamp(0.0, 1.0),
            ),
            output_gain_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
        }
    }

    /// Set fold amount (smoothed)
    pub fn set_fold_amount(&mut self, amount: f32) {
        self.fold_amount_smoother
            .set_target(amount.clamp(0.0, 1.0));
    }

    /// Set waveform type
    pub fn set_waveform(&mut self, waveform: WavefolderType) {
        self.waveform = waveform;
    }

    /// Set output gain (smoothed, linear)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.clamp(0.0, 8.0));
    }

    /// Get current fold amount
    pub fn fold_amount(&self) -> f32 {
        self.fold_amount_smoother.current_value()
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let fold_amount = self.fold_amount_smoother.process_sample();
        let output_gain = self.output_gain_smoother.process_sample();

        // Depth 0.0 leaves the signal alone on every shape, since each fold function is the
        // identity within +/-1 and a unity drive never leaves that range.
        let drive = 1.0 + fold_amount * MAX_DRIVE;
        let folded = fold(self.waveform, input * drive);

        folded * output_gain
    }

    /// Process a block of samples
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }
}

/// Fold `x` into the +/-1 range using the selected shape. Every shape is the identity for
/// inputs inside +/-1, so a small enough drive always passes through cleanly.
#[inline]
fn fold(waveform: WavefolderType, x: f32) -> f32 {
    if x.abs() <= 1.0 {
        return x;
    }

    match waveform {
        WavefolderType::Triangle => {
            // Repeated hard reflection off the +/-1 rails, alternating until in range.
            let mut y = x;
            loop {
                if y > 1.0 {
                    y = 2.0 - y;
                } else if y < -1.0 {
                    y = -2.0 - y;
                } else {
                    break y;
                }
            }
        }
        WavefolderType::Sine => (x * std::f32::consts::FRAC_PI_2).sin(),
        WavefolderType::Sawtooth => {
            let wrap = x - x.floor();
            2.0 * wrap - 1.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wavefolder() {
        let mut wavefolder = Wavefolder::new(48000.0, 0.5, WavefolderType::Triangle);

        let sample = wavefolder.process_sample(1.0);
        assert!(sample.is_finite());
        assert!(sample.abs() <= 1.0, "folded sample out of range: {sample}");
    }

    #[test]
    fn test_fold_amount() {
        let mut wavefolder = Wavefolder::new(48000.0, 0.0, WavefolderType::Triangle);

        // With no fold, should pass through
        let sample_no_fold = wavefolder.process_sample(0.5);
        assert!((sample_no_fold - 0.5).abs() < 0.01, "bypass gave {sample_no_fold}");

        // With fold, should distort
        wavefolder.set_fold_amount(1.0);
        let sample_folded = wavefolder.process_sample(0.5);
        assert!(sample_folded.abs() < 1.0);
    }

    #[test]
    fn every_shape_bypasses_at_zero_depth() {
        for waveform in [
            WavefolderType::Triangle,
            WavefolderType::Sine,
            WavefolderType::Sawtooth,
        ] {
            let mut wavefolder = Wavefolder::new(48_000.0, 0.0, waveform);
            for input in [-0.9, -0.25, 0.0, 0.25, 0.9] {
                let out = wavefolder.process_sample(input);
                assert!(
                    (out - input).abs() < 1e-6,
                    "{waveform:?} changed {input} to {out} at zero depth"
                );
            }
        }
    }

    #[test]
    fn full_depth_stays_bounded() {
        for waveform in [
            WavefolderType::Triangle,
            WavefolderType::Sine,
            WavefolderType::Sawtooth,
        ] {
            let mut wavefolder = Wavefolder::new(48_000.0, 1.0, waveform);
            for i in 0..2000 {
                let out = wavefolder.process_sample((i as f32 * 0.05).sin());
                assert!(
                    out.is_finite() && out.abs() <= 1.0001,
                    "{waveform:?} produced {out}"
                );
            }
        }
    }
}
