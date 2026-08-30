/// Saturation and waveshaping effect.
/// Adds harmonic distortion by applying a non-linear transfer function.
pub struct Saturation {
    pub drive: f32, // Input gain (linear multiplier)
    pub mix: f32,   // Wet/dry mix (0.0 to 1.0)
    pub mode: SaturationMode,
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
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let driven = input * self.drive;

        let processed = match self.mode {
            SaturationMode::HardClip => {
                driven.clamp(-1.0, 1.0)
            },
            SaturationMode::SoftClip => {
                let x = driven.clamp(-1.5, 1.5);
                x - (x * x * x) / 6.0
            },
            SaturationMode::Tanh => {
                driven.tanh()
            },
            SaturationMode::Foldback => {
                let threshold = 1.0;
                let mut x = driven;
                // Simple fast folding for high drive
                if x > threshold || x < -threshold {
                    x = (x + threshold) % (4.0 * threshold) - threshold;
                    if x > threshold {
                        x = 2.0 * threshold - x;
                    }
                }
                x
            }
        };

        // Auto-gain makeup (approximate based on drive)
        let makeup = 1.0 / self.drive.sqrt();
        let wet = processed * makeup;

        input * (1.0 - self.mix) + wet * self.mix
    }

    #[inline]
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }
}
