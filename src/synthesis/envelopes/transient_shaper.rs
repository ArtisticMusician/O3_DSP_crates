use crate::synthesis::envelopes::envelope_follower::EnvelopeFollower;
use crate::synthesis::utils::Processor;

/// Transient Shaper based on dual envelope followers.
/// Emphasizes or suppresses the attack and sustain portions of a signal.
pub struct TransientShaper {
    fast_follower: EnvelopeFollower,
    slow_follower: EnvelopeFollower,

    pub attack_gain: f32, // Linear gain > 1.0 (boost), < 1.0 (cut)
    pub sustain_gain: f32, // Linear gain > 1.0 (boost), < 1.0 (cut)
}

impl TransientShaper {
    pub fn new(sample_rate: f32) -> Self {
        let mut fast = EnvelopeFollower::new(sample_rate);
        fast.set_attack_release(1.0, 50.0);

        let mut slow = EnvelopeFollower::new(sample_rate);
        slow.set_attack_release(15.0, 150.0);

        Self {
            fast_follower: fast,
            slow_follower: slow,
            attack_gain: 1.0,
            sustain_gain: 1.0,
        }
    }

    pub fn set_parameters(&mut self, attack_db: f32, sustain_db: f32) {
        // Convert dB to linear gain
        self.attack_gain = 10.0_f32.powf(attack_db / 20.0);
        self.sustain_gain = 10.0_f32.powf(sustain_db / 20.0);
    }
}

impl Processor for TransientShaper {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let fast_env = self.fast_follower.process_sample(input);
        let slow_env = self.slow_follower.process_sample(input);

        // Attack signal is when fast env > slow env
        let attack_diff = (fast_env - slow_env).max(0.0);

        // Sustain signal is when slow env > fast env
        let sustain_diff = (slow_env - fast_env).max(0.0);

        // Compute gain modifier
        let env_max = fast_env.max(slow_env).max(1e-5); // prevent div by zero

        let attack_mod = (attack_diff / env_max) * (self.attack_gain - 1.0);
        let sustain_mod = (sustain_diff / env_max) * (self.sustain_gain - 1.0);

        let total_gain = (1.0 + attack_mod + sustain_mod).max(0.0);

        input * total_gain
    }
}
