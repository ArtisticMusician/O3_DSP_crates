use crate::synthesis::utils::Processor;

/// Nested allpass: `y[n] = -g * x[n] + x[n - D] + g * y[n - D]`.
///
/// Building block for Schroeder / Dattorro reverbs and phasers.
/// Delay line is allocated in `new()` only.
#[derive(Debug, Clone)]
pub struct AllpassFilter {
    buffer: Vec<f32>,
    mask: usize,
    write: usize,
    delay_samples: usize,
    gain: f32,
}

impl AllpassFilter {
    pub fn new(max_delay_samples: usize) -> Self {
        let capacity = max_delay_samples.max(2).next_power_of_two();
        Self {
            buffer: vec![0.0; capacity],
            mask: capacity - 1,
            write: 0,
            delay_samples: max_delay_samples.min(capacity - 1).max(1),
            gain: 0.5,
        }
    }

    pub fn set_delay_samples(&mut self, delay: usize) {
        self.delay_samples = delay.clamp(1, self.mask);
    }

    pub fn set_gain(&mut self, gain: f32) {
        self.gain = gain.clamp(-0.99, 0.99);
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    /// Get current delay in samples
    pub fn delay_samples(&self) -> usize {
        self.delay_samples
    }

    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let read = (self.write + self.buffer.len() - self.delay_samples) & self.mask;
        let buf = self.buffer[read];
        let y = -self.gain * input + buf;
        self.buffer[self.write] = input + self.gain * y;
        self.write = (self.write + 1) & self.mask;
        y
    }
}

impl Processor for AllpassFilter {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        self.process(sample)
    }
}
