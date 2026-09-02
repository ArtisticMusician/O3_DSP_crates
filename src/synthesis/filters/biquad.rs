use std::f32::consts::TAU as TWO_PI;
use crate::synthesis::utils::{ParameterSmoother, Processor};

/// Normalized Direct Form I coefficients (feedforward a*, feedback b*).
#[derive(Debug, Clone, Copy)]
pub struct BiquadCoefficients {
    pub a0: f32,
    pub a1: f32,
    pub a2: f32,
    pub b1: f32,
    pub b2: f32,
}

impl BiquadCoefficients {
    #[inline]
    fn normalize(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        let inv = 1.0 / a0;
        Self {
            a0: b0 * inv,
            a1: b1 * inv,
            a2: b2 * inv,
            b1: a1 * inv,
            b2: a2 * inv,
        }
    }

    #[inline]
    pub fn lowpass(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();
        Self::normalize(
            (1.0 - cosw) * 0.5,
            1.0 - cosw,
            (1.0 - cosw) * 0.5,
            1.0 + alpha,
            -2.0 * cosw,
            1.0 - alpha,
        )
    }

    #[inline]
    pub fn highpass(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();
        Self::normalize(
            (1.0 + cosw) * 0.5,
            -(1.0 + cosw),
            (1.0 + cosw) * 0.5,
            1.0 + alpha,
            -2.0 * cosw,
            1.0 - alpha,
        )
    }

    #[inline]
    pub fn bandpass(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();
        Self::normalize(alpha, 0.0, -alpha, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha)
    }

    #[inline]
    pub fn notch(cutoff: f32, q: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();
        Self::normalize(1.0, -2.0 * cosw, 1.0, 1.0 + alpha, -2.0 * cosw, 1.0 - alpha)
    }

    #[inline]
    pub fn peaking(cutoff: f32, q: f32, gain_db: f32, sample_rate: f32) -> Self {
        let omega = TWO_PI * cutoff / sample_rate;
        let alpha = omega.sin() / (2.0 * q);
        let cosw = omega.cos();
        let a = 10.0_f32.powf(gain_db * 0.025);
        Self::normalize(
            1.0 + alpha * a,
            -2.0 * cosw,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * cosw,
            1.0 - alpha / a,
        )
    }
}

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

    last_cutoff: f32,
    last_q: f32,
    last_gain: f32,

    sample_rate: f32,
    pub filter_type: BiquadType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
            last_cutoff: f32::NAN,
            last_q: f32::NAN,
            last_gain: f32::NAN,
            sample_rate,
            filter_type,
        };
        filt.update_coefficients(true);
        filt
    }

    pub fn set_parameters(&mut self, cutoff_hz: f32, q: f32, gain_db: f32) {
        self.cutoff_smoother
            .set_target(cutoff_hz.clamp(20.0, self.sample_rate * 0.45));
        self.q_smoother.set_target(q.max(0.1));
        self.gain_smoother.set_target(gain_db.clamp(-30.0, 30.0));
    }

    #[inline]
    fn update_coefficients(&mut self, force: bool) {
        let cutoff = self.cutoff_smoother.process_sample();
        let q = self.q_smoother.process_sample();
        let gain = self.gain_smoother.process_sample();

        if !force && cutoff == self.last_cutoff && q == self.last_q && gain == self.last_gain {
            return;
        }

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
        self.last_cutoff = cutoff;
        self.last_q = q;
        self.last_gain = gain;
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
        self.update_coefficients(false);

        let output = self.b0 * sample + self.b1 * self.x1 + self.b2 * self.x2
            - self.a1 * self.y1
            - self.a2 * self.y2;

        self.x2 = self.x1;
        self.x1 = sample;
        self.y2 = self.y1;
        self.y1 = output;
        output
    }
}
