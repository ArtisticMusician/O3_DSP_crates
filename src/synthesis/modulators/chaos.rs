
/// Chaos generator for creating deterministic random modulation signals.
/// Uses mathematical chaos systems (Logistic map, Lorenz map) to generate
/// complex, non-repeating patterns suitable for:
/// - Ambient textures
/// - Random modulation sources
/// - Procedural sound design
/// - Noise generation
#[derive(Debug, Clone, Copy)]
pub enum ChaosType {
    /// Logistic map: x_{n+1} = r * x_n * (1 - x_n)
    /// r = 3.8-4.0: chaotic behavior
    /// r = 2.0-3.0: periodic behavior
    Logistic,
    /// Lorenz map (simplified 1D): x_{n+1} = |x_n| * cos(r * x_n)
    /// r = 3.0-4.0: chaotic behavior
    Lorenz,
    /// Tent map: piecewise linear chaos
    /// Good for uniform distribution
    Tent,
}

/// Chaos generator
#[derive(Debug, Clone, Copy)]
pub struct ChaosGenerator {
    sample_rate: f32,

    // Current value
    current_value: f32,

    // Chaos state
    state: f32,
    chaos_type: ChaosType,

    // Smoothed parameters
    r_smoother: crate::synthesis::utils::ParameterSmoother,
    output_gain_smoother: crate::synthesis::utils::ParameterSmoother,

    // Phase accumulator for time-scaling
    phase: f32,
    phase_increment: f32,
}

impl Default for ChaosGenerator {
    fn default() -> Self {
        Self::new(48000.0, ChaosType::Logistic, 3.9) // 48kHz, logistic map, r=3.9 (chaotic)
    }
}

impl ChaosGenerator {
    /// Create a new chaos generator.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `chaos_type` - Type of chaos system
    /// * `r` - Control parameter (chaos threshold varies by type)
    pub fn new(sample_rate: f32, chaos_type: ChaosType, r: f32) -> Self {
        let r = r.clamp(1.0, 4.5);

        Self {
            sample_rate,
            current_value: 0.0,
            state: 0.1, // Initial state (avoid 0.0 for logistic map)
            chaos_type,
            r_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                r,
            ),
            output_gain_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                1.0,
            ),
            phase: 0.0,
            phase_increment: 1.0,
        }
    }

    /// Set control parameter (smoothed)
    pub fn set_r(&mut self, r: f32) {
        self.r_smoother.set_target(r.clamp(1.0, 4.5));
    }

    /// Set chaos type
    pub fn set_type(&mut self, chaos_type: ChaosType) {
        self.chaos_type = chaos_type;
    }

    /// Set output gain (smoothed)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.max(-24.0).min(24.0));
    }

    /// Set time scale (phase increment)
    pub fn set_time_scale(&mut self, rate_hz: f32) {
        self.phase_increment = rate_hz.max(0.1).min(self.sample_rate * 0.5) / self.sample_rate;
    }

    /// Reset state
    pub fn reset(&mut self) {
        self.state = 0.1;
        self.phase = 0.0;
    }

    /// Get current value
    pub fn value(&mut self) -> f32 {
        self.current_value * self.output_gain_smoother.process_sample()
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let r = self.r_smoother.process_sample();
        let output_gain = self.output_gain_smoother.process_sample();

        // Update phase
        self.phase += self.phase_increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        // Calculate chaos value
        let chaos_value = match self.chaos_type {
            ChaosType::Logistic => {
                // Logistic map
                let next_state = r * self.state * (1.0 - self.state);
                self.state = next_state.max(0.0).min(1.0);
                self.state * 2.0 - 1.0 // Map to [-1, 1]
            }
            ChaosType::Lorenz => {
                // Lorenz map (simplified)
                let next_state = (self.state.abs() * ((r * self.state).cos())).max(-1.0).min(1.0);
                self.state = next_state;
                self.state
            }
            ChaosType::Tent => {
                // Tent map
                let next_state = if self.state < 0.5 {
                    r * 2.0 * self.state
                } else {
                    r * 2.0 * (1.0 - self.state)
                };
                self.state = next_state.max(0.0).min(1.0);
                self.state * 2.0 - 1.0 // Map to [-1, 1]
            }
        };

        self.current_value = chaos_value * output_gain;
        self.current_value
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
    fn test_logistic_chaos() {
        let mut chaos = ChaosGenerator::new(48000.0, ChaosType::Logistic, 3.9);
        
        let mut previous = chaos.process_sample(0.0);
        let mut moved = false;

        for _ in 0..1000 {
            let current = chaos.process_sample(0.0);
            assert!(
                current.is_finite() && current.abs() <= 1.0,
                "chaos produced {current}"
            );
            if (current - previous).abs() > 1e-6 {
                moved = true;
            }
            previous = current;
        }

        assert!(moved, "logistic map never left its initial state");
    }

    #[test]
    fn test_chaos_r_values() {
        // Periodic behavior (r = 2.5)
        let mut periodic = ChaosGenerator::new(48000.0, ChaosType::Logistic, 2.5);
        let periodic_sample = periodic.process_sample(0.0);
        
        // Chaotic behavior (r = 3.9)
        let mut chaotic = ChaosGenerator::new(48000.0, ChaosType::Logistic, 3.9);
        let chaotic_sample = chaotic.process_sample(0.0);
        
        assert!(periodic_sample.abs() < 1.0);
        assert!(chaotic_sample.abs() < 1.0);
    }
}