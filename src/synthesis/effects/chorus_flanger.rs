use crate::synthesis::math_utils::TWO_PI;
use crate::synthesis::utils::hermite_interpolator::HermiteInterpolator;
use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

/// Modulated delay line used for Chorus and Flanger effects.
#[derive(Debug, Clone)]
pub struct ChorusFlanger {
    buffer_left: Vec<f32>,
    buffer_right: Vec<f32>,
    mask: usize,
    write_pos: usize,
    max_delay_samples: f32,

    sample_rate: f32,

    lfo_phase: f32,
    lfo_phase_offset_right: f32,

    rate_hz: ParameterSmoother,
    depth_samples: ParameterSmoother,
    base_delay_samples: ParameterSmoother,
    feedback: ParameterSmoother,
    mix: ParameterSmoother,
}

impl ChorusFlanger {
    pub fn new(sample_rate: f32) -> Self {
        // Pre-allocate up to 50ms of delay buffer (plus some safety margin)
        let capacity = ((0.055 * sample_rate).ceil() as usize).max(32).next_power_of_two();
        let max_delay_samples = capacity as f32 - 3.0;

        Self {
            max_delay_samples,
            buffer_left: vec![0.0; capacity],
            buffer_right: vec![0.0; capacity],
            mask: capacity - 1,
            write_pos: 0,

            sample_rate,

            lfo_phase: 0.0,
            lfo_phase_offset_right: 0.5, // 180 degrees offset by default

            rate_hz: ParameterSmoother::new(sample_rate, 10.0, 1.0),
            depth_samples: ParameterSmoother::new(sample_rate, 10.0, 0.005 * sample_rate),
            base_delay_samples: ParameterSmoother::new(sample_rate, 10.0, 0.010 * sample_rate),
            feedback: ParameterSmoother::new(sample_rate, 10.0, 0.0),
            mix: ParameterSmoother::new(sample_rate, 10.0, 0.5),
        }
    }

    pub fn reset(&mut self) {
        self.buffer_left.fill(0.0);
        self.buffer_right.fill(0.0);
        self.write_pos = 0;
        self.lfo_phase = 0.0;
    }

    pub fn set_rate_hz(&mut self, rate_hz: f32) {
        self.rate_hz.set_target(rate_hz.clamp(0.01, 20.0));
    }

    pub fn set_depth_ms(&mut self, depth_ms: f32) {
        let depth_samples = (depth_ms * 0.001 * self.sample_rate).max(0.0);
        let target_base = self.base_delay_samples.current_value();
        let safe_depth = depth_samples.max(0.0).min(self.max_delay_samples - target_base).max(0.0);
        self.depth_samples.set_target(safe_depth);
        // Re-clamp base just in case
        let safe_base = target_base.clamp(2.0, self.max_delay_samples - safe_depth);
        self.base_delay_samples.set_target(safe_base);
    }

    pub fn set_base_delay_ms(&mut self, delay_ms: f32) {
        let base_delay_samples = (delay_ms * 0.001 * self.sample_rate).clamp(2.0, self.max_delay_samples);
        let target_depth = self.depth_samples.current_value();
        let safe_base = base_delay_samples.clamp(2.0, self.max_delay_samples - target_depth);
        self.base_delay_samples.set_target(safe_base);
        // Re-clamp depth just in case
        let safe_depth = target_depth.max(0.0).min(self.max_delay_samples - safe_base).max(0.0);
        self.depth_samples.set_target(safe_depth);
    }

    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback.set_target(feedback.clamp(-0.70, 0.70));
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix.set_target(mix.clamp(0.0, 1.0));
    }

    pub fn set_lfo_phase_offset_right(&mut self, offset: f32) {
        self.lfo_phase_offset_right = offset.clamp(0.0, 1.0);
    }

    #[inline(always)]
    fn read_interpolated(buffer: &[f32], mask: usize, write_pos: usize, delay_samples: f32) -> f32 {
        let mut pos = write_pos as f32 - delay_samples;
        let cap = buffer.len() as f32;
        if pos < 0.0 {
            pos += cap;
        }

        let i = pos.floor() as usize;
        let frac = pos - i as f32;

        // Hermite interpolation mapping:
        // We need 4 points: y0 (i-1), y1 (i), y2 (i+1), y3 (i+2)
        // Note: write_pos moves forward, so older samples are "behind"
        let i0 = (i.wrapping_sub(1)) & mask;
        let i1 = i & mask;
        let i2 = (i + 1) & mask;
        let i3 = (i + 2) & mask;

        let y0 = buffer[i0];
        let y1 = buffer[i1];
        let y2 = buffer[i2];
        let y3 = buffer[i3];

        HermiteInterpolator::interpolate(y0, y1, y2, y3, frac)
    }



    #[inline(always)]
    pub fn process(&mut self, left_in: f32, right_in: f32) -> (f32, f32) {
        // Smooth parameters
        let rate_hz = self.rate_hz.process_sample();
        let depth_samples = self.depth_samples.process_sample();
        let base_delay_samples = self.base_delay_samples.process_sample();
        let feedback = self.feedback.process_sample();
        let mix = self.mix.process_sample();

        // Update LFO
        let lfo_val_left = (self.lfo_phase * TWO_PI).sin();
        let phase_right = (self.lfo_phase + self.lfo_phase_offset_right).fract();
        let lfo_val_right = (phase_right * TWO_PI).sin();

        self.lfo_phase += rate_hz / self.sample_rate;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }

        // Calculate delay times (minimum 1.0 sample to prevent looking into the future)
        let delay_left = (base_delay_samples + lfo_val_left * depth_samples).clamp(2.0, self.max_delay_samples);
        let delay_right = (base_delay_samples + lfo_val_right * depth_samples).clamp(2.0, self.max_delay_samples);

        // Read from delay lines
        let wet_left = Self::read_interpolated(&self.buffer_left, self.mask, self.write_pos, delay_left);
        let wet_right = Self::read_interpolated(&self.buffer_right, self.mask, self.write_pos, delay_right);

        // Prevent denormals
        let wet_left = wet_left + 1e-25;
        let wet_right = wet_right + 1e-25;

        // Write to delay lines with feedback
        self.buffer_left[self.write_pos] = left_in + wet_left * feedback;
        self.buffer_right[self.write_pos] = right_in + wet_right * feedback;

        self.write_pos = (self.write_pos + 1) & self.mask;

        // Mix dry/wet
        let out_left = left_in * (1.0 - mix) + wet_left * mix;
        let out_right = right_in * (1.0 - mix) + wet_right * mix;

        (out_left, out_right)
    }

    #[inline]
    pub fn process_block(&mut self, left_in: &[f32], right_in: &[f32], left_out: &mut [f32], right_out: &mut [f32]) {
        for (((l_in, r_in), l_out), r_out) in left_in.iter().zip(right_in.iter()).zip(left_out.iter_mut()).zip(right_out.iter_mut()) {
            let (l, r) = self.process(*l_in, *r_in);
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
        let mut effect = ChorusFlanger::new(44100.0);
        effect.set_mix(0.0);
        effect.set_feedback(0.0);

        // Process a few times so the mix parameter smoothes to 0
        for _ in 0..10000 {
            effect.process(0.0, 0.0);
        }

        let (l, r) = effect.process(1.0, -1.0);
        assert!((l - 1.0).abs() < 1e-5);
        assert!((r - (-1.0)).abs() < 1e-5);
    }

    #[test]
    fn test_zero_input_decay() {
        let mut effect = ChorusFlanger::new(44100.0);
        effect.set_mix(1.0);
        effect.set_feedback(0.5);

        for _ in 0..10000 {
            effect.process(0.0, 0.0);
        }

        // Feed an impulse
        effect.process(1.0, 1.0);

        // Wait for it to decay out completely
        for _ in 0..100000 {
            effect.process(0.0, 0.0);
        }

        let (l, r) = effect.process(0.0, 0.0);
        assert!(l.abs() < 1e-5);
        assert!(r.abs() < 1e-5);
    }

    #[test]
    fn test_parameter_limits() {
        let mut effect = ChorusFlanger::new(44100.0);
        effect.set_feedback(1.5);

        for _ in 0..10000 {
            effect.process(0.0, 0.0);
        }

        // Ensure feedback was clamped
        assert!(effect.feedback.current_value() <= 0.70);
    }

    #[test]
    fn test_huge_depth() {
        let mut effect = ChorusFlanger::new(44100.0);
        effect.set_depth_ms(10_000.0);
        for _ in 0..1000 {
            let (l, r) = effect.process(0.5, -0.5);
            assert!(!l.is_nan() && !r.is_nan());
        }
    }
}
