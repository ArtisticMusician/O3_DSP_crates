use crate::synthesis::utils::{ParameterSmoother, Processor};

/// Moog-style transistor ladder filter.
/// Emulates 4-pole lowpass with non-linear saturation.
#[derive(Debug, Clone, Copy)]
pub struct MoogLadderFilter {
    sample_rate: f32,

    // 4 stages of 1-pole RC filters
    stage: [f32; 4],

    cutoff_smoother: ParameterSmoother,
    res_smoother: ParameterSmoother,
    drive_smoother: ParameterSmoother,
}

impl MoogLadderFilter {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            stage: [0.0; 4],
            cutoff_smoother: ParameterSmoother::new(sample_rate, 15.0, 1000.0),
            res_smoother: ParameterSmoother::new(sample_rate, 15.0, 0.0),
            drive_smoother: ParameterSmoother::new(sample_rate, 15.0, 1.0),
        }
    }

    pub fn set_parameters(&mut self, cutoff_hz: f32, resonance: f32, drive: f32) {
        self.cutoff_smoother.set_target(cutoff_hz.clamp(20.0, self.sample_rate * 0.45));
        self.res_smoother.set_target(resonance.clamp(0.0, 4.0));
        self.drive_smoother.set_target(drive.clamp(1.0, 10.0));
    }

    #[inline(always)]
    fn fast_tanh(x: f32) -> f32 {
        let x2 = x * x;
        x * (27.0 + x2) / (27.0 + 9.0 * x2)
    }
}

impl Processor for MoogLadderFilter {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        let cutoff = self.cutoff_smoother.process_sample();
        let res = self.res_smoother.process_sample();
        let drive = self.drive_smoother.process_sample();

        let g = 1.0 - (-2.0 * std::f32::consts::PI * cutoff / self.sample_rate).exp();
        let in_sig = input * drive;
        let feedback = res * self.stage[3];

        let input_stage = Self::fast_tanh(in_sig - feedback);
        let t0 = input_stage;

        for i in 0..4 {
            let st_in = if i == 0 { t0 } else { self.stage[i-1] };
            self.stage[i] += g * (Self::fast_tanh(st_in) - Self::fast_tanh(self.stage[i]));
            if self.stage[i].abs() < 1e-15 { self.stage[i] = 0.0; }
        }

        self.stage[3]
    }
}
