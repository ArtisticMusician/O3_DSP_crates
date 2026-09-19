use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

/// A digital degradation effect supporting bit-depth truncation and sample-rate reduction.
#[derive(Debug, Clone)]
pub struct Bitcrusher {
    // Discrete parameters (not smoothed)
    scale: f32,
    downsample_factor: usize,

    // State
    hold_phase: usize,
    held_sample_l: f32,
    held_sample_r: f32,

    // Smoothed continuous parameter
    mix: ParameterSmoother,
}

impl Bitcrusher {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            scale: 2_f32.powf(16.0 - 1.0),
            downsample_factor: 1, // No downsampling by default (1x)

            hold_phase: 0,
            held_sample_l: 0.0,
            held_sample_r: 0.0,

            mix: ParameterSmoother::new(sample_rate, 10.0, 1.0),
        }
    }

    pub fn reset(&mut self) {
        self.hold_phase = 0;
        self.held_sample_l = 0.0;
        self.held_sample_r = 0.0;
    }

    pub fn set_bits(&mut self, bits: f32) {
        self.scale = 2_f32.powf(bits.clamp(2.0, 16.0) - 1.0);
    }

    pub fn set_downsample_factor(&mut self, factor: usize) {
        self.downsample_factor = factor.clamp(1, 512);
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix.set_target(mix.clamp(0.0, 1.0));
    }

    #[inline(always)]
    fn quantize(sample: f32, scale: f32) -> f32 {
        (sample * scale).round() / scale
    }

    /// Process a mono sample
    /// Process a mono sample. Note: do not call `process` and `process_stereo` in the same callback.
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let mix = self.mix.process_sample();

        // Zero-order hold downsampling
        if self.hold_phase == 0 {
            // New sample time: apply bit crushing
            self.held_sample_l = Self::quantize(input, self.scale);
        }

        self.hold_phase += 1;
        if self.hold_phase >= self.downsample_factor {
            self.hold_phase = 0;
        }

        let wet = self.held_sample_l;
        input * (1.0 - mix) + wet * mix
    }

    /// Process a stereo sample
    #[inline(always)]
    pub fn process_stereo(&mut self, left_in: f32, right_in: f32) -> (f32, f32) {
        let mix = self.mix.process_sample();

        // Zero-order hold downsampling
        if self.hold_phase == 0 {
            self.held_sample_l = Self::quantize(left_in, self.scale);
            self.held_sample_r = Self::quantize(right_in, self.scale);
        }

        self.hold_phase += 1;
        if self.hold_phase >= self.downsample_factor {
            self.hold_phase = 0;
        }

        let wet_l = self.held_sample_l;
        let wet_r = self.held_sample_r;

        (
            left_in * (1.0 - mix) + wet_l * mix,
            right_in * (1.0 - mix) + wet_r * mix
        )
    }

    #[inline]
    pub fn process_block(&mut self, input: &[f32], output: &mut [f32]) {
        for (i, o) in input.iter().zip(output.iter_mut()) {
            *o = self.process(*i);
        }
    }

    #[inline]
    pub fn process_block_stereo(&mut self, left_in: &[f32], right_in: &[f32], left_out: &mut [f32], right_out: &mut [f32]) {
        for (((l_in, r_in), l_out), r_out) in left_in.iter().zip(right_in.iter()).zip(left_out.iter_mut()).zip(right_out.iter_mut()) {
            let (l, r) = self.process_stereo(*l_in, *r_in);
            *l_out = l;
            *r_out = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dry_wet_pass_through() {
        let mut effect = Bitcrusher::new(44100.0);
        effect.set_mix(0.0);

        for _ in 0..10000 {
            effect.process(0.0);
        }

        let out = effect.process(0.42);
        assert!((out - 0.42).abs() < 1e-5);
    }

    #[test]
    fn test_zero_input_decay() {
        let mut effect = Bitcrusher::new(44100.0);
        effect.set_mix(1.0);
        effect.set_bits(16.0);

        for _ in 0..10000 {
            effect.process(0.0);
        }

        let out = effect.process(0.0);
        assert!(out.abs() < 1e-5);
    }

    #[test]
    fn test_downsampling() {
        let mut effect = Bitcrusher::new(44100.0);
        effect.set_mix(1.0);
        effect.set_bits(16.0);
        effect.set_downsample_factor(3);

        for _ in 0..10000 {
            effect.process(0.0);
        }

        effect.reset();

        // Hold phase 0 -> takes new sample
        let s1 = effect.process(0.5);
        // Hold phase 1 -> holds old sample
        let s2 = effect.process(0.2);
        // Hold phase 2 -> holds old sample
        let s3 = effect.process(0.8);
        // Hold phase 0 -> takes new sample
        let s4 = effect.process(0.9);

        assert!((s1 - 0.5).abs() < 1e-4);
        assert!((s2 - 0.5).abs() < 1e-4);
        assert!((s3 - 0.5).abs() < 1e-4);
        assert!((s4 - 0.9).abs() < 1e-4);
    }
}
