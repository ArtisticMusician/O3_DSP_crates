use std::f32::consts::PI;
use crate::synthesis::utils::{ParameterSmoother, Processor};

/// TPT / Zavalishin state-variable filter.
/// One recursion yields LP, BP, HP, and Notch. Stable up to Nyquist.
#[derive(Debug, Clone, Copy)]
pub struct StateVariableFilter {
    sample_rate: f32,
    s1: f32,
    s2: f32,

    cutoff_smoother: ParameterSmoother,
    q_smoother: ParameterSmoother,

    pub mode: SvfMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvfMode {
    LowPass,
    BandPass,
    HighPass,
    Notch,
}

impl StateVariableFilter {
    pub fn new(sample_rate: f32) -> Self {
        let mut svf = Self {
            sample_rate,
            s1: 0.0,
            s2: 0.0,
            cutoff_smoother: ParameterSmoother::new(sample_rate, 15.0, 1000.0),
            q_smoother: ParameterSmoother::new(sample_rate, 15.0, 0.707),
            mode: SvfMode::LowPass,
        };
        svf.set_parameters(1000.0, 0.707);
        svf.cutoff_smoother.reset(1000.0);
        svf.q_smoother.reset(0.707);
        svf
    }

    pub fn set_parameters(&mut self, cutoff_hz: f32, resonance: f32) {
        self.cutoff_smoother.set_target(cutoff_hz.clamp(20.0, self.sample_rate * 0.49));
        self.q_smoother.set_target(resonance.clamp(0.25, 20.0));
    }

    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

impl Processor for StateVariableFilter {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let cutoff = self.cutoff_smoother.process_sample();
        let q = self.q_smoother.process_sample();

        let g = (PI * cutoff / self.sample_rate).tan();
        let k = 1.0 / q;

        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let v3 = input - self.s2;
        let v1 = a1 * self.s1 + a2 * v3;
        let v2 = self.s2 + a2 * self.s1 + a3 * v3;

        self.s1 = 2.0 * v1 - self.s1;
        self.s2 = 2.0 * v2 - self.s2;

        let lp = v2;
        let bp = v1;
        let hp = input - k * bp - lp;

        match self.mode {
            SvfMode::LowPass => lp,
            SvfMode::BandPass => bp,
            SvfMode::HighPass => hp,
            SvfMode::Notch => lp + hp,
        }
    }
}
