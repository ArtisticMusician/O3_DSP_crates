
/// Loopable envelope for LFO-like modulation.
/// Generates a smooth, repeating waveform that can be shaped with custom attack,
/// decay, sustain, release, and loop parameters.
///
/// Unlike a traditional ADSR envelope that starts from 0 and ends at 0,
/// this envelope loops continuously between any two points.
#[derive(Debug, Clone, Copy)]
pub struct LoopableEnvelope {
    sample_rate: f32,

    // Current state and value
    state: LoopableState,
    current_value: f32,

    // Loop parameters (in samples)
    attack_samples: u32,
    decay_samples: u32,
    sustain_samples: u32,
    release_samples: u32,

    // Current step counter
    step_counter: u32,

    // Target values for each stage
    attack_target: f32,
    decay_target: f32,
    sustain_target: f32,
    release_target: f32,

    // Smoothed parameters
    attack_smoother: crate::synthesis::utils::ParameterSmoother,
    decay_smoother: crate::synthesis::utils::ParameterSmoother,
    sustain_smoother: crate::synthesis::utils::ParameterSmoother,
    release_smoother: crate::synthesis::utils::ParameterSmoother,
    output_gain_smoother: crate::synthesis::utils::ParameterSmoother,
}

/// Envelope states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopableState {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

impl Default for LoopableEnvelope {
    fn default() -> Self {
        Self::new(48000.0, 10.0, 20.0, 50.0, 30.0)
    }
}

impl LoopableEnvelope {
    /// Create a new loopable envelope.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `attack_ms` - Attack time in milliseconds
    /// * `decay_ms` - Decay time in milliseconds
    /// * `sustain_ms` - Sustain time in milliseconds
    /// * `release_ms` - Release time in milliseconds
    pub fn new(sample_rate: f32, attack_ms: f32, decay_ms: f32, sustain_ms: f32, release_ms: f32) -> Self {
        let attack_samples = (attack_ms * 0.001 * sample_rate).max(1.0) as u32;
        let decay_samples = (decay_ms * 0.001 * sample_rate).max(1.0) as u32;
        let sustain_samples = (sustain_ms * 0.001 * sample_rate).max(1.0) as u32;
        let release_samples = (release_ms * 0.001 * sample_rate).max(1.0) as u32;

        Self {
            sample_rate,
            state: LoopableState::Idle,
            current_value: 0.0,
            attack_samples,
            decay_samples,
            sustain_samples,
            release_samples,
            step_counter: 0,

            attack_target: 1.0,
            decay_target: 0.0,
            sustain_target: 0.7,
            release_target: 0.0,

            attack_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                attack_ms,
            ),
            decay_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                decay_ms,
            ),
            sustain_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                sustain_ms,
            ),
            release_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                release_ms,
            ),
            output_gain_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                1.0,
            ),
        }
    }

    /// Set attack time (smoothed)
    pub fn set_attack(&mut self, attack_ms: f32) {
        self.attack_smoother.set_target(attack_ms.max(0.1).min(5000.0));
    }

    /// Set decay time (smoothed)
    pub fn set_decay(&mut self, decay_ms: f32) {
        self.decay_smoother.set_target(decay_ms.max(0.1).min(5000.0));
    }

    /// Set sustain time (smoothed)
    pub fn set_sustain(&mut self, sustain_ms: f32) {
        self.sustain_smoother.set_target(sustain_ms.max(0.1).min(5000.0));
    }

    /// Set release time (smoothed)
    pub fn set_release(&mut self, release_ms: f32) {
        self.release_smoother.set_target(release_ms.max(0.1).min(5000.0));
    }

    /// Set attack target value (smoothed)
    pub fn set_attack_target(&mut self, value: f32) {
        self.attack_target = value.clamp(-1.0, 1.0);
    }

    /// Set decay target value (smoothed)
    pub fn set_decay_target(&mut self, value: f32) {
        self.decay_target = value.clamp(-1.0, 1.0);
    }

    /// Set sustain target value (smoothed)
    pub fn set_sustain_target(&mut self, value: f32) {
        self.sustain_target = value.clamp(-1.0, 1.0);
    }

    /// Set release target value (smoothed)
    pub fn set_release_target(&mut self, value: f32) {
        self.release_target = value.clamp(-1.0, 1.0);
    }

    /// Set output gain (smoothed)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.max(-24.0).min(24.0));
    }

    /// Get current value
    pub fn value(&mut self) -> f32 {
        self.current_value * self.output_gain_smoother.process_sample()
    }

    /// Get current state
    pub fn state(&self) -> LoopableState {
        self.state
    }

    /// Process a single sample
    #[inline(always)]
    fn update_params(&mut self) {
        let attack = self.attack_smoother.process_sample();
        let decay = self.decay_smoother.process_sample();
        let sustain = self.sustain_smoother.process_sample();
        let release = self.release_smoother.process_sample();

        self.attack_samples = (attack * 0.001 * self.sample_rate).max(1.0) as u32;
        self.decay_samples = (decay * 0.001 * self.sample_rate).max(1.0) as u32;
        self.sustain_samples = (sustain * 0.001 * self.sample_rate).max(1.0) as u32;
        self.release_samples = (release * 0.001 * self.sample_rate).max(1.0) as u32;
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        self.update_params();

        let output_gain = self.output_gain_smoother.process_sample();

        match self.state {
            LoopableState::Idle => {
                self.state = LoopableState::Attack;
                self.step_counter = 0;
            }
            LoopableState::Attack => {
                if self.step_counter >= self.attack_samples {
                    self.state = LoopableState::Decay;
                    self.step_counter = 0;
                } else {
                    self.step_counter += 1;
                }

                // Linear attack curve
                let progress = self.step_counter as f32 / self.attack_samples as f32;
                self.current_value = self.attack_target * progress;
            }
            LoopableState::Decay => {
                if self.step_counter >= self.decay_samples {
                    self.state = LoopableState::Sustain;
                    self.step_counter = 0;
                } else {
                    self.step_counter += 1;
                }

                // Linear decay curve
                let progress = self.step_counter as f32 / self.decay_samples as f32;
                self.current_value = (1.0 - progress) * self.attack_target + progress * self.decay_target;
            }
            LoopableState::Sustain => {
                if self.step_counter >= self.sustain_samples {
                    self.state = LoopableState::Release;
                    self.step_counter = 0;
                } else {
                    self.step_counter += 1;
                }

                // Hold sustain value
                self.current_value = self.sustain_target;
            }
            LoopableState::Release => {
                if self.step_counter >= self.release_samples {
                    // Loop back to attack
                    self.state = LoopableState::Attack;
                    self.step_counter = 0;
                } else {
                    self.step_counter += 1;
                }

                // Linear release curve
                let progress = self.step_counter as f32 / self.release_samples as f32;
                self.current_value = self.sustain_target * (1.0 - progress);
            }
        }

        self.current_value * output_gain
    }

    /// Reset to idle state
    pub fn reset(&mut self) {
        self.state = LoopableState::Idle;
        self.current_value = 0.0;
        self.step_counter = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loopable_envelope() {
        let sample_rate = 48_000.0;
        let mut envelope = LoopableEnvelope::new(sample_rate, 50.0, 100.0, 200.0, 150.0);

        // The cycle is 500 ms long, so one second of samples covers it twice.
        let (mut lowest, mut highest) = (f32::MAX, f32::MIN);
        for _ in 0..sample_rate as usize {
            let value = envelope.process_sample(0.0);
            assert!(value.is_finite(), "envelope produced {value}");
            lowest = lowest.min(value);
            highest = highest.max(value);
        }

        assert!(
            highest - lowest > 0.1,
            "looping envelope never moved: {lowest}..{highest}"
        );
    }

    #[test]
    fn test_envelope_values() {
        let mut envelope = LoopableEnvelope::new(48000.0, 10.0, 10.0, 10.0, 10.0);

        // Set targets
        envelope.set_attack_target(1.0);
        envelope.set_decay_target(0.5);
        envelope.set_sustain_target(0.5);
        envelope.set_release_target(0.0);

        let value = envelope.value();
        assert!((value - 0.0).abs() < 0.01);
    }
}