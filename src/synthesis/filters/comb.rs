/// Comb filter with configurable delay and feedback.
/// Creates notches and peaks in the frequency response based on delay time.
///
/// Two variants:
/// - Feedforward (FIR): y[n] = x[n] + feedback * x[n - D]
/// - Feedback (IIR): y[n] = x[n] + feedback * y[n - D]
///
/// Use cases:
/// - Flanging (short delay, high feedback)
/// - Phasing (modulated delay)
/// - Space simulation (long delay, low feedback)
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
    /// Create a new comb filter.
    ///
    /// # Arguments
    /// * `max_delay_samples` - Maximum delay in samples (power of 2 recommended)
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

    /// Set delay in samples
    pub fn set_delay_samples(&mut self, delay: usize) {
        self.delay_samples = delay.clamp(1, self.mask);
    }

    /// Set feedback amount (-1.0 to 0.99 for stability)
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(-0.99, 0.99);
    }

    /// Set damping (0.0 = no damping, ~0.2-0.4 = room-like)
    pub fn set_damp(&mut self, damp: f32) {
        self.damp = damp.clamp(0.0, 0.99);
    }

    /// Clear the buffer
    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
        self.damp_state = 0.0;
    }

    /// Get current delay in samples
    pub fn delay_samples(&self) -> usize {
        self.delay_samples
    }

    /// Get current feedback
    pub fn feedback(&self) -> f32 {
        self.feedback
    }

    /// Process a single sample (feedback comb)
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let read = (self.write + self.buffer.len() - self.delay_samples) & self.mask;
        let buf = self.buffer[read];
        let y = input + self.feedback * buf;
        self.buffer[self.write] = input + self.feedback * y * (1.0 - self.damp);
        self.write = (self.write + 1) & self.mask;
        if self.damp > 0.0 {
            self.damp_state = self.damp_state * self.damp + buf * (1.0 - self.damp);
            self.buffer[self.write] += self.damp_state;
        }
        y
    }

    /// Process a single sample (feedforward comb)
    #[inline(always)]
    pub fn process_feedforward(&mut self, input: f32) -> f32 {
        let read = (self.write + self.buffer.len() - self.delay_samples) & self.mask;
        let delayed = self.buffer[read];
        self.buffer[self.write] = input;
        self.write = (self.write + 1) & self.mask;
        input + self.feedback * delayed
    }

    /// Process a block of samples
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process(*sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comb_filter() {
        let mut comb = CombFilter::new(64);
        
        comb.set_delay_samples(32);
        comb.set_feedback(0.5);
        
        let first = comb.process(1.0);
        assert!(first.abs() > 0.5, "impulse should pass through: {first}");

        // delay = 32, so the impulse returns every 32 samples and decays by the feedback.
        let mut echoes = [0.0f32; 3];
        for index in 0..(32 * 3) {
            let y = comb.process(0.0);
            if index % 32 == 31 {
                echoes[index / 32] = y;
            }
        }

        assert!(echoes[0].abs() > 1e-3, "no echo arrived at the delay time");
        assert!(
            echoes[1].abs() < echoes[0].abs() && echoes[2].abs() < echoes[1].abs(),
            "echoes must decay: {echoes:?}"
        );
    }
}