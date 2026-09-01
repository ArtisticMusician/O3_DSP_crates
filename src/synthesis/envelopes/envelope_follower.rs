/// Envelope Follower for dynamic processing (Compressors, Gates, Auto-wah).
/// Tracks the amplitude envelope of an incoming audio signal.
#[derive(Debug, Clone)]
pub struct EnvelopeFollower {
    sample_rate: f32,
    envelope: f32,
    attack_coef: f32,
    release_coef: f32,
}

impl EnvelopeFollower {
    pub fn new(sample_rate: f32) -> Self {
        let mut follower = Self {
            sample_rate,
            envelope: 0.0,
            attack_coef: 0.0,
            release_coef: 0.0,
        };
        follower.set_attack_release(10.0, 100.0);
        follower
    }

    /// Set attack and release times in milliseconds.
    pub fn set_attack_release(&mut self, attack_ms: f32, release_ms: f32) {
        self.attack_coef = Self::calc_coef(attack_ms, self.sample_rate);
        self.release_coef = Self::calc_coef(release_ms, self.sample_rate);
    }

    #[inline(always)]
    fn calc_coef(time_ms: f32, sample_rate: f32) -> f32 {
        if time_ms <= 0.0 {
            0.0
        } else {
            (-1.0 / (time_ms * 0.001 * sample_rate)).exp()
        }
    }

    /// Process a single sample and return the tracked envelope level.
    #[inline(always)]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let abs_input = input.abs();

        // Attack or Release phase based on current envelope value
        let coef = if abs_input > self.envelope {
            self.attack_coef
        } else {
            self.release_coef
        };

        self.envelope = abs_input + coef * (self.envelope - abs_input);

        // Prevent denormals
        if self.envelope < 1e-15 {
            self.envelope = 0.0;
        }

        self.envelope
    }

    /// Returns current envelope level without advancing the state.
    pub fn current_envelope(&self) -> f32 {
        self.envelope
    }

    /// Process a block of samples and output the envelope array.
    #[inline]
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        let min_len = input.len().min(output.len());
        for i in 0..min_len {
            output[i] = self.process_sample(input[i]);
        }
    }
}
