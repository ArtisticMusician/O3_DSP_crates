
/// Step sequencer for generating rhythmic modulation patterns.
/// Creates a stepped signal that cycles through a sequence of values.
///
/// Use cases:
/// - Rhythmic LFO patterns
/// - Arpeggiator control
/// - Gate/trig sequencing
/// - Mod matrix step control
#[derive(Debug, Clone)]
pub struct StepSequencer {

    // Sequence values
    sequence: Vec<f32>,
    current_step: usize,

    // Step timing (in samples)

    // Smoothed parameters
    step_duration_smoother: crate::synthesis::utils::ParameterSmoother,
    output_gain_smoother: crate::synthesis::utils::ParameterSmoother,

    // Phase accumulator
    phase: f32,
}

impl Default for StepSequencer {
    fn default() -> Self {
        Self::new(48000.0, vec![0.0; 8], 16.0) // 8 steps, 16 samples per step
    }
}

impl StepSequencer {
    /// Create a new step sequencer.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `sequence` - Sequence of values (length determines number of steps)
    /// * `step_duration_ms` - Duration of each step in milliseconds
    pub fn new(sample_rate: f32, sequence: Vec<f32>, step_duration_ms: f32) -> Self {
        Self {
            sequence,
            current_step: 0,
            step_duration_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                step_duration_ms,
            ),
            output_gain_smoother: crate::synthesis::utils::ParameterSmoother::new(
                sample_rate,
                10.0,
                1.0,
            ),
            phase: 0.0,
        }
    }

    /// Set the sequence values
    pub fn set_sequence(&mut self, sequence: Vec<f32>) {
        self.sequence = sequence;
        self.current_step = 0;
    }

    /// Set step duration (smoothed)
    pub fn set_step_duration(&mut self, duration_ms: f32) {
        self.step_duration_smoother.set_target(duration_ms.max(1.0).min(5000.0));
    }

    /// Set output gain (smoothed)
    pub fn set_output_gain(&mut self, gain: f32) {
        self.output_gain_smoother.set_target(gain.max(-24.0).min(24.0));
    }

    /// Get current step value
    pub fn value(&mut self) -> f32 {
        if self.sequence.is_empty() {
            return 0.0;
        }

        self.sequence[self.current_step] * self.output_gain_smoother.process_sample()
    }

    /// Set a specific step value
    pub fn set_step(&mut self, step: usize, value: f32) {
        if step < self.sequence.len() {
            self.sequence[step] = value;
        }
    }

    /// Get current step
    pub fn current_step(&self) -> usize {
        self.current_step
    }

    /// Get number of steps
    pub fn num_steps(&self) -> usize {
        self.sequence.len()
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let step_duration = self.step_duration_smoother.process_sample();
        let output_gain = self.output_gain_smoother.process_sample();

        // An empty sequence is silence, not a division by zero.
        if self.sequence.is_empty() {
            return 0.0;
        }

        // Update step counter
        self.phase += 1.0;
        if self.phase >= step_duration {
            self.phase = 0.0;
            self.current_step = (self.current_step + 1) % self.sequence.len();
        }

        self.sequence[self.current_step] * output_gain
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
    fn empty_sequence_is_silent() {
        let mut sequencer = StepSequencer::new(48_000.0, Vec::new(), 16.0);

        for _ in 0..512 {
            assert_eq!(sequencer.process_sample(0.0), 0.0);
        }
        assert_eq!(sequencer.value(), 0.0);
        assert_eq!(sequencer.num_steps(), 0);
    }

    #[test]
    fn test_step_sequencer() {
        let mut sequencer = StepSequencer::new(48000.0, vec![0.0, 1.0, 0.0, 1.0], 16.0);
        
        let step_samples = (0.016 * 48_000.0) as usize;
        let (mut seen_low, mut seen_high) = (false, false);

        for _ in 0..(step_samples * 4) {
            let value = sequencer.process_sample(0.0);
            assert!(value.is_finite(), "sequencer produced {value}");
            if value < 0.5 {
                seen_low = true;
            } else {
                seen_high = true;
            }
        }

        assert!(seen_low && seen_high, "sequence never alternated");
        assert_eq!(sequencer.num_steps(), 4);
    }

    #[test]
    fn test_step_values() {
        let mut sequencer = StepSequencer::new(48000.0, vec![1.0, 2.0, 3.0, 4.0], 16.0);
        
        // One full cycle must visit all four steps.
        let step_samples = (0.016 * 48_000.0) as usize;
        let mut visited = Vec::new();

        for _ in 0..(step_samples * 4) {
            sequencer.process_sample(0.0);
            let step = sequencer.current_step();
            if visited.last() != Some(&step) {
                visited.push(step);
            }
        }

        visited.sort_unstable();
        visited.dedup();
        assert_eq!(visited.len(), 4, "expected 4 distinct steps, saw {visited:?}");
    }
}