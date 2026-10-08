
/// AR (Attack-Release) envelope - simple gate for triggering events.
/// Unlike ADSR, AR envelopes only have two stages, making them ideal for:
/// - Percussion triggers
/// - Simple gate modulation
/// - Quick on/off modulation
///
/// AHR (Attack-Hold-Release) adds a hold stage for pausing at peak value.
#[derive(Debug, Clone, Copy)]
pub enum ArEnvelopeType {
    /// Simple AR envelope (2 stages)
    Ar,
    /// AHR envelope (3 stages)
    Ahr,
}

/// AR/AHR envelope state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArEnvelopeState {
    Idle,
    Attack,
    Hold,
    Release,
    Done,
}

/// AR/AHR envelope
#[derive(Debug, Clone, Copy)]
pub struct ArEnvelope {

    // Current state and value
    state: ArEnvelopeState,
    current_value: f32,

    // Stage durations (in samples)
    attack_samples: u32,
    hold_samples: u32,
    release_samples: u32,

    // Current step counter
    step_counter: u32,

    // Target values
    attack_target: f32,
    hold_target: f32,
    release_target: f32,

    // Smoothed parameters
    attack_smoother: crate::synthesis::utils::ParameterSmoother,
    hold_smoother: crate::synthesis::utils::ParameterSmoother,
    release_smoother: crate::synthesis::utils::ParameterSmoother,
    output_gain_smoother: crate::synthesis::utils::ParameterSmoother,
}

impl Default for ArEnvelope {
    fn default() -> Self {
        Self::new(48000.0, ArEnvelopeType::Ar, 10.0, 0.0, 20.0)
    }
}

impl ArEnvelope {
    /// Create a new AR/AHR envelope.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `envelope_type` - AR or AHR envelope
    /// * `attack_ms` - Attack time in milliseconds
    /// * `hold_ms` - Hold time in milliseconds (ignored for AR)
    /// * `release_ms` - Release time in milliseconds
    pub fn new(sample_rate: f32, envelope_type: ArEnvelopeType, attack_ms: f32, hold_ms: f32, release_ms: f32) -> Self {
        let attack_samples = (attack_ms * 0.001 * sample_rate).max(1.0) as u32;
        let hold_samples = (hold_ms * 0.001 * sample_rate).max(1.0) as u32;
        let release_samples = (release_ms * 0.001 * sample_rate).max(1.0) as u32;

        let (attack_target, hold_target, release_target) = match envelope_type {
            ArEnvelopeType::Ar => (1.0, 1.0, 0.0),
            ArEnvelopeType::Ahr => (1.0, 1.0, 0.0),
        };

        Self {
            state: ArEnvelopeState::Idle,
            current_value: 0.0,
            attack_samples,
            hold_samples,
            release_samples,
            step_counter: 0,
            attack_target,
            hold_target,
            release_target,
            attack_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                attack_ms,
            ),
            hold_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                hold_ms,
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

    /// Set hold time (smoothed)
    pub fn set_hold(&mut self, hold_ms: f32) {
        self.hold_smoother.set_target(hold_ms.max(0.0).min(5000.0));
    }

    /// Set release time (smoothed)
    pub fn set_release(&mut self, release_ms: f32) {
        self.release_smoother.set_target(release_ms.max(0.1).min(5000.0));
    }

    /// Set attack target value
    pub fn set_attack_target(&mut self, value: f32) {
        self.attack_target = value.clamp(-1.0, 1.0);
    }

    /// Set hold target value
    pub fn set_hold_target(&mut self, value: f32) {
        self.hold_target = value.clamp(-1.0, 1.0);
    }

    /// Set release target value
    pub fn set_release_target(&mut self, value: f32) {
        self.release_target = value.clamp(-1.0, 1.0);
    }

    /// Set output gain (smoothed)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.max(-24.0).min(24.0));
    }

    /// Trigger the envelope (start from beginning)
    pub fn trigger(&mut self) {
        self.state = ArEnvelopeState::Attack;
        self.step_counter = 0;
    }

    /// Get current value
    pub fn value(&mut self) -> f32 {
        self.current_value * self.output_gain_smoother.process_sample()
    }

    /// Get current state
    pub fn state(&self) -> ArEnvelopeState {
        self.state
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let _attack = self.attack_smoother.process_sample();
        let _hold = self.hold_smoother.process_sample();
        let _release = self.release_smoother.process_sample();

        let output_gain = self.output_gain_smoother.process_sample();

        match self.state {
            ArEnvelopeState::Idle => {
                self.state = ArEnvelopeState::Attack;
                self.step_counter = 0;
            }
            ArEnvelopeState::Attack => {
                if self.step_counter >= self.attack_samples {
                    self.state = ArEnvelopeState::Hold;
                    self.step_counter = 0;
                } else {
                    self.step_counter += 1;
                }

                // Linear attack curve
                let progress = self.step_counter as f32 / self.attack_samples as f32;
                self.current_value = self.attack_target * progress;
            }
            ArEnvelopeState::Hold => {
                if self.step_counter >= self.hold_samples {
                    self.state = ArEnvelopeState::Release;
                    self.step_counter = 0;
                } else {
                    self.step_counter += 1;
                }

                // Hold peak value
                self.current_value = self.hold_target;
            }
            ArEnvelopeState::Release => {
                if self.step_counter >= self.release_samples {
                    self.state = ArEnvelopeState::Done;
                    self.current_value = self.release_target;
                } else {
                    self.step_counter += 1;
                }

                // Linear release curve
                let progress = self.step_counter as f32 / self.release_samples as f32;
                self.current_value = self.hold_target * (1.0 - progress);
            }
            ArEnvelopeState::Done => {
                self.current_value = self.release_target;
            }
        }

        self.current_value * output_gain
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
    fn test_ar_envelope() {
        let sample_rate = 48_000.0;
        let mut envelope = ArEnvelope::new(sample_rate, ArEnvelopeType::Ar, 10.0, 0.0, 20.0);

        assert_eq!(envelope.value(), 0.0, "silent before a trigger");

        envelope.trigger();

        // A 10 ms attack at 48 kHz is 480 samples; the first half must climb off zero.
        let mut previous = 0.0;
        for _ in 0..240 {
            let value = envelope.process_sample(0.0);
            assert!(value.is_finite(), "envelope produced {value}");
            assert!(value >= previous - 1e-6, "attack must rise: {previous} -> {value}");
            previous = value;
        }

        assert!(previous > 0.0, "envelope never left silence");
    }

    #[test]
    fn test_ahr_envelope() {
        let mut envelope = ArEnvelope::new(48000.0, ArEnvelopeType::Ahr, 10.0, 50.0, 20.0);
        
        envelope.trigger();
        
        // Advance through attack: the hold stage must not break the climb.
        let mut previous = 0.0;
        for _ in 0..15 {
            let value = envelope.process_sample(0.0);
            assert!(value.is_finite(), "envelope produced {value}");
            assert!(value >= previous - 1e-6, "attack must rise: {previous} -> {value}");
            previous = value;
        }
        assert!(previous > 0.0, "envelope never left silence");
    }
}