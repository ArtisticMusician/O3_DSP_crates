use crate::synthesis::utils::ParameterSmoother;

/// A simple delay line using a circular buffer.
/// Supports fractional delay via linear interpolation.
pub struct Delay {
    buffer: Vec<f32>,
    write_index: usize,
    sample_rate: f32,

    time_smoother: ParameterSmoother,
    feedback: f32,
    mix: f32,
}

impl Delay {
    pub fn new(sample_rate: f32, max_delay_ms: f32) -> Self {
        let max_samples = (max_delay_ms * 0.001 * sample_rate).ceil() as usize;
        // ensure minimum capacity and power of 2 is not strictly required here, just exact size
        let capacity = max_samples.max(1);

        Self {
            buffer: vec![0.0; capacity],
            write_index: 0,
            sample_rate,
            time_smoother: ParameterSmoother::new(sample_rate, 20.0, 500.0), // Default 500ms
            feedback: 0.5,
            mix: 0.5,
        }
    }

    pub fn set_parameters(&mut self, time_ms: f32, feedback: f32, mix: f32) {
        let max_ms = (self.buffer.len() as f32 / self.sample_rate) * 1000.0;
        self.time_smoother.set_target(time_ms.clamp(0.0, max_ms));
        self.feedback = feedback.clamp(-0.99, 0.99);
        self.mix = mix.clamp(0.0, 1.0);
    }

    #[inline(always)]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let delay_ms = self.time_smoother.process_sample();
        let delay_samples = delay_ms * 0.001 * self.sample_rate;

        let capacity = self.buffer.len() as f32;
        let mut read_pos = (self.write_index as f32) - delay_samples;

        // Wrap read pos
        if read_pos < 0.0 {
            read_pos += capacity;
        }

        // Linear interpolation for fractional delay
        let index1 = read_pos.floor() as usize;
        let frac = read_pos - index1 as f32;
        let index2 = if index1 + 1 >= self.buffer.len() { 0 } else { index1 + 1 };

        let s1 = self.buffer[index1];
        let s2 = self.buffer[index2];
        let delayed_sample = s1 + (s2 - s1) * frac;

        // Calculate new sample to write
        let to_write = input + delayed_sample * self.feedback;

        // Write to buffer
        self.buffer[self.write_index] = to_write;
        self.write_index += 1;
        if self.write_index >= self.buffer.len() {
            self.write_index = 0;
        }

        // Mix
        input * (1.0 - self.mix) + delayed_sample * self.mix
    }

    #[inline]
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
    }
}
