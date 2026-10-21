use crate::synthesis::utils::Processor;

/// Saturation and waveshaping effect.
/// Adds harmonic distortion by applying a non-linear transfer function.
#[derive(Debug, Clone)]
pub struct Saturation {
    drive: f32, // Input gain (linear multiplier)
    mix: f32,   // Wet/dry mix (0.0 to 1.0)
    mode: SaturationMode,
}

#[derive(Debug, Clone, Copy)]
pub enum SaturationMode {
    SoftClip,
    HardClip,
    Tanh,
    Foldback,
}

impl Saturation {
    pub fn new() -> Self {
        Self {
            drive: 1.0,
            mix: 1.0,
            mode: SaturationMode::Tanh,
        }
    }

    pub fn set_parameters(&mut self, drive_db: f32, mix: f32, mode: SaturationMode) {
        self.drive = 10.0_f32.powf(drive_db / 20.0).max(0.1);
        self.mix = mix.clamp(0.0, 1.0);
        self.mode = mode;
    }

    #[inline(always)]
    fn fast_tanh(x: f32) -> f32 {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

impl Processor for Saturation {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let driven = input * self.drive;

        let processed = match self.mode {
            SaturationMode::HardClip => driven.clamp(-1.0, 1.0),
            SaturationMode::SoftClip => {
                let x = driven.clamp(-1.5, 1.5);
                x - (x * x * x) / 6.0
            }
            SaturationMode::Tanh => Self::fast_tanh(driven),
            SaturationMode::Foldback => {
                // Odd-symmetric foldback
                let threshold = 1.0;
                let mut x = driven;
                let is_neg = x < 0.0;
                let abs_x = x.abs();

                if abs_x > threshold {
                    let mut folded = abs_x;
                    folded = folded.rem_euclid(4.0 * threshold);
                    if folded > 3.0 * threshold {
                        folded -= 4.0 * threshold;
                    } else if folded > threshold {
                        folded = 2.0 * threshold - folded;
                    }
                    x = if is_neg { -folded } else { folded };
                }
                x
            }
        };

        // Auto-gain makeup (approximate based on drive)
        let makeup = 1.0 / self.drive.sqrt();
        let wet = processed * makeup;

        input * (1.0 - self.mix) + wet * self.mix
    }
}
