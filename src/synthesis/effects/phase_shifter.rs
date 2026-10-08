use crate::synthesis::filters::allpass::AllpassFilter;
use crate::synthesis::utils::{random_f32, ParameterSmoother};
use std::f32::consts::TAU;

/// Phaser effect (multi-stage allpass filters).
/// Creates sweeping, modulated phase shifts that produce characteristic "swooshing" sound.
///
/// Classic phaser topologies:
/// - 4-stage allpass (classic 70s sound)
/// - 6-stage allpass (richer modulation)
/// - 8-stage allpass (deep, wide modulation)
#[derive(Debug, Clone)]
pub struct Phaser {
    sample_rate: f32,

    // Allpass filter stages
    allpasses: Vec<AllpassFilter>,

    // Wet/dry mix
    wet_dry: f32,

    // Smoothed parameters
    rate_smoother: ParameterSmoother,
    depth_smoother: ParameterSmoother,
    feedback_smoother: ParameterSmoother,

    // Modulation waveform and its running phase (0.0 - 1.0)
    modulation_waveform: PhaserModulation,
    modulation_phase: f32,

    // Last output of the allpass chain, fed back into its input
    feedback_state: f32,
}

/// Phaser modulation waveform
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhaserModulation {
    Sine,
    Triangle,
    Square,
    Random,
}

impl Default for Phaser {
    fn default() -> Self {
        Self::new(48000.0, 4) // 48kHz, 4-stage
    }
}

impl Phaser {
    /// Create a new phaser with the specified number of stages.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `num_stages` - Number of allpass stages (4, 6, 8, etc.)
    pub fn new(sample_rate: f32, num_stages: usize) -> Self {
        let num_stages = num_stages.max(4).min(16);

        let mut allpasses = Vec::with_capacity(num_stages);
        for _ in 0..num_stages {
            allpasses.push(AllpassFilter::new(20)); // 20 samples delay
        }

        Self {
            sample_rate,
            allpasses,
            wet_dry: 1.0,
            rate_smoother: ParameterSmoother::new(sample_rate, 10.0, 0.5),
            depth_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
            feedback_smoother: ParameterSmoother::new(sample_rate, 10.0, 0.5),
            modulation_waveform: PhaserModulation::Sine,
            modulation_phase: 0.0,
            feedback_state: 0.0,
        }
    }

    /// Set modulation rate (smoothed)
    pub fn set_rate(&mut self, rate_hz: f32) {
        self.rate_smoother.set_target(rate_hz.max(0.1).min(20.0));
    }

    /// Set modulation depth (smoothed)
    pub fn set_depth(&mut self, depth: f32) {
        self.depth_smoother.set_target(depth.max(0.0).min(10.0));
    }

    /// Set feedback amount (smoothed)
    pub fn set_feedback(&mut self, feedback: f32) {
        self.feedback_smoother.set_target(feedback.max(-24.0).min(6.0));
    }

    /// Set wet/dry mix (smoothed)
    pub fn set_wet_dry(&mut self, mix: f32) {
        self.wet_dry = mix.max(0.0).min(1.0);
    }

    /// Set modulation waveform
    pub fn set_modulation(&mut self, waveform: PhaserModulation) {
        self.modulation_waveform = waveform;
    }

    /// Get current modulation value
    #[inline(always)]
    fn get_modulation(&mut self) -> f32 {
        let rate = self.rate_smoother.process_sample();
        let depth = self.depth_smoother.process_sample().clamp(0.0, 1.0);

        // Advance the LFO for this sample (`rate` is in Hz).
        self.modulation_phase += rate / self.sample_rate;
        if self.modulation_phase >= 1.0 {
            self.modulation_phase -= self.modulation_phase.floor();
        }

        let phase = self.modulation_phase;
        let lfo = match self.modulation_waveform {
            PhaserModulation::Sine => 0.5 + 0.5 * (TAU * phase).sin(),
            PhaserModulation::Triangle => {
                if phase < 0.5 {
                    2.0 * phase
                } else {
                    2.0 * (1.0 - phase)
                }
            }
            PhaserModulation::Square => {
                if phase < 0.5 {
                    1.0
                } else {
                    0.0
                }
            }
            PhaserModulation::Random => random_f32(),
        };

        // Depth narrows the sweep around its midpoint; depth 0.0 holds it still.
        0.5 + (lfo - 0.5) * depth
    }

    /// Process a single sample
    pub fn process_sample(&mut self, input: f32) -> f32 {
        let modulation = self.get_modulation();
        let feedback = self.feedback_smoother.process_sample().clamp(-0.95, 0.95);

        // Feedback wraps the whole chain, as on a real phaser.
        let mut output = input + feedback * self.feedback_state;

        for allpass in self.allpasses.iter_mut() {
            // Sweep the allpass gain across 0.5 - 1.0.
            allpass.set_gain(modulation * 0.5 + 0.5);
            output = allpass.process(output);
        }

        self.feedback_state = output;

        output * self.wet_dry + input * (1.0 - self.wet_dry)
    }

    /// Process a block of samples
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(*sample);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phaser() {
        let sample_rate = 48_000.0;
        let mut phaser = Phaser::new(sample_rate, 4);
        phaser.set_wet_dry(1.0);

        // With no feedback the allpass chain must be transparent in level.
        phaser.set_feedback(0.0);

        let mut phase = 0.0f32;
        let (mut input_energy, mut output_energy) = (0.0f32, 0.0f32);

        for _ in 0..8192 {
            phase = (phase + 220.0 / sample_rate).fract();
            let x = (std::f32::consts::TAU * phase).sin();
            let y = phaser.process_sample(x);

            assert!(y.is_finite(), "phaser produced {y}");
            input_energy += x * x;
            output_energy += y * y;
        }

        // An allpass chain passes every frequency at unity magnitude.
        let ratio = (output_energy / input_energy).sqrt();
        assert!((ratio - 1.0).abs() < 0.15, "allpass changed the level: {ratio}");
    }

    #[test]
    fn test_wet_dry() {
        let mut phaser = Phaser::new(48000.0, 4);
        
        phaser.set_wet_dry(0.0);
        let sample = phaser.process_sample(1.0);
        assert!((sample - 1.0).abs() < 0.01); // Should pass through dry
        
        phaser.set_wet_dry(1.0);
        let sample = phaser.process_sample(1.0);
        assert!(sample.abs() < 1.0); // Should have effect
    }
}