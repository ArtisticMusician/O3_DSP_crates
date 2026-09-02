use crate::synthesis::utils::{ParameterSmoother, Processor};
use std::f32::consts::TAU as TWO_PI;

/// Biquad filter coefficients for various filter types.
/// Real-time safe, parameter smoothed.
#[derive(Debug, Clone, Copy)]
pub struct BiquadCoefficients {
    pub a0: f32,
    pub a1: f32,
    pub a2: f32,
    pub b1: f32,
    pub b2: f32,
}

impl BiquadCoefficients {
    #[inline(always)]
    pub fn lowpass(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();

        let b0 = (1.0 - cosw) * 0.5;
        let b1 = 1.0 - cosw;
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cosw;
        let a2 = 1.0 - alpha;

        Self {
            a0: b0 / a0,
            a1: b1 / a0,
            a2: b2 / a0,
            b1: a1 / a0,
            b2: a2 / a0,
        }
    }

    #[inline(always)]
    pub fn highpass(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();

        let b0 = (1.0 + cosw) * 0.5;
        let b1 = -(1.0 + cosw);
        let b2 = b0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cosw;
        let a2 = 1.0 - alpha;

        Self {
            a0: b0 / a0,
            a1: b1 / a0,
            a2: b2 / a0,
            b1: a1 / a0,
            b2: a2 / a0,
        }
    }

    #[inline(always)]
    pub fn bandpass(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();

        let b0 = alpha;
        let b1 = 0.0;
        let b2 = -alpha;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cosw;
        let a2 = 1.0 - alpha;

        Self {
            a0: b0 / a0,
            a1: b1 / a0,
            a2: b2 / a0,
            b1: a1 / a0,
            b2: a2 / a0,
        }
    }

    #[inline(always)]
    pub fn notch(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();

        let b0 = 1.0;
        let b1 = -2.0 * cosw;
        let b2 = 1.0;
        let a0 = 1.0 + alpha;
        let a1 = -2.0 * cosw;
        let a2 = 1.0 - alpha;

        Self {
            a0: b0 / a0,
            a1: b1 / a0,
            a2: b2 / a0,
            b1: a1 / a0,
            b2: a2 / a0,
        }
    }

    #[inline(always)]
    pub fn peaking(cutoff: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();
        let a = 10.0_f32.powf(gain_db / 40.0);

        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cosw;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cosw;
        let a2 = 1.0 - alpha / a;

        Self {
            a0: b0 / a0,
            a1: b1 / a0,
            a2: b2 / a0,
            b1: a1 / a0,
            b2: a2 / a0,
        }
    }
}

/// High-performance Biquad IIR filter with parameter smoothing.
#[derive(Debug, Clone, Copy)]
pub struct BiquadFilter {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,

    cutoff_smoother: ParameterSmoother,
    q_smoother: ParameterSmoother,
    gain_smoother: ParameterSmoother,

    sample_rate: f32,
    pub filter_type: BiquadType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BiquadType {
    LowPass,
    HighPass,
    BandPass,
    Notch,
    Peaking,
}

impl BiquadFilter {
    pub fn new(sample_rate: f32, filter_type: BiquadType) -> Self {
        let mut filt = Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
            cutoff_smoother: ParameterSmoother::new(sample_rate, 15.0, 1000.0),
            q_smoother: ParameterSmoother::new(sample_rate, 15.0, 0.707),
            gain_smoother: ParameterSmoother::new(sample_rate, 15.0, 0.0),
            sample_rate,
            filter_type,
        };
        filt.update_coefficients();
        filt
    }

    pub fn set_parameters(&mut self, cutoff_hz: f32, q: f32, gain_db: f32) {
        self.cutoff_smoother
            .set_target(cutoff_hz.clamp(20.0, self.sample_rate * 0.45));
        self.q_smoother.set_target(q.max(0.1));
        self.gain_smoother.set_target(gain_db.clamp(-30.0, 30.0));
    }

    #[inline]
    fn update_coefficients(&mut self) {
        let cutoff = self.cutoff_smoother.process_sample();
        let q = self.q_smoother.process_sample();
        let gain = self.gain_smoother.process_sample();

        let coefs = match self.filter_type {
            BiquadType::LowPass => BiquadCoefficients::lowpass(cutoff, q, self.sample_rate),
            BiquadType::HighPass => BiquadCoefficients::highpass(cutoff, q, self.sample_rate),
            BiquadType::BandPass => BiquadCoefficients::bandpass(cutoff, q, self.sample_rate),
            BiquadType::Notch => BiquadCoefficients::notch(cutoff, q, self.sample_rate),
            BiquadType::Peaking => BiquadCoefficients::peaking(cutoff, q, gain, self.sample_rate),
        };

        self.b0 = coefs.a0;
        self.b1 = coefs.a1;
        self.b2 = coefs.a2;
        self.a1 = coefs.b1;
        self.a2 = coefs.b2;
    }

    pub fn reset(&mut self) {
        self.x1 = 0.0;
        self.x2 = 0.0;
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

impl Processor for BiquadFilter {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        self.update_coefficients();

        let output = self.b0 * sample + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        let output = if output.abs() < 1e-15 { 0.0 } else { output };

        self.x2 = self.x1;
        self.x1 = sample;
        self.y2 = self.y1;
        self.y1 = output;

        output
    }
}
