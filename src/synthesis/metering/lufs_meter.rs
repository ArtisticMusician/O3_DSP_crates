use crate::synthesis::math_utils::linear_to_db;
use std::f32::consts::PI;

/// Silence floor, matching `rms_meter.rs`.
const SILENCE_LUFS: f32 = -144.0;

/// The BS.1770 offset: it cancels the K-weighting gain at 1 kHz, so a stereo 1 kHz sine
/// reads the same LUFS as its dBFS level.
const LOUDNESS_OFFSET: f32 = -0.691;

/// Absolute gate: blocks quieter than this never count.
const ABSOLUTE_GATE_LUFS: f32 = -70.0;

/// Relative gate: blocks more than this far below the ungated average never count.
const RELATIVE_GATE_LU: f32 = -10.0;

/// BS.1770 K-weighting pre-filter (high shelf) and RLB high-pass, as filter prototypes.
const SHELF_FREQUENCY: f32 = 1681.974_5;
const SHELF_GAIN_DB: f32 = 3.999_84;
const SHELF_Q: f32 = 0.707_175_2;
const HIGHPASS_FREQUENCY: f32 = 38.135_47;
const HIGHPASS_Q: f32 = 0.500_327;

/// ITU-R BS.1770 loudness meter.
///
/// K-weights each channel, sums their mean squares, and reports:
/// - **momentary** over 400 ms
/// - **short-term** over 3 s
/// - **integrated**, gated (BS.1770-4): 400 ms blocks stepped every 100 ms, with a -70 LUFS
///   absolute gate and then a -10 LU relative gate
///
/// Silence reports [`SILENCE_LUFS`]. The K-weighting filters are derived from the standard's
/// prototype parameters, so the meter calibrates at any sample rate rather than only at 48 kHz.
#[derive(Debug, Clone)]
pub struct LufsMeter {
    shelf: Coefs,
    highpass: Coefs,

    // Direct form II transposed state, one pair per channel
    shelf_state: [[f32; 2]; 2],
    highpass_state: [[f32; 2]; 2],

    momentary: SlidingWindow,
    short_term: SlidingWindow,

    // 400 ms block means, sampled every 100 ms, for the gated integrated measure
    blocks: Vec<f32>,
    block_interval: u32,
    samples_since_block: u32,
}

impl LufsMeter {
    /// Create a meter for `sample_rate`.
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(8_000.0);

        Self {
            shelf: high_shelf(SHELF_FREQUENCY, SHELF_GAIN_DB, SHELF_Q, sample_rate),
            highpass: high_pass(HIGHPASS_FREQUENCY, HIGHPASS_Q, sample_rate),

            shelf_state: [[0.0; 2]; 2],
            highpass_state: [[0.0; 2]; 2],

            momentary: SlidingWindow::new(window_samples(sample_rate, 400.0)),
            short_term: SlidingWindow::new(window_samples(sample_rate, 3_000.0)),

            blocks: Vec::new(),
            block_interval: window_samples(sample_rate, 100.0).max(1) as u32,
            samples_since_block: 0,
        }
    }

    /// Feed one mono sample.
    #[inline(always)]
    pub fn process_sample(&mut self, sample: f32) {
        let weighted = self.k_weight(0, sample);
        self.push(weighted * weighted);
    }

    /// Feed a stereo pair. Both channels carry the standard's weight of 1.0.
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) {
        let left = self.k_weight(0, left);
        let right = self.k_weight(1, right);
        self.push(left * left + right * right);
    }

    #[inline(always)]
    fn k_weight(&mut self, channel: usize, sample: f32) -> f32 {
        let staged = process(&self.shelf, &mut self.shelf_state[channel], sample);
        process(&self.highpass, &mut self.highpass_state[channel], staged)
    }

    #[inline(always)]
    fn push(&mut self, weighted_square: f32) {
        self.momentary.push(weighted_square);
        self.short_term.push(weighted_square);

        // Snapshot the 400 ms window every 100 ms - the standard's 75% overlap.
        self.samples_since_block += 1;
        if self.samples_since_block >= self.block_interval {
            self.samples_since_block = 0;
            if self.momentary.is_full() {
                self.blocks.push(self.momentary.mean());
            }
        }
    }

    /// Momentary loudness (400 ms window), or the silence floor before it fills.
    pub fn momentary(&self) -> f32 {
        if !self.momentary.is_full() {
            return SILENCE_LUFS;
        }
        loudness(self.momentary.mean())
    }

    /// Short-term loudness (3 s window), or the silence floor before it fills.
    pub fn short_term(&self) -> f32 {
        if !self.short_term.is_full() {
            return SILENCE_LUFS;
        }
        loudness(self.short_term.mean())
    }

    /// Gated integrated loudness over everything fed so far.
    pub fn integrated(&self) -> f32 {
        if self.blocks.is_empty() {
            return SILENCE_LUFS;
        }

        // Absolute gate first.
        let above_absolute: Vec<f32> = self
            .blocks
            .iter()
            .copied()
            .filter(|block| loudness(*block) > ABSOLUTE_GATE_LUFS)
            .collect();

        if above_absolute.is_empty() {
            return SILENCE_LUFS;
        }

        // Then the relative gate, measured from the absolute-gated average.
        let absolute_mean = above_absolute.iter().sum::<f32>() / above_absolute.len() as f32;
        let relative_threshold = loudness(absolute_mean) + RELATIVE_GATE_LU;

        let gated: Vec<f32> = above_absolute
            .into_iter()
            .filter(|block| loudness(*block) > relative_threshold)
            .collect();

        if gated.is_empty() {
            return SILENCE_LUFS;
        }

        loudness(gated.iter().sum::<f32>() / gated.len() as f32)
    }

    /// Number of 400 ms blocks that have been measured so far.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Clear everything, including the integrated history.
    pub fn reset(&mut self) {
        self.shelf_state = [[0.0; 2]; 2];
        self.highpass_state = [[0.0; 2]; 2];
        self.momentary.clear();
        self.short_term.clear();
        self.blocks.clear();
        self.samples_since_block = 0;
    }
}

/// Mean square to LUFS.
fn loudness(mean_square: f32) -> f32 {
    if mean_square <= 1e-12 {
        return SILENCE_LUFS;
    }

    let lufs = LOUDNESS_OFFSET + linear_to_db(mean_square.sqrt());
    lufs.max(SILENCE_LUFS)
}

fn window_samples(sample_rate: f32, milliseconds: f32) -> usize {
    ((milliseconds * 0.001 * sample_rate).ceil() as usize).max(1)
}

/// Normalised biquad coefficients (a0 == 1), direct form II transposed.
#[derive(Debug, Clone, Copy)]
struct Coefs {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[inline(always)]
fn process(coefs: &Coefs, state: &mut [f32; 2], input: f32) -> f32 {
    let output = coefs.b0 * input + state[0];
    state[0] = coefs.b1 * input - coefs.a1 * output + state[1];
    state[1] = coefs.b2 * input - coefs.a2 * output;
    output
}

/// BS.1770 K-weighting stage 1: high shelf. Unity at DC, `gain_db` at Nyquist.
fn high_shelf(f0: f32, gain_db: f32, q: f32, sample_rate: f32) -> Coefs {
    let k = (PI * f0 / sample_rate).tan();
    let vh = 10.0f32.powf(gain_db / 20.0);
    let vb = vh.powf(0.499_666_77);

    let a0 = 1.0 + k / q + k * k;

    Coefs {
        b0: (vh + vb * k / q + k * k) / a0,
        b1: 2.0 * (k * k - vh) / a0,
        b2: (vh - vb * k / q + k * k) / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// BS.1770 K-weighting stage 2: RLB high-pass.
fn high_pass(f0: f32, q: f32, sample_rate: f32) -> Coefs {
    let k = (PI * f0 / sample_rate).tan();
    let a0 = 1.0 + k / q + k * k;

    Coefs {
        b0: 1.0 / a0,
        b1: -2.0 / a0,
        b2: 1.0 / a0,
        a1: 2.0 * (k * k - 1.0) / a0,
        a2: (1.0 - k / q + k * k) / a0,
    }
}

/// Fixed-capacity ring of squared samples, reporting the mean of what it holds.
#[derive(Debug, Clone)]
struct SlidingWindow {
    values: Vec<f32>,
    write: usize,
    filled: usize,
}

impl SlidingWindow {
    fn new(capacity: usize) -> Self {
        Self {
            values: vec![0.0; capacity.max(1)],
            write: 0,
            filled: 0,
        }
    }

    #[inline(always)]
    fn push(&mut self, value: f32) {
        self.values[self.write] = value;
        self.write += 1;
        if self.write >= self.values.len() {
            self.write = 0;
        }
        if self.filled < self.values.len() {
            self.filled += 1;
        }
    }

    fn is_full(&self) -> bool {
        self.filled >= self.values.len()
    }

    fn mean(&self) -> f32 {
        if self.filled == 0 {
            return 0.0;
        }

        self.values.iter().take(self.filled).sum::<f32>() / self.filled as f32
    }

    fn clear(&mut self) {
        self.values.fill(0.0);
        self.write = 0;
        self.filled = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synthesis::math_utils::db_to_linear;
    use std::f32::consts::TAU;

    /// Feed a stereo sine at `dbfs` for `seconds`, then report the integrated loudness.
    fn integrated_for_sine(dbfs: f32, seconds: f32, sample_rate: f32) -> f32 {
        let mut meter = LufsMeter::new(sample_rate);
        let amplitude = db_to_linear(dbfs);
        let increment = 1_000.0 / sample_rate;
        let samples = (seconds * sample_rate) as usize;
        let mut phase = 0.0f32;

        for _ in 0..samples {
            phase = (phase + increment).fract();
            let sample = amplitude * (TAU * phase).sin();
            meter.process_stereo(sample, sample);
        }

        meter.integrated()
    }

    #[test]
    fn stereo_minus_23_dbfs_reads_minus_23_lufs() {
        let lufs = integrated_for_sine(-23.0, 5.0, 48_000.0);
        assert!((lufs + 23.0).abs() < 0.5, "expected -23 LUFS, got {lufs}");
    }

    #[test]
    fn stereo_minus_20_dbfs_reads_minus_20_lufs() {
        let lufs = integrated_for_sine(-20.0, 5.0, 48_000.0);
        assert!((lufs + 20.0).abs() < 0.5, "expected -20 LUFS, got {lufs}");
    }

    #[test]
    fn calibration_holds_at_44k1() {
        let lufs = integrated_for_sine(-23.0, 5.0, 44_100.0);
        assert!((lufs + 23.0).abs() < 0.5, "expected -23 LUFS at 44.1 kHz, got {lufs}");
    }

    #[test]
    fn silence_reports_the_floor() {
        let mut meter = LufsMeter::new(48_000.0);

        for _ in 0..(48_000 * 2) {
            meter.process_stereo(0.0, 0.0);
        }

        assert_eq!(meter.integrated(), SILENCE_LUFS);
    }

    #[test]
    fn quiet_passages_are_gated_out_of_the_integrated_measure() {
        // 2 s of -23 dBFS, then 4 s at the absolute gate floor. The silence must not drag the
        // integrated reading down towards it.
        let sample_rate = 48_000.0;
        let mut meter = LufsMeter::new(sample_rate);

        let loud = db_to_linear(-23.0);
        let increment = 1_000.0 / sample_rate;
        let mut phase = 0.0f32;

        for _ in 0..(sample_rate as usize * 2) {
            phase = (phase + increment).fract();
            let sample = loud * (TAU * phase).sin();
            meter.process_stereo(sample, sample);
        }
        for _ in 0..(sample_rate as usize * 4) {
            meter.process_stereo(0.0, 0.0);
        }

        let integrated = meter.integrated();
        assert!(meter.block_count() > 0);
        assert!(
            (integrated + 23.0).abs() < 1.5,
            "gating should hold the reading near -23 LUFS, got {integrated}"
        );
    }

    #[test]
    fn windows_need_to_fill_before_they_report() {
        let mut meter = LufsMeter::new(48_000.0);

        assert_eq!(meter.momentary(), SILENCE_LUFS);
        assert_eq!(meter.short_term(), SILENCE_LUFS);

        for _ in 0..(48_000 / 2) {
            meter.process_stereo(0.1, -0.1);
        }

        assert!(meter.momentary() > SILENCE_LUFS, "momentary should have filled");
        assert_eq!(meter.short_term(), SILENCE_LUFS, "3 s window should still be filling");
    }

    #[test]
    fn mono_and_stereo_agree_for_a_centred_signal() {
        // One channel at -23 dBFS mono is 3 dB quieter than the same signal on two channels.
        let sample_rate = 48_000.0;
        let mono = integrated_for_sine_mono(-23.0, 5.0, sample_rate);
        let stereo = integrated_for_sine(-23.0, 5.0, sample_rate);

        assert!(
            (stereo - mono - 3.01).abs() < 0.5,
            "stereo should sit 3 dB above mono: mono {mono}, stereo {stereo}"
        );
    }

    fn integrated_for_sine_mono(dbfs: f32, seconds: f32, sample_rate: f32) -> f32 {
        let mut meter = LufsMeter::new(sample_rate);
        let amplitude = db_to_linear(dbfs);
        let increment = 1_000.0 / sample_rate;
        let samples = (seconds * sample_rate) as usize;
        let mut phase = 0.0f32;

        for _ in 0..samples {
            phase = (phase + increment).fract();
            meter.process_sample(amplitude * (TAU * phase).sin());
        }

        meter.integrated()
    }
}
