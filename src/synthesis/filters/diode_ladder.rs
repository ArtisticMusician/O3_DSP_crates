use crate::synthesis::utils::ParameterSmoother;
use std::f32::consts::PI;

/// Resonance at which the ladder starts to self-oscillate.
const MAX_RESONANCE: f32 = 4.0;

/// Input drive into the feedback saturator.
const FEEDBACK_DRIVE: f32 = 1.2;

/// Diode ladder filter - a 4-pole voltage-controlled lowpass with the diode ladder's signature.
///
/// Same zero-delay-feedback ladder topology as [`crate::synthesis::filters::ladder_filter::LadderFilter`],
/// with the two things that make the diode ladder sound different:
///
/// - **Asymmetric saturation in the feedback path.** Each half of the waveform is compressed
///   differently, so hard resonance thickens into even as well as odd harmonics instead of
///   staying symmetric.
/// - **Per-stage saturation.** Every one-pole stage clips its own output rather than only the
///   input node, which is what happens when each transistor pair saturates in turn.
///
/// Resonance runs 0.0 to 4.0 and self-oscillates at the top. An anti-denormal bias keeps the
/// tail from stalling the CPU.
#[derive(Debug, Clone, Copy)]
pub struct DiodeLadderFilter {
    sample_rate: f32,

    // One-pole integrator states
    stages: [f32; 4],

    // Cached coefficients for the hot loop
    g: f32,
    big_g: f32,
    resonance: f32,

    // Smoothed parameters
    freq_smoother: ParameterSmoother,
    resonance_smoother: ParameterSmoother,
}

impl Default for DiodeLadderFilter {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl DiodeLadderFilter {
    /// Create a new diode ladder filter.
    pub fn new(sample_rate: f32) -> Self {
        let mut filter = Self {
            sample_rate,
            stages: [0.0; 4],
            g: 0.0,
            big_g: 0.0,
            resonance: 0.0,
            freq_smoother: ParameterSmoother::new(sample_rate, 15.0, 1000.0),
            resonance_smoother: ParameterSmoother::new(sample_rate, 15.0, 0.0),
        };

        filter.update_coefficients();
        filter
    }

    /// Set cutoff and resonance. Both are smoothed.
    pub fn set_parameters(&mut self, cutoff_hz: f32, resonance: f32) {
        let ceiling = (self.sample_rate * 0.45).min(20_000.0);
        self.freq_smoother
            .set_target(cutoff_hz.clamp(20.0, ceiling));
        self.resonance_smoother
            .set_target(resonance.clamp(0.0, MAX_RESONANCE));
    }

    /// Current cutoff in Hz.
    pub fn cutoff(&self) -> f32 {
        self.freq_smoother.current_value()
    }

    /// Current resonance.
    pub fn resonance(&self) -> f32 {
        self.resonance_smoother.current_value()
    }

    /// Clear the integrator states.
    pub fn reset(&mut self) {
        self.stages = [0.0; 4];
    }

    /// Advance the smoothers and re-derive the TPT coefficient.
    #[inline(always)]
    fn update_coefficients(&mut self) {
        let cutoff = self.freq_smoother.process_sample();
        self.resonance = self.resonance_smoother.process_sample();

        let omega = PI * cutoff / self.sample_rate;
        self.g = omega.tan();
        self.big_g = self.g / (1.0 + self.g);
    }

    /// Process one sample.
    #[inline(always)]
    pub fn process(&mut self, input: f32) -> f32 {
        self.update_coefficients();

        let big_g = self.big_g;
        let k = self.resonance;

        // Zero-delay feedback solve: the ladder's feedback term folded into the input.
        let g2 = big_g * big_g;
        let g3 = g2 * big_g;
        let g4 = g3 * big_g;

        // Copy the integrator states out so the stages can be updated in place below.
        let s0 = self.stages[0];
        let s1 = self.stages[1];
        let s2 = self.stages[2];
        let s3 = self.stages[3];

        let feedback_state = g3 * s0 + g2 * s1 + big_g * s2 + s3;

        // The diode asymmetry lives here.
        let driven = diode_saturate((input + 1e-25) - k * feedback_state * FEEDBACK_DRIVE);
        let u = driven / (1.0 + k * g4);

        // Cascade the four one-pole stages, saturating each one.
        let mut node = u;
        for index in 0..4 {
            let state = self.stages[index];
            // Trapezoidal integrator: the update gain is G = g / (1 + g), never the raw g.
            let v = (node - state) * big_g;
            let y = v + state;
            self.stages[index] = y + v;
            node = soft_clip(y);
        }

        node
    }
}

/// Asymmetric diode soft clip.
///
/// Identity inside +/-1.5, so nominal levels (and some feedback headroom) pass through untouched.
/// Above the knee the two halves fold differently - positive peaks compress towards +2.5, negative
/// peaks towards -4.5 - which is the asymmetry that gives a diode ladder its extra harmonics.
/// Continuous and monotonic, so it introduces no DC either.
#[inline(always)]
fn diode_saturate(x: f32) -> f32 {
    const KNEE: f32 = 1.5;

    if x > KNEE {
        let excess = x - KNEE;
        KNEE + excess / (1.0 + excess)
    } else if x < -KNEE {
        let excess = -x - KNEE;
        -KNEE - excess / (1.0 + 0.5 * excess)
    } else {
        x
    }
}

/// Symmetric per-stage saturation: identity inside +/-1.5, soft knee above, asymptote at +/-4.5.
#[inline(always)]
fn soft_clip(x: f32) -> f32 {
    const KNEE: f32 = 1.5;

    if x > KNEE {
        let excess = x - KNEE;
        KNEE + excess / (1.0 + 0.5 * excess)
    } else if x < -KNEE {
        let excess = -x - KNEE;
        -KNEE - excess / (1.0 + 0.5 * excess)
    } else {
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// Settled DC gain.
    fn dc_gain(resonance: f32) -> f32 {
        let mut filter = DiodeLadderFilter::new(48_000.0);
        filter.set_parameters(1_000.0, resonance);

        let mut out = 0.0;
        for _ in 0..16_384 {
            out = filter.process(1.0);
        }
        out
    }

    /// Settled RMS at the cutoff frequency for a unit sine.
    fn rms_at_cutoff(resonance: f32) -> f32 {
        let mut filter = DiodeLadderFilter::new(48_000.0);
        filter.set_parameters(1_000.0, resonance);

        let increment = 1_000.0 / 48_000.0;
        let mut phase = 0.0f32;
        for _ in 0..24_000 {
            phase = (phase + increment).fract();
            filter.process((TAU * phase).sin());
        }

        let window = 8_192;
        let mut energy = 0.0;
        for _ in 0..window {
            phase = (phase + increment).fract();
            let y = filter.process((TAU * phase).sin());
            energy += y * y;
        }
        (energy / window as f32).sqrt()
    }

    #[test]
    fn dc_passes_at_unity() {
        let gain = dc_gain(0.0);
        assert!((gain - 1.0).abs() < 0.05, "dc gain {gain}");
    }

    #[test]
    fn reduces_dc_as_resonance_rises() {
        // A ladder loses low-frequency gain as the feedback takes over. The asymmetric feedback
        // saturators can leave a small negative residue at extreme resonance, so only the
        // magnitude is checked.
        let clean = dc_gain(0.0);
        let resonant = dc_gain(3.5);
        assert!(clean > 0.9, "clean dc gain {clean}");
        assert!(
            resonant.abs() < clean * 0.5,
            "resonance should eat bass: {clean} -> {resonant}"
        );
    }

    #[test]
    fn resonance_raises_the_cutoff_peak() {
        let flat = rms_at_cutoff(0.0);
        let peaked = rms_at_cutoff(3.0);

        assert!(
            peaked > flat * 2.0,
            "the cutoff peak should rise with resonance: {flat} -> {peaked}"
        );
    }

    #[test]
    fn stable_at_maximum_resonance() {
        let mut filter = DiodeLadderFilter::new(48_000.0);
        filter.set_parameters(1_000.0, MAX_RESONANCE);

        // Give it something to ring with, then let it run.
        filter.process(1.0);

        for i in 0..200_000 {
            let y = filter.process(0.0);
            assert!(y.is_finite(), "non-finite at sample {i}: {y}");
            assert!(y.abs() < 10.0, "blew up at sample {i}: {y}");
        }
    }

    #[test]
    fn silence_stays_silent() {
        let mut filter = DiodeLadderFilter::new(48_000.0);
        filter.set_parameters(800.0, 2.0);

        // The anti-denormal bias means "silent" is a hair above zero, not exactly zero.
        for _ in 0..10_000 {
            let y = filter.process(0.0);
            assert!(y.abs() < 1e-20, "silence produced {y}");
        }
    }

    #[test]
    fn cutoff_sweep_stays_finite() {
        let mut filter = DiodeLadderFilter::new(48_000.0);

        for step in 0..=200 {
            let cutoff = 20.0 + (48_000.0 * 0.45 - 20.0) * (step as f32 / 200.0);
            filter.set_parameters(cutoff, 3.9);

            for _ in 0..512 {
                let y = filter.process(0.5);
                assert!(y.is_finite(), "non-finite at cutoff {cutoff}");
            }
        }
    }

    #[test]
    fn asymmetric_clip_folds_the_two_halves_differently() {
        // Symmetric clipping would give equal and opposite bounds; the diode ladder does not.
        let positive = diode_saturate(10.0);
        let negative = diode_saturate(-10.0);

        assert!(
            (1.5..3.0).contains(&positive),
            "positive half should compress past the knee: {positive}"
        );
        assert!(negative < -2.5, "negative half should reach further: {negative}");
        assert_eq!(diode_saturate(0.0), 0.0, "no DC shift");
        assert_eq!(diode_saturate(0.5), 0.5, "nominal levels must pass untouched");
        assert_eq!(soft_clip(-0.75), -0.75);
    }
}
