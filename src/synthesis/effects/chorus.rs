use crate::synthesis::math_utils::TWO_PI;
use crate::synthesis::utils::Processor;

const VOICES: usize = 2;

/// Two-voice mono chorus: short modulated delay taps summed with the dry signal.
///
/// Delay memory is reserved in `new()`. LFO phase is advanced per sample with
/// a cheap sine approximation; no heap traffic on the audio thread.
#[derive(Debug, Clone)]
pub struct Chorus {
    buffer: Vec<f32>,
    mask: usize,
    write: usize,
    sample_rate: f32,
    phase: [f32; VOICES],
    phase_inc: [f32; VOICES],
    base_delay_samples: f32,
    depth_samples: f32,
    mix: f32,
    feedback: f32,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        // 40 ms is plenty for chorus / light flanging.
        let capacity = ((0.040 * sample_rate).ceil() as usize).max(16).next_power_of_two();
        let mut this = Self {
            buffer: vec![0.0; capacity],
            mask: capacity - 1,
            write: 0,
            sample_rate,
            phase: [0.0, 0.25],
            phase_inc: [0.0, 0.0],
            base_delay_samples: 0.012 * sample_rate,
            depth_samples: 0.004 * sample_rate,
            mix: 0.35,
            feedback: 0.0,
        };
        this.set_rate_hz(0.6);
        this
    }

    pub fn set_rate_hz(&mut self, rate_hz: f32) {
        let inc = (rate_hz.clamp(0.01, 12.0) / self.sample_rate).max(0.0);
        self.phase_inc[0] = inc;
        // Second voice slightly detuned so the image does not lock.
        self.phase_inc[1] = inc * 1.07;
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
    fn fast_sin(phase: f32) -> f32 {
        // Phase is [0, 1). Quarter-wave polynomial is plenty for LFO use.
        (phase * TWO_PI).sin()
    }

    #[inline(always)]
    fn read_at(&self, delay_samples: f32) -> f32 {
        let mut pos = self.write as f32 - delay_samples;
        let cap = self.buffer.len() as f32;
        while pos < 0.0 {
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
        for v in 0..VOICES {
            let lfo = Self::fast_sin(self.phase[v]);
            let delay = (self.base_delay_samples + lfo * self.depth_samples).max(1.0);
            wet += self.read_at(delay);
            self.phase[v] += self.phase_inc[v];
            if self.phase[v] >= 1.0 {
                self.phase[v] -= 1.0;
            }
        }
        wet *= 0.5;

        self.buffer[self.write] = input + wet * self.feedback;
        self.write = (self.write + 1) & self.mask;

        input * (1.0 - self.mix) + wet * self.mix
    }
}
