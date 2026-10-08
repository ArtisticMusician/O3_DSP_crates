use crate::synthesis::utils::{ParameterSmoother, Processor};
use std::f32::consts::TAU as TWO_PI;

/// Sallen-Key 2-pole Voltage-Controlled filter.
/// Classic active filter topology with 4 components (2 resistors, 2 capacitors).
/// Supports Lowpass, Highpass, Bandpass, and Notch modes via capacitor switching.
///
/// Topology:
/// - Non-inverting op-amp configuration with RC feedback network
/// - Gain set by feedback resistors (1 + Rf/Rg)
/// - Quality factor Q increases with gain (potentially unstable if Q > 1/sqrt(2))
///
/// Realised here as a normalised biquad, so a 2-pole LP/HP at `q = 0.7071` is
/// Butterworth (-3 dB at the cutoff) and the cutoff gain of the response is exactly `Q`.
#[derive(Debug, Clone, Copy)]
pub struct SallenKeyFilter {
    sample_rate: f32,

    // Filter coefficients (pre-computed for current mode, normalised so a0 == 1)
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,

    // State variables (direct form II transposed)
    w1: f32,
    w2: f32,

    // Smoothed parameters. `gain_smoother` holds the *linear* op-amp gain, since that is
    // what scales Q; `set_gain_db` converts.
    freq_smoother: ParameterSmoother,
    q_smoother: ParameterSmoother,
    gain_smoother: ParameterSmoother,

    // Current mode
    mode: SallenKeyMode,
}

/// Sallen-Key filter modes (determined by which capacitor is switched)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SallenKeyMode {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

impl Default for SallenKeyFilter {
    fn default() -> Self {
        Self::new(48000.0, 1000.0, 0.707, 1.0)
    }
}

impl SallenKeyFilter {
    /// Create a new Sallen-Key filter.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `cutoff_hz` - Target cutoff frequency
    /// * `q` - Quality factor (0.7071 = Butterworth, >0.7071 = peaking)
    /// * `gain_db` - Internal op-amp gain in dB (affects Q: effective_Q = Q * gain)
    pub fn new(sample_rate: f32, cutoff_hz: f32, q: f32, gain_db: f32) -> Self {
        let gain = db_to_gain(gain_db);
        Self::from_parameters(sample_rate, cutoff_hz, q, gain, SallenKeyMode::LowPass)
    }

    /// Create from explicit parameters. `gain` is linear (1.0 = unity).
    pub fn from_parameters(
        sample_rate: f32,
        cutoff_hz: f32,
        q: f32,
        gain: f32,
        mode: SallenKeyMode,
    ) -> Self {
        let cutoff_hz = cutoff_hz.clamp(10.0, sample_rate * 0.45);
        let q = q.clamp(0.1, 10.0);
        let gain = gain.clamp(1.0, 9.0);

        let mut filter = Self {
            sample_rate,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,

            w1: 0.0,
            w2: 0.0,

            freq_smoother: ParameterSmoother::new(sample_rate, 10.0, cutoff_hz),
            q_smoother: ParameterSmoother::new(sample_rate, 10.0, q),
            gain_smoother: ParameterSmoother::new(sample_rate, 10.0, gain),

            mode,
        };

        // Bake the starting coefficients so the filter is usable before the first sample.
        let (b0, b1, b2, a1, a2) = Self::coefficients(sample_rate, cutoff_hz, q, gain, mode);
        filter.b0 = b0;
        filter.b1 = b1;
        filter.b2 = b2;
        filter.a1 = a1;
        filter.a2 = a2;

        filter
    }

    /// Normalised biquad coefficients for one mode (a0 == 1), with `Q_eff = q * gain`.
    fn coefficients(
        sample_rate: f32,
        cutoff_hz: f32,
        q: f32,
        gain: f32,
        mode: SallenKeyMode,
    ) -> (f32, f32, f32, f32, f32) {
        let omega = TWO_PI * cutoff_hz.clamp(10.0, sample_rate * 0.45) / sample_rate;
        let (sinw, cosw) = omega.sin_cos();

        let q_eff = (q * gain).clamp(0.1, 20.0);
        let alpha = sinw / (2.0 * q_eff);
        let a0 = 1.0 + alpha;

        let (b0, b1, b2) = match mode {
            SallenKeyMode::LowPass => {
                let half = (1.0 - cosw) / 2.0;
                (half, 1.0 - cosw, half)
            }
            SallenKeyMode::HighPass => {
                let half = (1.0 + cosw) / 2.0;
                (half, -(1.0 + cosw), half)
            }
            // Constant-peak-gain band-pass: |H| = 1 at the cutoff.
            SallenKeyMode::BandPass => (alpha, 0.0, -alpha),
            SallenKeyMode::Notch => (1.0, -2.0 * cosw, 1.0),
        };

        (
            b0 / a0,
            b1 / a0,
            b2 / a0,
            -2.0 * cosw / a0,
            (1.0 - alpha) / a0,
        )
    }

    /// Set cutoff frequency (smoothed)
    pub fn set_cutoff(&mut self, cutoff_hz: f32) {
        self.freq_smoother
            .set_target(cutoff_hz.clamp(10.0, self.sample_rate * 0.45));
    }

    /// Set quality factor (smoothed)
    pub fn set_q(&mut self, q: f32) {
        self.q_smoother.set_target(q.clamp(0.1, 10.0));
    }

    /// Set internal gain in dB (smoothed)
    pub fn set_gain_db(&mut self, gain_db: f32) {
        self.gain_smoother.set_target(db_to_gain(gain_db));
    }

    /// Set filter mode
    pub fn set_mode(&mut self, mode: SallenKeyMode) {
        self.mode = mode;
        self.update_coefficients();
    }

    /// Advance the smoothed parameters and re-derive the coefficients.
    #[inline(always)]
    fn update_coefficients(&mut self) {
        let cutoff_hz = self.freq_smoother.process_sample();
        let q = self.q_smoother.process_sample();
        let gain = self.gain_smoother.process_sample();

        let (b0, b1, b2, a1, a2) = Self::coefficients(self.sample_rate, cutoff_hz, q, gain, self.mode);

        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }

    /// Get current mode
    pub fn mode(&self) -> SallenKeyMode {
        self.mode
    }

    /// Get current cutoff frequency
    pub fn cutoff(&self) -> f32 {
        self.freq_smoother.current_value()
    }

    /// Get current Q
    pub fn q(&self) -> f32 {
        self.q_smoother.current_value()
    }
}

/// Internal op-amp gain: dB to linear, clamped to the topology's usable range.
fn db_to_gain(gain_db: f32) -> f32 {
    10.0f32.powf(gain_db / 20.0).clamp(1.0, 9.0)
}

impl Processor for SallenKeyFilter {
    #[inline(always)]
    fn process_sample(&mut self, input: f32) -> f32 {
        self.update_coefficients();

        // Direct form II transposed
        let out = self.b0 * input + self.w1;
        self.w1 = self.b1 * input - self.a1 * out + self.w2;
        self.w2 = self.b2 * input - self.a2 * out;

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state RMS of a unit-amplitude sine driven through `filter`.
    fn sine_rms(filter: &mut SallenKeyFilter, freq: f32, sample_rate: f32) -> f32 {
        let increment = freq / sample_rate;
        let mut phase = 0.0f32;

        for _ in 0..(sample_rate as usize / 4) {
            phase = (phase + increment).fract();
            filter.process_sample((TWO_PI * phase).sin());
        }

        let window = 8192;
        let mut energy = 0.0;
        for _ in 0..window {
            phase = (phase + increment).fract();
            let y = filter.process_sample((TWO_PI * phase).sin());
            energy += y * y;
        }
        (energy / window as f32).sqrt()
    }

    #[test]
    fn test_lowpass_response() {
        let sample_rate = 48_000.0;
        let cutoff_hz = 1000.0;

        // DC should pass through at unity once the filter settles.
        let mut filter = SallenKeyFilter::new(sample_rate, cutoff_hz, 0.7071, 0.0);
        let mut dc_out = 0.0;
        for _ in 0..8192 {
            dc_out = filter.process_sample(1.0);
        }
        assert!((dc_out - 1.0).abs() < 0.01, "dc gain {dc_out}");

        // A 2-pole Butterworth lowpass is -3 dB at its cutoff (unit sine -> 0.5 RMS).
        let mut at_cutoff = SallenKeyFilter::new(sample_rate, cutoff_hz, 0.7071, 0.0);
        let rms = sine_rms(&mut at_cutoff, cutoff_hz, sample_rate);
        assert!((rms - 0.5).abs() < 0.02, "cutoff rms {rms}");

        // An octave above the cutoff it must fall away further.
        let mut above = SallenKeyFilter::new(sample_rate, cutoff_hz, 0.7071, 0.0);
        let rms_above = sine_rms(&mut above, cutoff_hz * 4.0, sample_rate);
        assert!(rms_above < 0.1, "4x cutoff rms {rms_above}");
    }

    #[test]
    fn test_mode_switching() {
        let mut filter = SallenKeyFilter::new(48000.0, 1000.0, 0.707, 1.0);

        assert_eq!(filter.mode(), SallenKeyMode::LowPass);

        filter.set_mode(SallenKeyMode::HighPass);
        assert_eq!(filter.mode(), SallenKeyMode::HighPass);
        assert!(filter.process_sample(1.0).is_finite());
    }

    #[test]
    fn internal_gain_raises_q() {
        // The op-amp gain multiplies Q, so the cutoff peak grows with it.
        let sample_rate = 48_000.0;
        let cutoff_hz = 1000.0;

        let mut unity = SallenKeyFilter::new(sample_rate, cutoff_hz, 0.7071, 0.0);
        let mut driven = SallenKeyFilter::new(sample_rate, cutoff_hz, 0.7071, 12.0);

        let unity_rms = sine_rms(&mut unity, cutoff_hz, sample_rate);
        let driven_rms = sine_rms(&mut driven, cutoff_hz, sample_rate);

        assert!(
            driven_rms > unity_rms * 1.2,
            "driven {driven_rms} should peak past unity {unity_rms}"
        );
    }

    #[test]
    fn stays_stable_at_every_mode() {
        for mode in [
            SallenKeyMode::LowPass,
            SallenKeyMode::HighPass,
            SallenKeyMode::BandPass,
            SallenKeyMode::Notch,
        ] {
            let mut filter = SallenKeyFilter::from_parameters(48_000.0, 1000.0, 0.7071, 1.0, mode);
            let mut last = 0.0;
            for _ in 0..16_384 {
                last = filter.process_sample(1.0);
            }
            assert!(last.is_finite(), "{mode:?} went non-finite");
        }
    }
}
