use crate::synthesis::filters::{AllpassBank, BankMode, CombBank};
use crate::synthesis::utils::Processor;

/// Classic Schroeder tank: 4 parallel damped combs → 2 series allpasses.
///
/// Delay lengths are Freeverb's 44.1 kHz set, scaled to the host rate.
/// All delay memory is reserved in `new()`.
#[derive(Debug, Clone)]
pub struct SchroederReverb {
    combs: CombBank,
    allpasses: AllpassBank,
    mix: f32,
    sample_rate: f32,
}

// Freeverb comb / allpass delays at 44.1 kHz.
const COMB_DELAYS_441: [usize; 4] = [1116, 1188, 1277, 1356];
const ALLPASS_DELAYS_441: [usize; 2] = [556, 441];

fn scaled_delay(delay_at_441: usize, scale: f32) -> usize {
    let samples = ((delay_at_441 as f32) * scale).round() as usize;
    samples.max(2)
}

impl SchroederReverb {
    pub fn new(sample_rate: f32) -> Self {
        let scale = sample_rate.max(1.0) / 44_100.0;

        let comb_specs = [
            (scaled_delay(COMB_DELAYS_441[0], scale), 0.84),
            (scaled_delay(COMB_DELAYS_441[1], scale), 0.84),
            (scaled_delay(COMB_DELAYS_441[2], scale), 0.84),
            (scaled_delay(COMB_DELAYS_441[3], scale), 0.84),
        ];
        let ap_specs = [
            (scaled_delay(ALLPASS_DELAYS_441[0], scale), 0.5),
            (scaled_delay(ALLPASS_DELAYS_441[1], scale), 0.5),
        ];

        let mut combs = CombBank::new(&comb_specs, BankMode::Parallel);
        combs.set_damp_all(0.2);

        Self {
            combs,
            allpasses: AllpassBank::new(&ap_specs, BankMode::Series),
            mix: 0.25,
            sample_rate,
        }
    }

    /// Room size → comb feedback. 0 = short tank, 1 = long decay (~0.93).
    pub fn set_room_size(&mut self, size: f32) {
        let fb = 0.7 + 0.23 * size.clamp(0.0, 1.0);
        self.combs.set_feedback_all(fb);
    }

    /// HF damping in the comb loop. 0 = bright, 1 = dark.
    pub fn set_damping(&mut self, damp: f32) {
        self.combs.set_damp_all(damp.clamp(0.0, 1.0) * 0.4);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn clear(&mut self) {
        self.combs.clear();
        self.allpasses.clear();
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    pub fn comb_count(&self) -> usize {
        self.combs.len()
    }

    pub fn allpass_count(&self) -> usize {
        self.allpasses.len()
    }
}

impl Processor for SchroederReverb {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let tank = self.allpasses.process(self.combs.process(input));
        input * (1.0 - self.mix) + tank * self.mix
    }
}

#[cfg(test)]
mod tests {
    use super::SchroederReverb;
    use crate::synthesis::utils::Processor;

    #[test]
    fn tank_has_four_combs_and_two_allpasses() {
        let r = SchroederReverb::new(48_000.0);
        assert_eq!(r.comb_count(), 4);
        assert_eq!(r.allpass_count(), 2);
    }

    #[test]
    fn impulse_decays() {
        let mut r = SchroederReverb::new(48_000.0);
        r.set_mix(1.0);
        let _ = r.process_sample(1.0);
        let mut last = 1.0f32;
        for _ in 0..16 {
            let mut acc = 0.0;
            for _ in 0..512 {
                acc += r.process_sample(0.0).abs();
            }
            assert!(acc <= last * 3.0 + 1e-2, "energy grew: {acc} vs {last}");
            last = acc;
        }
    }
}
