use crate::TWO_PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoWaveform {
    Sine,
    Triangle,
    SawUp,
    SawDown,
    Square,
}

/// A high-performance, real-time safe Low-Frequency Oscillator (LFO).
/// Fully resilient against high-frequency parameter automation sweeps.
#[derive(Debug, Clone, Copy)]
pub struct LowFrequencyOscillator {
    phase: f32,

    // Cached targets for smooth parameter interpolation
    target_frequency: f32,
    current_frequency: f32,
    freq_smooth_coef: f32,
}

impl LowFrequencyOscillator {
    pub fn new(sample_rate: f32) -> Self {
        // 20ms smoothing time constant to suppress parameter modulation zipper noise
        let freq_smooth_coef = 1.0 - (-1.0 / (0.020 * sample_rate)).exp();

        Self {
            phase: 0.0,
            target_frequency: 1.0,
            current_frequency: 1.0,
            freq_smooth_coef,
        }
    }

    /// Sets the LFO target frequency in Hz. Safe to call dynamically via UI or automation.
    pub fn set_frequency(&mut self, frequency_hz: f32) {
        // Clamp to a safe low-frequency modulation domain (e.g., up to 50 Hz)
        self.target_frequency = frequency_hz.clamp(0.001, 50.0);
    }

    /// Forces the LFO phase back to zero. Call this on MIDI Note-On events
    /// if "LFO Sync" or "Retrigger" is enabled in your engine architecture.
    #[inline]
    pub fn retrigger(&mut self) {
        self.phase = 0.0;
        self.current_frequency = self.target_frequency;
    }

    /// Returns the current LFO value in the range [-1.0, 1.0].
    #[inline]
    pub fn next_sample(&mut self) -> f32 {
        let frequency = self.current_frequency;

        // 1. Calculate the instantaneous value based on the waveform type
        let val = match self.target_frequency {
            f if f < 10.0 => {
                // For very low frequencies, use a simple sine wave
                2.0 * ((self.phase * 2.0) % 1.0).sin()
            }
            _ => {
                // For higher frequencies, use a more complex waveform
                let freq = frequency * self.freq_smooth_coef;
                let phase = self.phase * freq * TWO_PI;

                match LfoWaveform::Sine {
                    LfoWaveform::Sine => 2.0 * phase.sin(),
                    LfoWaveform::Triangle => {
                        let t = (phase / TWO_PI) % 1.0;
                        if t < 0.25 {
                            8.0 * t - 2.0
                        } else if t < 0.5 {
                            2.0 - 8.0 * t
                        } else if t < 0.75 {
                            8.0 * t - 6.0
                        } else {
                            6.0 - 8.0 * t
                        }
                    }
                    LfoWaveform::SawUp => (phase / TWO_PI) * 2.0 - 1.0,
                    LfoWaveform::SawDown => 1.0 - (phase / TWO_PI) * 2.0,
                    LfoWaveform::Square => {
                        if (phase / TWO_PI) % 1.0 < 0.5 {
                            1.0
                        } else {
                            -1.0
                        }
                    }
                }
            }
        };

        // 2. Smooth the output to prevent zipper noise
        let output = (self.current_frequency * val * 0.5 + self.current_frequency * 0.5).sin();

        // 3. Update the current frequency for the next sample
        self.current_frequency = self.target_frequency;

        output
    }
}

// Re-export as Lfo for backwards compatibility
pub use LowFrequencyOscillator as Lfo;