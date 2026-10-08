use std::f32::consts::TAU;

/// Phase Distortion Oscillator (Warp / Wobble Oscillator).
/// Generates rich harmonics by modulating the phase of a sine wave through a 
/// non-linear waveshaping function.
///
/// Key features:
/// - Creates rich harmonic content (odd harmonics primarily)
/// - Controls "warp" amount to shape harmonic spectrum
/// - Can be used for bass wobble, industrial textures, or FM-like synthesis
#[derive(Debug, Clone)]
pub struct PhaseDistortionOscillator {
    sample_rate: f32,

    // Waveshaper coefficients (polynomial approximation)
    // For a cubic waveshaper: y = x + k*x^3

    // Oscillator state
    phase: f32,
    phase_increment: f32,

    // Smoothed parameters
    freq_smoother: crate::synthesis::utils::ParameterSmoother,
    warp_smoother: crate::synthesis::utils::ParameterSmoother,
    output_gain_smoother: crate::synthesis::utils::ParameterSmoother,

    // Pre-computed waveshaper coefficients
    waveshaper_coef: f32,
}

impl Default for PhaseDistortionOscillator {
    fn default() -> Self {
        Self::new(48000.0, 100.0, 0.5) // 100Hz, warp=0.5
    }
}

impl PhaseDistortionOscillator {
    /// Create a new phase distortion oscillator.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `frequency` - Base frequency in Hz
    /// * `warp` - Warp amount (0.0 = pure sine, 1.0 = maximum distortion)
    pub fn new(sample_rate: f32, frequency: f32, warp: f32) -> Self {
        Self {
            sample_rate,
            freq_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                frequency,
            ),
            warp_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                warp,
            ),
            output_gain_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                1.0,
            ),
            phase: 0.0,
            phase_increment: frequency / sample_rate,
            waveshaper_coef: 0.0,
        }
    }

    /// Set frequency (smoothed)
    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq_smoother.set_target(freq_hz.max(10.0).min(self.sample_rate * 0.2));
        self.phase_increment = freq_hz / self.sample_rate;
    }

    /// Set warp amount (smoothed)
    pub fn set_warp(&mut self, warp: f32) {
        self.warp_smoother.set_target(warp.max(0.0).min(1.0));
    }

    /// Set output gain (smoothed)
    pub fn set_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.max(0.0).min(1.0));
    }

    /// Get current frequency
    pub fn frequency(&mut self) -> f32 {
        self.freq_smoother.process_sample()
    }

    /// Get current warp amount
    pub fn warp(&mut self) -> f32 {
        self.warp_smoother.process_sample()
    }

    /// Update waveshaper coefficients
    fn update_waveshaper(&mut self) {
        let warp = self.warp_smoother.process_sample();
        
        // Cubic waveshaper coefficients
        // For warp = 1.0, we get strong odd harmonic content
        self.waveshaper_coef = warp * warp * warp * 0.5;
    }

    /// Process a single sample
    #[inline(always)]
    fn waveshape(&self, x: f32) -> f32 {
        // Cubic waveshaper: y = x + k*x^3
        x + self.waveshaper_coef * x * x * x
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let freq = self.freq_smoother.process_sample();
        let gain = self.output_gain_smoother.process_sample();

        // Update phase
        self.phase += freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Generate the carrier, then apply the phase distortion (waveshaping).
        // `update_waveshaper` advances the warp smoother, so it is not read again here.
        self.update_waveshaper();
        let carrier = (TAU * self.phase).sin();

        // Peak-normalised so the level holds as warp is dialled in.
        self.waveshape(carrier) / (1.0 + self.waveshaper_coef) * gain
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
    fn test_phase_distortion() {
        let sample_rate = 48_000.0;
        let freq = 100.0;
        let mut oscillator = PhaseDistortionOscillator::new(sample_rate, freq, 0.5);

        // The waveshaper is monotonic, so the oscillator must run at the frequency asked for.
        let mut crossings = 0;
        let mut previous = oscillator.process_sample(0.0);
        for _ in 0..(sample_rate as usize - 1) {
            let current = oscillator.process_sample(0.0);
            assert!(current.is_finite() && current.abs() <= 1.0, "output {current}");
            if previous <= 0.0 && current > 0.0 {
                crossings += 1;
            }
            previous = current;
        }

        assert!(
            (crossings as f32 - freq).abs() < 3.0,
            "expected ~{freq} cycles per second, saw {crossings}"
        );
    }

    #[test]
    fn test_warp_range() {
        let sample_rate = 48_000.0;
        let samples = 4800;

        let mut clean = PhaseDistortionOscillator::new(sample_rate, 100.0, 0.0);
        let mut warped = PhaseDistortionOscillator::new(sample_rate, 100.0, 1.0);

        let (mut clean_energy, mut difference_energy, mut clean_peak) = (0.0f32, 0.0f32, 0.0f32);

        for _ in 0..samples {
            let plain = clean.process_sample(0.0);
            let distorted = warped.process_sample(0.0);

            clean_energy += plain * plain;
            difference_energy += (distorted - plain) * (distorted - plain);
            clean_peak = clean_peak.max(plain.abs());
        }

        // With warp at zero the oscillator is a plain unit sine.
        assert!((clean_peak - 1.0).abs() < 1e-3, "clean peak {clean_peak}");
        let clean_rms = (clean_energy / samples as f32).sqrt();
        assert!(
            (clean_peak / clean_rms - std::f32::consts::SQRT_2).abs() < 0.01,
            "clean wave should be a sine"
        );

        // Full warp must actually reshape the wave.
        let difference_rms = (difference_energy / samples as f32).sqrt();
        assert!(
            difference_rms > 0.05,
            "warp had no audible effect: difference rms {difference_rms}"
        );
    }
}