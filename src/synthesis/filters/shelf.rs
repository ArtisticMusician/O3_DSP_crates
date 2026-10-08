use crate::synthesis::utils::{ParameterSmoother, Processor};
use std::f32::consts::TAU as TWO_PI;

/// Shelf filter (Low-Shelf or High-Shelf).
/// Boosts or cuts frequencies below/above the cutoff with a smooth transition.
///
/// Uses the RBJ audio-EQ-cookbook shelf formulation. `gain_db` is the shelf gain
/// reached at DC (low shelf) or Nyquist (high shelf); the opposite end stays at unity.
/// `q` sets the transition steepness: 1.0 is the gentle "Butterworth-like" shelf,
/// higher values make the corner sharper (0.707 is the widest useful setting).
#[derive(Debug, Clone, Copy)]
pub struct ShelfFilter {
    sample_rate: f32,

    // Coefficients (direct form II transposed, normalised so a0 == 1)
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,

    // State variables
    w1: f32,
    w2: f32,

    // Smoothed parameters
    freq_smoother: ParameterSmoother,
    gain_smoother: ParameterSmoother,
    q_smoother: ParameterSmoother,

    // Mode
    mode: ShelfMode,
}

/// Shelf filter modes
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShelfMode {
    LowShelf,
    HighShelf,
}

impl Default for ShelfFilter {
    fn default() -> Self {
        Self::new(48000.0, 200.0, -3.0, 1.0, ShelfMode::LowShelf)
    }
}

impl ShelfFilter {
    /// Create a new shelf filter.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `cutoff_hz` - Cutoff frequency (shelf "center" frequency)
    /// * `gain_db` - Amount of boost/cut in dB (positive = boost, negative = cut)
    /// * `q` - Quality factor (1.0 = Butterworth, >1.0 = steeper transition)
    pub fn new(sample_rate: f32, cutoff_hz: f32, gain_db: f32, q: f32, mode: ShelfMode) -> Self {
        Self::from_parameters(sample_rate, cutoff_hz, gain_db, q, mode)
    }

    /// Create from explicit parameters.
    pub fn from_parameters(
        sample_rate: f32,
        cutoff_hz: f32,
        gain_db: f32,
        q: f32,
        mode: ShelfMode,
    ) -> Self {
        let cutoff_hz = cutoff_hz.clamp(10.0, sample_rate * 0.45);
        let q = q.clamp(0.1, 10.0);

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
            gain_smoother: ParameterSmoother::new(sample_rate, 10.0, gain_db),
            q_smoother: ParameterSmoother::new(sample_rate, 10.0, q),

            mode,
        };

        // Bake the starting coefficients immediately so the filter is usable before
        // the first process_sample() call.
        let (b0, b1, b2, a1, a2) = Self::coefficients(sample_rate, cutoff_hz, gain_db, q, mode);
        filter.b0 = b0;
        filter.b1 = b1;
        filter.b2 = b2;
        filter.a1 = a1;
        filter.a2 = a2;

        filter
    }

    /// RBJ shelf coefficients, normalised so `a0 == 1`.
    ///
    /// `gain_db` maps to `a = 10^(gain_db / 40)`, so the shelf reaches `10^(gain_db / 20)`
    /// at its boosted end and unity at the other.
    fn coefficients(
        sample_rate: f32,
        cutoff_hz: f32,
        gain_db: f32,
        q: f32,
        mode: ShelfMode,
    ) -> (f32, f32, f32, f32, f32) {
        let omega = TWO_PI * cutoff_hz.clamp(10.0, sample_rate * 0.45) / sample_rate;
        let sinw = omega.sin();
        let cosw = omega.cos();

        let a = 10.0f32.powf(gain_db / 40.0);
        let sqrt_a = a.sqrt();
        let alpha = sinw / (2.0 * q.clamp(0.1, 10.0));

        let two_sqrt_a_alpha = 2.0 * sqrt_a * alpha;

        let (b0, b1, b2, a0, a1, a2) = match mode {
            ShelfMode::LowShelf => (
                a * ((a + 1.0) - (a - 1.0) * cosw + two_sqrt_a_alpha),
                2.0 * a * ((a - 1.0) - (a + 1.0) * cosw),
                a * ((a + 1.0) - (a - 1.0) * cosw - two_sqrt_a_alpha),
                (a + 1.0) + (a - 1.0) * cosw + two_sqrt_a_alpha,
                -2.0 * ((a - 1.0) + (a + 1.0) * cosw),
                (a + 1.0) + (a - 1.0) * cosw - two_sqrt_a_alpha,
            ),
            ShelfMode::HighShelf => (
                a * ((a + 1.0) + (a - 1.0) * cosw + two_sqrt_a_alpha),
                -2.0 * a * ((a - 1.0) + (a + 1.0) * cosw),
                a * ((a + 1.0) + (a - 1.0) * cosw - two_sqrt_a_alpha),
                (a + 1.0) - (a - 1.0) * cosw + two_sqrt_a_alpha,
                2.0 * ((a - 1.0) - (a + 1.0) * cosw),
                (a + 1.0) - (a - 1.0) * cosw - two_sqrt_a_alpha,
            ),
        };

        (b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
    }

    /// Set cutoff frequency (smoothed)
    pub fn set_cutoff(&mut self, cutoff_hz: f32) {
        self.freq_smoother
            .set_target(cutoff_hz.clamp(10.0, self.sample_rate * 0.45));
    }

    /// Set gain in dB (smoothed)
    pub fn set_gain(&mut self, gain_db: f32) {
        self.gain_smoother.set_target(gain_db.clamp(-24.0, 24.0));
    }

    /// Set Q factor (smoothed)
    pub fn set_q(&mut self, q: f32) {
        self.q_smoother.set_target(q.clamp(0.1, 10.0));
    }

    /// Set filter mode
    pub fn set_mode(&mut self, mode: ShelfMode) {
        self.mode = mode;
        self.update_coefficients();
    }

    /// Advance the smoothed parameters and re-derive the coefficients.
    #[inline(always)]
    fn update_coefficients(&mut self) {
        let cutoff_hz = self.freq_smoother.process_sample();
        let gain_db = self.gain_smoother.process_sample();
        let q = self.q_smoother.process_sample();

        let (b0, b1, b2, a1, a2) =
            Self::coefficients(self.sample_rate, cutoff_hz, gain_db, q, self.mode);

        self.b0 = b0;
        self.b1 = b1;
        self.b2 = b2;
        self.a1 = a1;
        self.a2 = a2;
    }

    /// Get current mode
    pub fn mode(&self) -> ShelfMode {
        self.mode
    }

    /// Get current cutoff frequency
    pub fn cutoff(&self) -> f32 {
        self.freq_smoother.current_value()
    }

    /// Get current gain in dB
    pub fn gain(&self) -> f32 {
        self.gain_smoother.current_value()
    }
}

impl Processor for ShelfFilter {
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

    /// Steady-state response to a DC input (the shelf gain is only reached once the
    /// filter has settled - a single sample only shows `b0`).
    fn dc_gain(filter: &mut ShelfFilter) -> f32 {
        let mut out = 0.0;
        for _ in 0..4096 {
            out = filter.process_sample(1.0);
        }
        out
    }

    #[test]
    fn test_low_shelf_dc() {
        // At DC, low-shelf should pass the gain
        let mut filter = ShelfFilter::new(48000.0, 200.0, 6.0, 1.0, ShelfMode::LowShelf);
        let out = dc_gain(&mut filter);
        let gain_linear = 10f32.powf(6.0 / 20.0);
        assert!((out - gain_linear).abs() < 0.01, "dc gain {out}");
    }

    #[test]
    fn test_high_shelf_dc() {
        // At DC, high-shelf should have no gain (0dB)
        let mut filter = ShelfFilter::new(48000.0, 20000.0, 6.0, 1.0, ShelfMode::HighShelf);
        let out = dc_gain(&mut filter);
        assert!((out - 1.0).abs() < 0.01, "dc gain {out}");
    }

    #[test]
    fn test_mode_switching() {
        let mut filter = ShelfFilter::new(48000.0, 200.0, 6.0, 1.0, ShelfMode::LowShelf);
        assert_eq!(filter.mode(), ShelfMode::LowShelf);

        filter.set_mode(ShelfMode::HighShelf);
        assert_eq!(filter.mode(), ShelfMode::HighShelf);
    }

    #[test]
    fn high_shelf_boosts_its_own_end() {
        // A +6 dB high shelf must reach +6 dB at Nyquist, not at DC.
        let mut filter = ShelfFilter::new(48000.0, 1000.0, 6.0, 0.707, ShelfMode::HighShelf);
        let mut last = 0.0;
        for i in 0..2000 {
            let x = if i % 2 == 0 { 1.0 } else { -1.0 }; // Nyquist
            last = filter.process_sample(x);
        }
        assert!((last.abs() - 10f32.powf(6.0 / 20.0)).abs() < 0.05, "nyquist gain {last}");
    }
}
