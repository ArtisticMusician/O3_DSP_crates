use crate::synthesis::utils::Processor;

/// Feedback comb filter with a power-of-two delay line.
///
/// `y[n] = x[n] + feedback * lp(y[n - D])`
/// `damp` is a one-pole in the feedback path (0 = no damping).
/// Allocate the line in `new()`; the process path is allocation-free.
#[derive(Debug, Clone)]
pub struct CombFilter {
    buffer: Vec<f32>,
    mask: usize,
    write: usize,
    delay_samples: usize,
    feedback: f32,
    damp: f32,
    damp_state: f32,
}

impl CombFilter {
    pub fn new(max_delay_samples: usize) -> Self {
        let capacity = max_delay_samples.max(2).next_power_of_two();
        Self {
            buffer: vec![0.0; capacity],
            mask: capacity - 1,
            write: 0,
            delay_samples: max_delay_samples.min(capacity - 1).max(1),
            feedback: 0.5,
            damp: 0.0,
            damp_state: 0.0,
        }
    }

    pub fn set_delay_samples(&mut self, delay: usize) {
        self.delay_samples = delay.clamp(1, self.mask);
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(-0.99, 0.99);
    }

    /// One-pole damping in the feedback loop. 0 = bright, ~0.2–0.4 = room-like.
    pub fn set_damp(&mut self, damp: f32) {
        self.damp = damp.clamp(0.0, 0.99);
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
        self.damp_state = 0.0;
    }

    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let read = (self.write + self.buffer.len() - self.delay_samples) & self.mask;
        let delayed = self.buffer[read];
        self.damp_state += self.damp * (delayed - self.damp_state);
        let y = input + self.damp_state * self.feedback;
        self.buffer[self.write] = y;
        self.write = (self.write + 1) & self.mask;
        y
    }
}

impl Processor for CombFilter {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        self.process(sample)
    }
}
