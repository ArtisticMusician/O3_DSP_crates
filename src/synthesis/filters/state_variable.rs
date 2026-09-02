use crate::synthesis::utils::{ParameterSmoother, Processor};
use std::f32::consts::TAU as TWO_PI;

/// State Variable Filter (SVF) - multi-mode with excellent musicality and self-oscillation.
/// Implemented using Chamberlin/Zavalishin topology.
#[derive(Debug, Clone, Copy)]
pub struct StateVariableFilter {
    sample_rate: f32,
    ic1: f32,
    ic2: f32, // Integrator states

    freq_smoother: ParameterSmoother,
    damp_smoother: ParameterSmoother,

    pub mode: SvfMode,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvfMode {
    LowPass,
    BandPass,
    HighPass,
    Notch,
}

impl StateVariableFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            ic1: 0.0,
            ic2: 0.0,
            freq_smoother: ParameterSmoother::new(sample_rate, 15.0, 1000.0),
            damp_smoother: ParameterSmoother::new(sample_rate, 15.0, 0.5),
            mode: SvfMode::LowPass,
        }
    }

    pub fn set_parameters(&mut self, cutoff_hz: f32, resonance: f32) {
        let f = (TWO_PI * cutoff_hz / self.sample_rate).clamp(0.0, 0.99 * std::f32::consts::PI);
        self.freq_smoother.set_target(f);

        let d = (1.0 / resonance.max(0.5)).clamp(0.0, 1.0); // Rough Q → damping
        self.damp_smoother.set_target(d);
    }
}

impl Processor for StateVariableFilter {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let f = self.freq_smoother.process_sample();
        let d = self.damp_smoother.process_sample();

        let hp = input - d * self.ic1 - self.ic2;
        let bp = self.ic1 + f * hp;
        let lp = self.ic2 + f * bp;

        self.ic1 = bp;
        self.ic2 = lp;

        // Anti-denormal
        if self.ic1.abs() < 1e-15 {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < 1e-15 {
            self.ic2 = 0.0;
        }

        match self.mode {
            SvfMode::LowPass => lp,
            SvfMode::BandPass => bp,
            SvfMode::HighPass => hp,
            SvfMode::Notch => hp + lp,
        }
    }
}
