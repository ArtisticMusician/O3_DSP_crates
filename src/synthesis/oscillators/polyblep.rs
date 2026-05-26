use crate::TWO_PI;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Sawtooth,
    Square,
    Triangle,
}

/// A high-stability oscillator utilizing PolyBLEP step correction to prevent aliasing.
#[derive(Debug, Clone, Copy)]
pub struct TableOscillator {
    phase: f32,
    phase_increment: f32,
    sample_rate: f32,
}

impl TableOscillator {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            phase_increment: 0.0,
            sample_rate,
        }
    }

    pub fn set_frequency(&mut self, frequency: f32) {
        self.phase_increment = (frequency / self.sample_rate).clamp(0.0, 0.5);
    }

    /// PolyBLEP residual correction utility to damp discontinuities.
    #[inline(always)]
    fn poly_blep(&self, t: f32) -> f32 {
        let dt = self.phase_increment;
        if t < dt {
            let t_div = t / dt;
            t_div + t_div - (t_div * t_div) - 1.0
        } else if t > 1.0 - dt {
            let t_div = (t - 1.0) / dt;
            (t_div * t_div) + t_div + t_div + 1.0
        } else {
            0.0
        }
    }

    /// Render the next sample cleanly. Real-time safe.
    #[inline(always)]
    pub fn next_sample(&mut self, wave_type: Waveform) -> f32 {
        let output = match wave_type {
            Waveform::Sine => (self.phase * TWO_PI).sin(),
            Waveform::Sawtooth => {
                let naive = (self.phase * 2.0) - 1.0;
                naive - self.poly_blep(self.phase)
            }
            Waveform::Square => {
                let naive = if self.phase < 0.5 { 1.0 } else { -1.0 };
                let mut blep_corrected = naive + self.poly_blep(self.phase);
                
                // Branchless replacement for the expensive floating-point modulo operator
                let half_phase = self.phase + 0.5;
                let half_phase_wrapped = if half_phase >= 1.0 { half_phase - 1.0 } else { half_phase };
                
                blep_corrected -= self.poly_blep(half_phase_wrapped);
                blep_corrected
            }
            Waveform::Triangle => {
                let val = (self.phase * 2.0) - 1.0;
                2.0 * val.abs() - 1.0
            }
        };

        self.phase += self.phase_increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        output
    }
}