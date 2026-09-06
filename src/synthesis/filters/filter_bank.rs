use super::{AllpassFilter, CombFilter};
use crate::synthesis::utils::Processor;

/// Parallel sum vs series cascade for a bank of identical primitives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BankMode {
    /// Sum every stage's output, then scale by `1/N`.
    Parallel,
    /// Feed each stage into the next.
    Series,
}

/// N feedback combs, sized at construction. Use Parallel for a Schroeder tank.
#[derive(Debug, Clone)]
pub struct CombBank {
    stages: Vec<CombFilter>,
    mode: BankMode,
}

impl CombBank {
    /// One comb per `(delay_samples, feedback)` pair. Empty `specs` is a no-op pass-through.
    pub fn new(specs: &[(usize, f32)], mode: BankMode) -> Self {
        let stages = specs
            .iter()
            .map(|&(delay, feedback)| {
                let mut comb = CombFilter::new(delay.max(2));
                comb.set_delay_samples(delay.max(1));
                comb.set_feedback(feedback);
                comb
            })
            .collect();
        Self { stages, mode }
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn mode(&self) -> BankMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: BankMode) {
        self.mode = mode;
    }

    pub fn stage_mut(&mut self, index: usize) -> Option<&mut CombFilter> {
        self.stages.get_mut(index)
    }

    pub fn set_feedback_all(&mut self, feedback: f32) {
        for stage in &mut self.stages {
            stage.set_feedback(feedback);
        }
    }

    pub fn set_damp_all(&mut self, damp: f32) {
        for stage in &mut self.stages {
            stage.set_damp(damp);
        }
    }

    pub fn clear(&mut self) {
        for stage in &mut self.stages {
            stage.clear();
        }
    }

    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        match self.mode {
            BankMode::Series => {
                let mut x = input;
                for stage in &mut self.stages {
                    x = stage.process(x);
                }
                x
            }
            BankMode::Parallel => {
                if self.stages.is_empty() {
                    return input;
                }
                let mut acc = 0.0;
                for stage in &mut self.stages {
                    acc += stage.process(input);
                }
                acc * (1.0 / self.stages.len() as f32)
            }
        }
    }
}

impl Processor for CombBank {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        self.process(sample)
    }
}

/// N allpasses, sized at construction. Use Series for a Schroeder diffusion chain.
#[derive(Debug, Clone)]
pub struct AllpassBank {
    stages: Vec<AllpassFilter>,
    mode: BankMode,
}

impl AllpassBank {
    /// One allpass per `(delay_samples, gain)` pair.
    pub fn new(specs: &[(usize, f32)], mode: BankMode) -> Self {
        let stages = specs
            .iter()
            .map(|&(delay, gain)| {
                let mut ap = AllpassFilter::new(delay.max(2));
                ap.set_delay_samples(delay.max(1));
                ap.set_gain(gain);
                ap
            })
            .collect();
        Self { stages, mode }
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn mode(&self) -> BankMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: BankMode) {
        self.mode = mode;
    }

    pub fn stage_mut(&mut self, index: usize) -> Option<&mut AllpassFilter> {
        self.stages.get_mut(index)
    }

    pub fn set_gain_all(&mut self, gain: f32) {
        for stage in &mut self.stages {
            stage.set_gain(gain);
        }
    }

    pub fn clear(&mut self) {
        for stage in &mut self.stages {
            stage.clear();
        }
    }

    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        match self.mode {
            BankMode::Series => {
                let mut x = input;
                for stage in &mut self.stages {
                    x = stage.process(x);
                }
                x
            }
            BankMode::Parallel => {
                if self.stages.is_empty() {
                    return input;
                }
                let mut acc = 0.0;
                for stage in &mut self.stages {
                    acc += stage.process(input);
                }
                acc * (1.0 / self.stages.len() as f32)
            }
        }
    }
}

impl Processor for AllpassBank {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        self.process(sample)
    }
}

#[cfg(test)]
mod tests {
    use super::{AllpassBank, BankMode, CombBank};

    #[test]
    fn parallel_comb_bank_is_stable() {
        let mut bank = CombBank::new(&[(128, 0.7), (196, 0.7), (256, 0.7)], BankMode::Parallel);
        let mut peak = 0.0f32;
        for n in 0..2048 {
            let x = if n == 0 { 1.0 } else { 0.0 };
            peak = peak.max(bank.process(x).abs());
        }
        assert!(peak < 4.0, "parallel tank blew up: {peak}");
    }

    #[test]
    fn series_allpass_preserves_impulse_energy_order() {
        let mut bank = AllpassBank::new(&[(32, 0.5), (48, 0.5)], BankMode::Series);
        let first = bank.process(1.0);
        assert!(first.abs() > 0.0);
    }
}
