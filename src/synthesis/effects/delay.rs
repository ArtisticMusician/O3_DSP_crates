use crate::synthesis::utils::{HermiteInterpolator, ParameterSmoother, Processor};

/// Mono feedback delay with fractional read via 4-point Hermite interpolation.
pub struct MonoFeedbackDelay {
    buffer: Vec<f32>,
    write_index: usize,
    sample_rate: f32,
    time_smoother: ParameterSmoother,
    feedback: f32,
    mix: f32,
}

impl MonoFeedbackDelay {
    pub fn new(sample_rate: f32, max_delay_ms: f32) -> Self {
        let max_samples = (max_delay_ms * 0.001 * sample_rate).ceil() as usize;
        let capacity = max_samples.max(8).next_power_of_two();

        Self {
            buffer: vec![0.0; capacity],
            write_index: 0,
            sample_rate,
            time_smoother: ParameterSmoother::new(sample_rate, 20.0, 250.0),
            feedback: 0.5,
            mix: 0.5,
        }
    }

    pub fn set_parameters(&mut self, time_ms: f32, feedback: f32, mix: f32) {
        let max_ms = (self.buffer.len() as f32 / self.sample_rate) * 1000.0;
        self.time_smoother
            .set_target(time_ms.clamp(0.1, (max_ms - 4.0).max(0.1)));
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

        let read = self.write_index as f32 - delay_samples;
        let read_wrapped = read.rem_euclid(capacity as f32);
        let index1 = read_wrapped as usize;
        let frac = read_wrapped - index1 as f32;

        let y0 = self.buffer[index1.wrapping_sub(1) & mask];
        let y1 = self.buffer[index1 & mask];
        let y2 = self.buffer[(index1 + 1) & mask];
        let y3 = self.buffer[(index1 + 2) & mask];
        let delayed = HermiteInterpolator::interpolate(y0, y1, y2, y3, frac);

        self.buffer[self.write_index] = input + delayed * self.feedback;
        self.write_index = (self.write_index + 1) & mask;

        input * (1.0 - self.mix) + delayed * self.mix
    }
}
