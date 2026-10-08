use crate::synthesis::utils::random_f32;

/// Audio-rate modulator for FM/PM synthesis.
/// Generates continuous, smooth modulation signals that can be used for
/// frequency modulation, phase modulation, or other audio-rate processing.
///
/// Supports multiple waveforms and can be used for:
/// - FM synthesis operators
/// - Phase modulation
/// - Ring modulation
/// - Amplitude modulation
#[derive(Debug, Clone, Copy)]
pub struct AudioRateModulator {
    sample_rate: f32,

    // Current value
    current_value: f32,

    // Waveform type
    waveform: AudioRateWaveform,

    // Frequency (Hz)

    // Amplitude

    // Phase accumulator
    phase: f32,

    // Smoothed parameters
    freq_smoother: crate::synthesis::utils::ParameterSmoother,
    amp_smoother: crate::synthesis::utils::ParameterSmoother,
    output_gain_smoother: crate::synthesis::utils::ParameterSmoother,
}

/// Audio-rate waveform types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioRateWaveform {
    Sine,
    Square,
    Triangle,
    Sawtooth,
    Random,
}

impl Default for AudioRateModulator {
    fn default() -> Self {
        Self::new(48000.0, 10.0, 0.5, AudioRateWaveform::Sine)
    }
}

impl AudioRateModulator {
    /// Create a new audio-rate modulator.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `frequency` - Modulation frequency in Hz
    /// * `amplitude` - Modulation amplitude
    /// * `waveform` - Waveform type
    pub fn new(sample_rate: f32, frequency: f32, amplitude: f32, waveform: AudioRateWaveform) -> Self {
        Self {
            sample_rate,
            waveform,
            current_value: 0.0,
            phase: 0.0,
            freq_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                5.0,
                frequency,
            ),
            amp_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                5.0,
                amplitude,
            ),
            output_gain_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                5.0,
                1.0,
            ),
        }
    }

    /// Set frequency (smoothed)
    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq_smoother.set_target(freq_hz.max(0.1).min(self.sample_rate * 0.5));
    }

    /// Set amplitude (smoothed)
    pub fn set_amplitude(&mut self, amp: f32) {
        self.amp_smoother.set_target(amp.max(-24.0).min(24.0));
    }

    /// Set waveform
    pub fn set_waveform(&mut self, waveform: AudioRateWaveform) {
        self.waveform = waveform;
    }

    /// Get current value
    pub fn value(&mut self) -> f32 {
        self.current_value * self.output_gain_smoother.process_sample()
    }

    /// Process a single sample
    #[inline(always)]
    fn generate_sample(&mut self) -> f32 {
        let freq = self.freq_smoother.process_sample();
        let amp = self.amp_smoother.process_sample();

        match self.waveform {
            AudioRateWaveform::Sine => {
                // Simple sine wave
                let phase_increment = freq / self.sample_rate;
                self.phase += phase_increment;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                (std::f32::consts::TAU * self.phase).sin() * amp
            }
            AudioRateWaveform::Square => {
                // Square wave (50% duty cycle)
                let phase_increment = freq / self.sample_rate;
                self.phase += phase_increment;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                if self.phase < 0.5 {
                    amp
                } else {
                    -amp
                }
            }
            AudioRateWaveform::Triangle => {
                // Triangle wave
                let phase_increment = freq / self.sample_rate;
                self.phase += phase_increment;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                let t = self.phase * 2.0;
                if t < 1.0 {
                    2.0 * t * amp - amp
                } else {
                    amp - 2.0 * (t - 1.0) * amp
                }
            }
            AudioRateWaveform::Sawtooth => {
                // Sawtooth wave
                let phase_increment = freq / self.sample_rate;
                self.phase += phase_increment;
                if self.phase >= 1.0 {
                    self.phase -= 1.0;
                }
                2.0 * self.phase * amp - amp
            }
            AudioRateWaveform::Random => {
                // White noise
                let noise = 2.0 * (random_f32() - 0.5);
                noise * amp
            }
        }
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let output = self.generate_sample();
        self.current_value = output;
        output
    }

    /// Process a block of samples
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audio_rate_modulator() {
        let sample_rate = 48_000.0;
        let freq = 10.0;
        let mut modulator = AudioRateModulator::new(sample_rate, freq, 0.5, AudioRateWaveform::Sine);

        let mut crossings = 0;
        let mut previous = modulator.process_sample(0.0);
        for _ in 0..(sample_rate as usize - 1) {
            let current = modulator.process_sample(0.0);
            assert!(current.is_finite(), "modulator produced {current}");
            if previous <= 0.0 && current > 0.0 {
                crossings += 1;
            }
            previous = current;
        }

        assert!(
            (crossings as f32 - freq).abs() <= 2.0,
            "expected ~{freq} cycles per second, saw {crossings}"
        );
    }

    #[test]
    fn test_waveforms() {
        let mut sine = AudioRateModulator::new(48000.0, 10.0, 0.5, AudioRateWaveform::Sine);
        let mut square = AudioRateModulator::new(48000.0, 10.0, 0.5, AudioRateWaveform::Square);
        let mut triangle = AudioRateModulator::new(48000.0, 10.0, 0.5, AudioRateWaveform::Triangle);
        
        let sine_sample = sine.process_sample(0.0);
        let square_sample = square.process_sample(0.0);
        let triangle_sample = triangle.process_sample(0.0);
        
        assert!(sine_sample.abs() < 1.0);
        assert!(square_sample.abs() <= 0.5);
        assert!(triangle_sample.abs() < 1.0);
    }
}