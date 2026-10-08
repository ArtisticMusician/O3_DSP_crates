use crate::synthesis::utils::{random_f32, ParameterSmoother};
use std::f32::consts::TAU;

/// Vector Synthesis oscillator (4-quadrant morph).
/// Morphs between multiple waveforms using vector-based interpolation.
///
/// Key features:
/// - Morph between any combination of sine, square, triangle, sawtooth, noise
/// - Smooth transitions without zipper noise
/// - Rich harmonic content through waveform blending
#[derive(Debug, Clone, Copy)]
pub struct VectorSynthesis {
    sample_rate: f32,

    // Current waveform vector (sum of weighted waveforms)
    current_value: f32,

    // Waveform contributions
    sine: f32,
    square: f32,
    triangle: f32,
    sawtooth: f32,
    noise: f32,

    // Phase accumulator (0.0 to 1.0)
    phase: f32,

    // Smoothed parameters
    freq_smoother: ParameterSmoother,
    amplitude_smoother: ParameterSmoother,
    output_gain_smoother: ParameterSmoother,
}

impl Default for VectorSynthesis {
    fn default() -> Self {
        Self::new(48000.0, 440.0, 1.0)
    }
}

impl VectorSynthesis {
    /// Create a new vector synthesis oscillator.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `frequency` - Base frequency in Hz
    /// * `amplitude` - Base amplitude
    pub fn new(sample_rate: f32, frequency: f32, amplitude: f32) -> Self {
        Self {
            sample_rate,
            freq_smoother: ParameterSmoother::new(sample_rate, 10.0, frequency),
            amplitude_smoother: ParameterSmoother::new(sample_rate, 10.0, amplitude),
            output_gain_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
            phase: 0.0,
            current_value: 0.0,
            sine: 1.0,
            square: 0.0,
            triangle: 0.0,
            sawtooth: 0.0,
            noise: 0.0,
        }
    }

    /// Set frequency (smoothed)
    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq_smoother
            .set_target(freq_hz.max(10.0).min(self.sample_rate * 0.2));
    }

    /// Set amplitude (smoothed)
    pub fn set_amplitude(&mut self, amp: f32) {
        self.amplitude_smoother.set_target(amp.max(0.0).min(1.0));
    }

    /// Set sine contribution (0.0 to 1.0)
    pub fn set_sine(&mut self, value: f32) {
        self.sine = value.clamp(0.0, 1.0);
    }

    /// Set square contribution (0.0 to 1.0)
    pub fn set_square(&mut self, value: f32) {
        self.square = value.clamp(0.0, 1.0);
    }

    /// Set triangle contribution (0.0 to 1.0)
    pub fn set_triangle(&mut self, value: f32) {
        self.triangle = value.clamp(0.0, 1.0);
    }

    /// Set sawtooth contribution (0.0 to 1.0)
    pub fn set_sawtooth(&mut self, value: f32) {
        self.sawtooth = value.clamp(0.0, 1.0);
    }

    /// Set noise contribution (0.0 to 1.0)
    pub fn set_noise(&mut self, value: f32) {
        self.noise = value.clamp(0.0, 1.0);
    }

    /// Set all contributions at once
    pub fn set_waveforms(&mut self, sine: f32, square: f32, triangle: f32, sawtooth: f32, noise: f32) {
        self.sine = sine.clamp(0.0, 1.0);
        self.square = square.clamp(0.0, 1.0);
        self.triangle = triangle.clamp(0.0, 1.0);
        self.sawtooth = sawtooth.clamp(0.0, 1.0);
        self.noise = noise.clamp(0.0, 1.0);
    }

    /// Get current value
    pub fn value(&self) -> f32 {
        self.current_value * self.output_gain_smoother.current_value()
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let freq = self.freq_smoother.process_sample();
        let amplitude = self.amplitude_smoother.process_sample();
        let output_gain = self.output_gain_smoother.process_sample();

        // Update phase
        self.phase += freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Generate individual waveforms, each bounded to +/-1.
        let p = self.phase;
        let sine_wave = (TAU * p).sin();
        let square_wave = if p < 0.5 { 1.0 } else { -1.0 };
        let triangle_wave = 2.0 * (2.0 * p - 1.0).abs() - 1.0;
        let sawtooth_wave = 2.0 * p - 1.0;

        // Compute weighted sum
        let noise = 2.0 * (random_f32() - 0.5);
        let waveform = self.sine * sine_wave
            + self.square * square_wave
            + self.triangle * triangle_wave
            + self.sawtooth * sawtooth_wave
            + self.noise * noise;

        let output = waveform * amplitude * output_gain;
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
    fn test_vector_synthesis() {
        let mut osc = VectorSynthesis::new(48000.0, 440.0, 1.0);

        osc.set_waveforms(1.0, 0.0, 0.0, 0.0, 0.0); // Pure sine

        // A unit-amplitude sine never leaves +/-1.
        for _ in 0..512 {
            let sample = osc.process_sample(0.0);
            assert!(sample.abs() <= 1.0, "sine exceeded range: {sample}");
        }
    }

    #[test]
    fn test_morphing() {
        let mut osc = VectorSynthesis::new(48000.0, 440.0, 1.0);

        osc.set_waveforms(1.0, 0.0, 0.0, 0.0, 0.0); // Sine
        let sample1 = osc.process_sample(0.0);

        osc.set_waveforms(0.0, 1.0, 0.0, 0.0, 0.0); // Square
        let sample2 = osc.process_sample(0.0);

        assert!(sample1.abs() < 1.0);
        assert!(sample2.abs() <= 1.0);
    }

    #[test]
    fn every_waveform_stays_in_range() {
        let mut osc = VectorSynthesis::new(48_000.0, 440.0, 1.0);

        for index in 0..5 {
            let mut weights = [0.0f32; 5];
            weights[index] = 1.0;
            osc.set_waveforms(weights[0], weights[1], weights[2], weights[3], weights[4]);

            for _ in 0..512 {
                let sample = osc.process_sample(0.0);
                assert!(
                    sample.abs() <= 1.0,
                    "waveform {index} exceeded range: {sample}"
                );
            }
        }
    }
}