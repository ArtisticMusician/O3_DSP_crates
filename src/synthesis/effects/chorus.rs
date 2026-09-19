use crate::synthesis::math_utils::TWO_PI;
use crate::synthesis::utils::Processor;

/// Hard cap so voice state stays on the stack. `set_voices` picks how many run.
pub const MAX_CHORUS_VOICES: usize = 4;

/// Multi-voice mono chorus: short modulated delay taps summed with the dry signal.
///
/// Delay memory is reserved in `new()`. Voice count is 1..=MAX_CHORUS_VOICES.
#[derive(Debug, Clone)]
pub struct Chorus {
    buffer: Vec<f32>,
    mask: usize,
    write: usize,
    sample_rate: f32,
    voices: usize,
    phase: [f32; MAX_CHORUS_VOICES],
    phase_inc: [f32; MAX_CHORUS_VOICES],
    rate_hz: f32,
    base_delay_samples: f32,
    depth_samples: f32,
    mix: f32,
    feedback: f32,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        let capacity = ((0.040 * sample_rate).ceil() as usize)
            .max(16)
            .next_power_of_two();
        let mut this = Self {
            buffer: vec![0.0; capacity],
            mask: capacity - 1,
            write: 0,
            sample_rate,
            voices: 2,
            phase: [0.0, 0.25, 0.5, 0.75],
            phase_inc: [0.0; MAX_CHORUS_VOICES],
            rate_hz: 0.6,
            base_delay_samples: 0.012 * sample_rate,
            depth_samples: 0.004 * sample_rate,
            mix: 0.35,
            feedback: 0.0,
        };
        this.recompute_phase_inc();
        this
    }

    pub fn set_voices(&mut self, voices: usize) {
        self.voices = voices.clamp(1, MAX_CHORUS_VOICES);
        self.recompute_phase_inc();
    }

    pub fn voices(&self) -> usize {
        self.voices
    }

    pub fn set_rate_hz(&mut self, rate_hz: f32) {
        self.rate_hz = rate_hz.clamp(0.01, 12.0);
        self.recompute_phase_inc();
    }

    fn recompute_phase_inc(&mut self) {
        let base = self.rate_hz / self.sample_rate;
        for v in 0..MAX_CHORUS_VOICES {
            // Slight detune per voice so the image does not lock.
            self.phase_inc[v] = base * (1.0 + 0.07 * v as f32);
        }
    }

    pub fn set_depth_ms(&mut self, depth_ms: f32) {
        self.depth_samples = (depth_ms.clamp(0.0, 12.0) * 0.001 * self.sample_rate).max(0.0);
    }

    pub fn set_base_delay_ms(&mut self, delay_ms: f32) {
        let max_ms = (self.buffer.len() as f32 / self.sample_rate) * 1000.0 - 3.0;
        self.base_delay_samples = delay_ms.clamp(1.0, max_ms.max(1.0)) * 0.001 * self.sample_rate;
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback = feedback.clamp(-0.7, 0.7);
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write = 0;
    }

    #[inline(always)]
    fn read_at(&self, delay_samples: f32) -> f32 {
        let mut pos = self.write as f32 - delay_samples;
        let cap = self.buffer.len() as f32;
        if pos < 0.0 {
            pos += cap;
        }
        let i0 = pos.floor() as usize;
        let frac = pos - i0 as f32;
        let s0 = self.buffer[i0 & self.mask];
        let s1 = self.buffer[(i0 + 1) & self.mask];
        s0 + (s1 - s0) * frac
    }
}

impl Processor for Chorus {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let mut wet = 0.0;
        let n = self.voices;
        for v in 0..n {
            let lfo = (self.phase[v] * TWO_PI).sin();
            let delay = (self.base_delay_samples + lfo * self.depth_samples).clamp(1.0, self.buffer.len() as f32 - 3.0);
            wet += self.read_at(delay);
            self.phase[v] += self.phase_inc[v];
            if self.phase[v] >= 1.0 {
                self.phase[v] -= 1.0;
            }
        }
        wet *= 1.0 / n as f32;

        self.buffer[self.write] = input + wet * self.feedback;
        self.write = (self.write + 1) & self.mask;

        input * (1.0 - self.mix) + wet * self.mix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_huge_depth() {
        let mut effect = Chorus::new(44100.0);
        effect.set_base_delay_ms(100.0);
        effect.set_depth_ms(12.0);
        let out = effect.process_sample(0.5);
        assert!(!out.is_nan());
    }
}
