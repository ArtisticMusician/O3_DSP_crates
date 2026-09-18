use crate::synthesis::math_utils::TWO_PI;
use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CarrierShape {
    Sine,
    Triangle,
}

/// A Ring Modulator effect that multiplies the input signal by a carrier oscillator.
#[derive(Debug, Clone)]
pub struct RingModulator {
    sample_rate: f32,

    carrier_phase: f32,
    carrier_freq: ParameterSmoother,
    carrier_shape: CarrierShape,

    mix: ParameterSmoother,
}

impl RingModulator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,

            carrier_phase: 0.0,
            carrier_freq: ParameterSmoother::new(sample_rate, 10.0, 440.0),
            carrier_shape: CarrierShape::Sine,

            mix: ParameterSmoother::new(sample_rate, 10.0, 1.0),
        }
    }

    pub fn reset(&mut self) {
        self.carrier_phase = 0.0;
    }

    pub fn set_freq_hz(&mut self, freq_hz: f32) {
        // Nyquist limit approx
        let max_freq = self.sample_rate * 0.49;
        self.carrier_freq.set_target(freq_hz.clamp(0.01, max_freq));
    }

    pub fn set_carrier_shape(&mut self, shape: CarrierShape) {
        self.carrier_shape = shape;
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix.set_target(mix.clamp(0.0, 1.0));
    }

    #[inline(always)]
    fn get_carrier_sample(&mut self, freq_hz: f32) -> f32 {
        let val = match self.carrier_shape {
            CarrierShape::Sine => (self.carrier_phase * TWO_PI).sin(),
            CarrierShape::Triangle => {
                let mut tri = self.carrier_phase * 4.0;
                if tri > 2.0 {
                    tri = 4.0 - tri;
                }
                tri - 1.0
            }
        };

        self.carrier_phase += freq_hz / self.sample_rate;
        if self.carrier_phase >= 1.0 {
            self.carrier_phase -= 1.0;
        }

        val
    }

    /// Process a mono sample using the internal carrier oscillator
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        let freq_hz = self.carrier_freq.process_sample();
        let mix = self.mix.process_sample();

        let carrier = self.get_carrier_sample(freq_hz);
        let wet = input * carrier;

        input * (1.0 - mix) + wet * mix
    }

    /// Process a stereo sample using the internal carrier oscillator
    #[inline(always)]
    pub fn process_stereo(&mut self, left_in: f32, right_in: f32) -> (f32, f32) {
        let freq_hz = self.carrier_freq.process_sample();
        let mix = self.mix.process_sample();

        let carrier = self.get_carrier_sample(freq_hz);
        let wet_l = left_in * carrier;
        let wet_r = right_in * carrier;

        (
            left_in * (1.0 - mix) + wet_l * mix,
            right_in * (1.0 - mix) + wet_r * mix
        )
    }

    /// Process a mono sample using an external carrier
    #[inline(always)]
    pub fn process_with_carrier(&mut self, input: f32, carrier: f32) -> f32 {
        // Still need to step the smoothers
        self.carrier_freq.process_sample(); // Ignored in this mode
        let mix = self.mix.process_sample();

        let wet = input * carrier;
        input * (1.0 - mix) + wet * mix
    }

    /// Process a stereo sample using external carriers
    #[inline(always)]
    pub fn process_stereo_with_carrier(&mut self, left_in: f32, right_in: f32, carrier_l: f32, carrier_r: f32) -> (f32, f32) {
        // Still need to step the smoothers
        self.carrier_freq.process_sample(); // Ignored in this mode
        let mix = self.mix.process_sample();

        let wet_l = left_in * carrier_l;
        let wet_r = right_in * carrier_r;

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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dry_wet_pass_through() {
        let mut effect = RingModulator::new(44100.0);
        effect.set_mix(0.0);

        for _ in 0..10000 {
            effect.process(0.0);
        }

        let out = effect.process(1.0);
        assert!((out - 1.0).abs() < 1e-5);
    }

    #[test]
    fn test_zero_input_decay() {
        let mut effect = RingModulator::new(44100.0);
        effect.set_mix(1.0);

        for _ in 0..10000 {
            effect.process(0.0);
        }

        let out = effect.process(0.0);
        assert!(out.abs() < 1e-5);
    }

    #[test]
    fn test_external_carrier() {
        let mut effect = RingModulator::new(44100.0);
        effect.set_mix(1.0);

        for _ in 0..10000 {
            effect.process_with_carrier(0.0, 0.0);
        }

        // 0.5 * -0.5 = -0.25
        let out = effect.process_with_carrier(0.5, -0.5);
        assert!((out - (-0.25)).abs() < 1e-5);
    }
}
