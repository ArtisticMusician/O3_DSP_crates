use crate::synthesis::utils::ParameterSmoother;
use std::f32::consts::{PI, TAU as TWO_PI};

/// Re-derive coefficients once the smoothed cutoff has moved by more than this (Hz).
const CUTOFF_EPSILON: f32 = 1e-4;

/// Linkwitz-Riley crossover filter.
///
/// A Linkwitz-Riley crossover of order `N` is two identical Butterworth filters of order
/// `N/2` in series - once for the low band and once for the high band. That construction is
/// what gives LR crossovers their two defining properties:
/// - Lowpass and highpass are in phase at the crossover frequency (0 deg difference)
/// - Each band is -6 dB at the crossover, and the summed output is magnitude-flat
///
/// Order: 4, 6, 8, 10 ... (even numbers only; lower or odd values are rounded up).
#[derive(Debug, Clone)]
pub struct LinkwitzRileyCrossover {
    sample_rate: f32,

    // Smoothed crossover frequency
    freq_smoother: ParameterSmoother,

    // Current order, and the cutoff the coefficients were last derived at
    order: CrossoverOrder,
    current_cutoff: f32,

    // Cascaded sections, one list per band
    lowpass: Vec<Biquad>,
    highpass: Vec<Biquad>,
}

/// Crossover order (must be even: 4, 6, 8, ...)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossoverOrder {
    Fourth,
    Sixth,
    Eighth,
    Tenth,
    Custom(u8),
}

impl CrossoverOrder {
    pub fn to_u8(&self) -> u8 {
        match self {
            CrossoverOrder::Fourth => 4,
            CrossoverOrder::Sixth => 6,
            CrossoverOrder::Eighth => 8,
            CrossoverOrder::Tenth => 10,
            CrossoverOrder::Custom(n) => *n,
        }
    }
}

impl Default for LinkwitzRileyCrossover {
    fn default() -> Self {
        Self::new(48000.0, 2000.0, CrossoverOrder::Fourth) // 2kHz, 4th order
    }
}

impl LinkwitzRileyCrossover {
    /// Create a new Linkwitz-Riley crossover.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `crossover_hz` - Crossover frequency
    /// * `order` - Crossover order (4, 6, 8, etc.)
    pub fn new(sample_rate: f32, crossover_hz: f32, order: CrossoverOrder) -> Self {
        Self::from_parameters(sample_rate, crossover_hz, order)
    }

    /// Create from explicit parameters.
    pub fn from_parameters(sample_rate: f32, crossover_hz: f32, order: CrossoverOrder) -> Self {
        let cutoff = clamp_cutoff(crossover_hz, sample_rate);

        let mut crossover = Self {
            sample_rate,
            freq_smoother: ParameterSmoother::new(sample_rate, 10.0, cutoff),
            order,
            current_cutoff: cutoff,
            lowpass: Vec::new(),
            highpass: Vec::new(),
        };

        crossover.build_sections();
        crossover
    }

    /// Build both bands' sections for the current order, clearing any stored state.
    fn build_sections(&mut self) {
        let butterworth_order = (self.order.to_u8().max(2) / 2) as usize;
        let (qs, needs_first_order) = butterworth_section_qs(butterworth_order);

        self.lowpass.clear();
        self.highpass.clear();

        // Two identical Butterworth blocks in series define the Linkwitz-Riley response.
        for _ in 0..2 {
            if needs_first_order {
                self.lowpass.push(Biquad::new(SectionKind::FirstOrderLowPass, 0.0));
                self.highpass.push(Biquad::new(SectionKind::FirstOrderHighPass, 0.0));
            }
            for &q in &qs {
                self.lowpass.push(Biquad::new(SectionKind::LowPass, q));
                self.highpass.push(Biquad::new(SectionKind::HighPass, q));
            }
        }

        self.update_coefficients(self.current_cutoff);
    }

    /// Re-derive every section's coefficients for `cutoff`, keeping the filter state.
    fn update_coefficients(&mut self, cutoff: f32) {
        for section in self.lowpass.iter_mut().chain(self.highpass.iter_mut()) {
            section.update(self.sample_rate, cutoff);
        }
    }

    /// Set crossover frequency (smoothed)
    pub fn set_frequency(&mut self, crossover_hz: f32) {
        self.freq_smoother
            .set_target(clamp_cutoff(crossover_hz, self.sample_rate));
    }

    /// Set crossover order. Rebuilds the filter sections and clears their state.
    pub fn set_order(&mut self, order: CrossoverOrder) {
        self.order = order;
        self.build_sections();
    }

    /// Get current crossover frequency
    pub fn frequency(&self) -> f32 {
        self.current_cutoff
    }

    /// Get current order
    pub fn order(&self) -> CrossoverOrder {
        self.order
    }

    /// Advance the frequency smoother and re-derive coefficients if the cutoff moved.
    #[inline(always)]
    fn refresh(&mut self) {
        let cutoff = self.freq_smoother.process_sample();

        if (cutoff - self.current_cutoff).abs() > CUTOFF_EPSILON {
            self.current_cutoff = cutoff;
            self.update_coefficients(cutoff);
        }
    }

    /// Clear the section state, e.g. after a jump in crossover frequency.
    pub fn reset(&mut self) {
        for section in self.lowpass.iter_mut().chain(self.highpass.iter_mut()) {
            section.reset();
        }
    }

    /// Low band output for one input sample.
    #[inline(always)]
    pub fn lowpass(&mut self, input: f32) -> f32 {
        self.refresh();
        self.low_band(input)
    }

    /// High band output for one input sample.
    #[inline(always)]
    pub fn highpass(&mut self, input: f32) -> f32 {
        self.refresh();
        self.high_band(input)
    }

    /// Low band, without advancing the smoother - for the paired entry points.
    #[inline(always)]
    fn low_band(&mut self, input: f32) -> f32 {
        let mut out = input;
        for section in &mut self.lowpass {
            out = section.process(out);
        }
        out
    }

    /// High band, without advancing the smoother - for the paired entry points.
    #[inline(always)]
    fn high_band(&mut self, input: f32) -> f32 {
        let mut out = input;
        for section in &mut self.highpass {
            out = section.process(out);
        }
        out
    }

    /// Summed output (lowpass + highpass) - magnitude-flat, so it tracks the input.
    pub fn summed(&mut self, input: f32) -> f32 {
        self.refresh();
        self.low_band(input) + self.high_band(input)
    }

    /// Process input and return both bands.
    pub fn process(&mut self, input: f32) -> (f32, f32) {
        self.refresh();
        (self.low_band(input), self.high_band(input))
    }

    /// Single-output entry point for the chained filter module: returns the summed bands.
    /// A crossover is a two-output device - use `process()` when you need the bands apart.
    #[inline(always)]
    pub fn process_sample(&mut self, input: f32) -> f32 {
        self.summed(input)
    }
}

fn clamp_cutoff(crossover_hz: f32, sample_rate: f32) -> f32 {
    crossover_hz.clamp(20.0, sample_rate * 0.45)
}

/// Butterworth second-order section Qs for order `m`, plus whether a first-order section
/// is also required (true when `m` is odd).
fn butterworth_section_qs(m: usize) -> (Vec<f32>, bool) {
    let m = m.max(1);
    let mut qs = Vec::with_capacity(m / 2);

    for k in 0..(m / 2) {
        let theta = (2 * k + 1) as f32 * PI / (2.0 * m as f32);
        qs.push(1.0 / (2.0 * theta.sin()));
    }

    (qs, m % 2 == 1)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SectionKind {
    LowPass,
    HighPass,
    FirstOrderLowPass,
    FirstOrderHighPass,
}

/// One normalised section (a0 == 1) in direct form II transposed. First-order sections
/// carry `b2 == a2 == 0`, so the whole crossover is a single section type.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    kind: SectionKind,
    q: f32,
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    w1: f32,
    w2: f32,
}

impl Biquad {
    fn new(kind: SectionKind, q: f32) -> Self {
        Self {
            kind,
            q,
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
            w1: 0.0,
            w2: 0.0,
        }
    }

    /// Recompute coefficients for `cutoff` without touching the section state.
    fn update(&mut self, sample_rate: f32, cutoff: f32) {
        let cutoff = clamp_cutoff(cutoff, sample_rate);

        match self.kind {
            SectionKind::LowPass | SectionKind::HighPass => {
                let omega = TWO_PI * cutoff / sample_rate;
                let (sinw, cosw) = omega.sin_cos();
                let alpha = sinw / (2.0 * self.q.max(0.1));
                let a0 = 1.0 + alpha;

                let (b0, b1, b2) = if self.kind == SectionKind::LowPass {
                    let half = (1.0 - cosw) / 2.0;
                    (half, 1.0 - cosw, half)
                } else {
                    let half = (1.0 + cosw) / 2.0;
                    (half, -(1.0 + cosw), half)
                };

                self.b0 = b0 / a0;
                self.b1 = b1 / a0;
                self.b2 = b2 / a0;
                self.a1 = -2.0 * cosw / a0;
                self.a2 = (1.0 - alpha) / a0;
            }
            SectionKind::FirstOrderLowPass | SectionKind::FirstOrderHighPass => {
                // Bilinear transform of a 1st-order Butterworth section.
                let t = (PI * cutoff / sample_rate).tan();

                self.a1 = (t - 1.0) / (t + 1.0);
                self.a2 = 0.0;
                self.b2 = 0.0;

                if self.kind == SectionKind::FirstOrderLowPass {
                    self.b0 = t / (t + 1.0);
                    self.b1 = self.b0;
                } else {
                    self.b0 = 1.0 / (t + 1.0);
                    self.b1 = -self.b0;
                }
            }
        }
    }

    #[inline(always)]
    fn process(&mut self, input: f32) -> f32 {
        let out = self.b0 * input + self.w1;
        self.w1 = self.b1 * input - self.a1 * out + self.w2;
        self.w2 = self.b2 * input - self.a2 * out;
        out
    }

    fn reset(&mut self) {
        self.w1 = 0.0;
        self.w2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(crossover: &mut LinkwitzRileyCrossover, input: f32, samples: usize) -> (f32, f32) {
        let mut out = (0.0, 0.0);
        for _ in 0..samples {
            out = crossover.process(input);
        }
        out
    }

    #[test]
    fn test_crossover_at_dc() {
        let mut crossover = LinkwitzRileyCrossover::new(48000.0, 2000.0, CrossoverOrder::Fourth);

        // DC settles into the low band; the high band rejects it.
        let (lp, hp) = settle(&mut crossover, 1.0, 4096);
        assert!((lp - 1.0).abs() < 0.01, "lowpass dc {lp}");
        assert!(hp.abs() < 0.01, "highpass dc {hp}");
    }

    #[test]
    fn test_crossover_sum() {
        let mut crossover = LinkwitzRileyCrossover::new(48000.0, 2000.0, CrossoverOrder::Fourth);

        let (lp, hp) = settle(&mut crossover, 1.0, 4096);
        assert!((lp + hp - 1.0).abs() < 0.01, "dc sum {}", lp + hp);
    }

    #[test]
    fn test_order_switching() {
        let mut crossover = LinkwitzRileyCrossover::new(48000.0, 2000.0, CrossoverOrder::Fourth);
        assert_eq!(crossover.order(), CrossoverOrder::Fourth);

        crossover.set_order(CrossoverOrder::Sixth);
        assert_eq!(crossover.order(), CrossoverOrder::Sixth);
    }

    /// The defining Linkwitz-Riley property: each band sits at -6 dB at the crossover
    /// frequency, and the two bands recombine at unity magnitude.
    #[test]
    fn bands_are_minus_six_db_and_sum_to_unity() {
        let sample_rate = 48_000.0;
        let crossover_hz = 2000.0;
        let mut crossover =
            LinkwitzRileyCrossover::new(sample_rate, crossover_hz, CrossoverOrder::Fourth);

        let increment = crossover_hz / sample_rate;
        let mut phase = 0.0f32;

        // Let the transient die out first.
        for _ in 0..8192 {
            phase = (phase + increment).fract();
            crossover.process((TWO_PI * phase).sin());
        }

        let window = 8192;
        let (mut lp_energy, mut hp_energy, mut sum_energy) = (0.0, 0.0, 0.0);
        for _ in 0..window {
            phase = (phase + increment).fract();
            let x = (TWO_PI * phase).sin();
            let (lp, hp) = crossover.process(x);
            lp_energy += lp * lp;
            hp_energy += hp * hp;
            sum_energy += (lp + hp) * (lp + hp);
        }

        let rms = |energy: f32| (energy / window as f32).sqrt();
        let reference = 1.0f32 / 2.0f32.sqrt();

        let lp_rms = rms(lp_energy);
        let hp_rms = rms(hp_energy);
        let sum_rms = rms(sum_energy);

        assert!(
            (lp_rms - reference * 0.5).abs() < 0.02,
            "low band should be -6 dB at crossover: {lp_rms}"
        );
        assert!(
            (hp_rms - reference * 0.5).abs() < 0.02,
            "high band should be -6 dB at crossover: {hp_rms}"
        );
        assert!(
            (sum_rms - reference).abs() < 0.02,
            "summed bands should be magnitude-flat: {sum_rms} vs {reference}"
        );
    }

    #[test]
    fn higher_orders_stay_stable_and_flat() {
        for order in [
            CrossoverOrder::Sixth,
            CrossoverOrder::Eighth,
            CrossoverOrder::Tenth,
        ] {
            let mut crossover = LinkwitzRileyCrossover::new(48_000.0, 2000.0, order);
            let (lp, hp) = settle(&mut crossover, 0.5, 8192);

            assert!(lp.is_finite() && hp.is_finite(), "{order:?} went non-finite");
            assert!((lp - 0.5).abs() < 0.01, "{order:?} lowpass dc {lp}");
            assert!(hp.abs() < 0.01, "{order:?} highpass dc {hp}");
        }
    }
}
