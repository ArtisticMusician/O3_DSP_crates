use crate::synthesis::utils::{ParameterSmoother, Processor};
use crate::synthesis::utils::HermiteInterpolator;

/// A simple Mono delay line using a circular buffer.
/// Supports fractional delay via Hermite interpolation.
pub struct MonoFeedbackDelay {
    buffer: Vec<f32>,
    write_index: usize,
    sample_rate: f32,

    time_smoother: ParameterSmoother,
    feedback: f32,
    mix: f32,

    interpolator: HermiteInterpolator,
}

impl MonoFeedbackDelay {
    pub fn new(sample_rate: f32, max_delay_ms: f32) -> Self {
        let max_samples = (max_delay_ms * 0.001 * sample_rate).ceil() as usize;
        // ensure minimum capacity and power of 2 for easy wrap
        let capacity = max_samples.max(4).next_power_of_two();

        Self {
            buffer: vec![0.0; capacity],
            write_index: 0,
            sample_rate,
            time_smoother: ParameterSmoother::new(sample_rate, 20.0, 500.0), // Default 500ms
            feedback: 0.5,
            mix: 0.5,
            interpolator: HermiteInterpolator::new(),
        }
    }

    pub fn set_parameters(&mut self, time_ms: f32, feedback: f32, mix: f32) {
        let max_ms = (self.buffer.len() as f32 / self.sample_rate) * 1000.0;
        self.time_smoother.set_target(time_ms.clamp(0.0, max_ms - 2.0)); // keep 2ms margin for interpolation
        self.feedback = feedback.clamp(-0.99, 0.99);
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
    }
}

impl Processor for MonoFeedbackDelay {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let delay_ms = self.time_smoother.process_sample();
        let delay_samples = delay_ms * 0.001 * self.sample_rate;

        let capacity = self.buffer.len();
        let mask = capacity - 1;

        let mut read_pos = (self.write_index as f32) - delay_samples;

        // Wrap read pos
        while read_pos < 0.0 {
            read_pos += capacity as f32;
        }

        let index1 = read_pos.floor() as usize;
        let frac = read_pos - index1 as f32;

        // Push 4 samples into interpolator for Hermite
        for i in 0..4 {
            // Need samples around the read point
            // For x = index1, we want indices: x-1, x, x+1, x+2
            let mut idx = (index1 as isize) + (i as isize) - 1;
            while idx < 0 {
                idx += capacity as isize;
            }
            let safe_idx = (idx as usize) & mask;
            self.interpolator.push_sample(self.buffer[safe_idx]);
        }

        let delayed_sample = self.interpolator.process_sample(frac);

        // Calculate new sample to write
        let to_write = input + delayed_sample * self.feedback;

        // Write to buffer
        self.buffer[self.write_index] = to_write;
        self.write_index = (self.write_index + 1) & mask;

        // Mix
        input * (1.0 - self.mix) + delayed_sample * self.mix
    }
}
