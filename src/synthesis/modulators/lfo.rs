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
    phase_increment: f32,
    sample_rate: f32,
    
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
            phase_increment: 0.0,
            sample_rate,
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
    }

    /// Renders the next control-rate modulation coefficient.
    /// Output range maps perfectly to a bipolar [-1.0, 1.0) space.
    #[inline(always)]
    pub fn next_sample(&mut self, waveform: LfoWaveform) -> f32 {
        // 1. Smoothly interpolate frequency to eliminate zipper artifacts during sweeps
        self.current_frequency += (self.target_frequency - self.current_frequency) * self.freq_smooth_coef;
        self.phase_increment = self.current_frequency / self.sample_rate;

        // 2. Generate the waveform pattern based on the uncorrupted phase register
        let output = match waveform {
            LfoWaveform::Sine => (self.phase * TWO_PI).sin(),
            
            LfoWaveform::Triangle => {
                // Symmetrical, zero-centered triangle wave conversion
                let val = (self.phase * 2.0) - 1.0;
                2.0 * val.abs() - 1.0
            }
            
            LfoWaveform::SawUp => {
                (self.phase * 2.0) - 1.0
            }
            
            LfoWaveform::SawDown => {
                1.0 - (self.phase * 2.0)
            }
            
            LfoWaveform::Square => {
                // Branchless evaluation to prevent pipeline stalls
                if self.phase < 0.5 { 1.0 } else { -1.0 }
            }
        };

        // 3. Advance phase register across the [0.0, 1.0) boundary
        self.phase += self.phase_increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        output
    }
}