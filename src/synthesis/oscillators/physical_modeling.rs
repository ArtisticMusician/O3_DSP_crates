use crate::synthesis::utils::ParameterSmoother;
use std::f32::consts::TAU;

/// Mass-spring physical modeling oscillator.
/// Simulates plucked string and drum-like sounds using digital waveguides.
///
/// Physical modeling basics:
/// - Mass represents string/air column inertia
/// - Spring represents restoring force (tension)
/// - Damping is the energy lost per cycle
///
/// The resonator rings on its own once struck: `strike()` loads the impulse and
/// `process_sample()` runs the mass-spring integration, so the input argument is only
/// there to match the processor shape.
///
/// Use cases:
/// - Plucked string (guitar, harp)
/// - Drum heads
/// - Wind instruments (simplified)
#[derive(Debug, Clone, Copy)]
pub struct MassSpring {
    sample_rate: f32,

    // Physical parameters
    mass: f32,
    damping: f32,

    // State variables
    displacement: f32,
    velocity: f32,

    // Strike impulse, consumed by the next processed sample
    strike: f32,

    // Smoothed parameters
    frequency_smoother: ParameterSmoother,
    damping_smoother: ParameterSmoother,
    output_gain_smoother: ParameterSmoother,

    // Current frequency (for UI feedback)
    current_frequency: f32,
}

impl Default for MassSpring {
    fn default() -> Self {
        Self::new(48000.0, 220.0, 0.1) // 48kHz, 220Hz, 0.1 damping
    }
}

impl MassSpring {
    /// Create a new mass-spring oscillator.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `frequency` - Fundamental frequency in Hz
    /// * `damping` - Energy loss per cycle (0.0 = no damping, 1.0 = complete silence)
    pub fn new(sample_rate: f32, frequency: f32, damping: f32) -> Self {
        let frequency = frequency.clamp(20.0, 2000.0);
        let damping = damping.clamp(0.0, 1.0);

        Self {
            sample_rate,
            mass: 1.0,
            damping,
            displacement: 0.0,
            velocity: 0.0,
            strike: 0.0,
            frequency_smoother: ParameterSmoother::new(sample_rate, 10.0, frequency),
            damping_smoother: ParameterSmoother::new(sample_rate, 10.0, damping),
            output_gain_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
            current_frequency: frequency,
        }
    }

    /// Set frequency (smoothed)
    pub fn set_frequency(&mut self, freq_hz: f32) {
        let freq_hz = freq_hz.clamp(20.0, 2000.0);
        self.frequency_smoother.set_target(freq_hz);
        self.current_frequency = freq_hz;
    }

    /// Set damping (smoothed)
    pub fn set_damping(&mut self, damping: f32) {
        self.damping_smoother.set_target(damping.clamp(0.0, 1.0));
    }

    /// Set output gain (smoothed, linear)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.clamp(0.0, 8.0));
    }

    /// Strike the oscillator (pluck, hit, etc.)
    pub fn strike(&mut self, strength: f32) {
        self.strike = strength;
        self.displacement = 0.0;
    }

    /// Reset to initial state
    pub fn reset(&mut self) {
        self.displacement = 0.0;
        self.velocity = 0.0;
        self.strike = 0.0;
    }

    /// Get current frequency
    pub fn frequency(&self) -> f32 {
        self.current_frequency
    }

    /// Get current amplitude
    pub fn amplitude(&self) -> f32 {
        (self.displacement * self.displacement + self.velocity * self.velocity).sqrt()
    }

    /// Per-sample restoring force coefficient for `freq`: omega^2, with omega in radians
    /// per sample. Using Hz here instead would oscillate far above Nyquist.
    #[inline]
    fn spring_constant(&self, freq: f32) -> f32 {
        let omega = TAU * freq / self.sample_rate;
        omega * omega
    }

    /// Per-sample velocity loss that removes `damping` of the amplitude over one cycle.
    #[inline]
    fn velocity_loss(&self, freq: f32) -> f32 {
        let cycles_per_sample = freq / self.sample_rate;
        let decay = (1.0 - self.damping).max(1e-6).powf(cycles_per_sample);
        1.0 - decay
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let freq = self.frequency_smoother.process_sample();
        self.damping = self.damping_smoother.process_sample();
        let output_gain = self.output_gain_smoother.process_sample();

        // A strike loads the resonator; it rings with no further input.
        if self.strike != 0.0 {
            self.velocity += self.strike;
            self.strike = 0.0;
        }

        let spring_constant = self.spring_constant(freq);
        let loss = self.velocity_loss(freq);

        // Mass-spring equation: a = (F - b*v - k*x) / m, with no external force.
        let acceleration = -(loss * self.velocity + spring_constant * self.displacement) / self.mass;

        // Semi-implicit Euler: stable for omega well below 2 rad/sample.
        self.velocity += acceleration;
        self.displacement += self.velocity;

        self.current_frequency = freq;
        self.displacement * output_gain
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
    fn test_mass_spring() {
        let mut osc = MassSpring::new(48000.0, 220.0, 0.1);

        assert_eq!(osc.process_sample(0.0), 0.0, "silent before a strike");

        // Strike the oscillator: it must produce output with no further input.
        osc.strike(1.0);
        let sample = osc.process_sample(0.0);
        assert!(sample.abs() > 0.1, "strike produced {sample}");
        assert!(sample.is_finite());
    }

    #[test]
    fn test_decay() {
        let mut osc = MassSpring::new(48000.0, 220.0, 0.1);
        osc.strike(1.0);

        let period = (48_000.0 / 220.0) as usize;
        let peak_over = |osc: &mut MassSpring, samples: usize| {
            let mut peak = 0.0f32;
            for _ in 0..samples {
                peak = peak.max(osc.process_sample(0.0).abs());
            }
            peak
        };

        let first_cycle = peak_over(&mut osc, period);
        peak_over(&mut osc, 48_000 - period); // skip a second of ring-down
        let later_cycle = peak_over(&mut osc, period);

        assert!(
            later_cycle < first_cycle * 0.5,
            "envelope should decay: {first_cycle} -> {later_cycle}"
        );
    }

    #[test]
    fn rings_at_its_requested_frequency() {
        let sample_rate = 48_000.0;
        let freq = 440.0;
        let mut osc = MassSpring::new(sample_rate, freq, 0.001);
        osc.strike(1.0);

        // Count zero crossings over a second of ring-down.
        let mut crossings = 0;
        let mut previous = osc.process_sample(0.0);
        for _ in 0..(sample_rate as usize - 1) {
            let current = osc.process_sample(0.0);
            if previous <= 0.0 && current > 0.0 {
                crossings += 1;
            }
            previous = current;
        }

        assert!(
            (crossings as f32 - freq).abs() < 10.0,
            "expected ~{freq} cycles per second, saw {crossings}"
        );
    }
}
