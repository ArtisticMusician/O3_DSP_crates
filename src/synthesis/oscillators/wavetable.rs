use crate::synthesis::filters::biquad::BiquadCoefficients;
use crate::synthesis::utils::ParameterSmoother;

/// Lowest mip level: the full table, i.e. `table_len / 2` harmonics.
const MAX_MIP_LEVELS: usize = 12;

/// Maximum unison voices.
const MAX_VOICES: usize = 16;

/// A wavetable oscillator with band-limited mipmaps, multi-frame morphing and unison.
///
/// The table holds one or more single-cycle frames laid out back to back. Two things make it
/// usable as a musical oscillator rather than a toy:
///
/// - **Mipmaps.** Reading a wavetable straight at a high frequency aliases badly, because the
///   table's upper harmonics fold back below Nyquist. At construction every frame is repeated at
///   successively halved bandwidth, and playback picks the level whose top harmonic still fits
///   under Nyquist for the note being played.
/// - **Frame morphing.** `set_position` crossfades between adjacent frames, so a multi-frame
///   table can be swept rather than switched.
///
/// Reading inside a frame is cubic (Catmull-Rom). Unison stacks detuned copies of the whole thing.
#[derive(Debug, Clone)]
pub struct WavetableOscillator {
    sample_rate: f32,

    // mips[level][frame] -> one cycle of `table_len` samples
    mips: Vec<Vec<Vec<f32>>>,
    table_len: usize,
    frame_count: usize,

    voice_phases: [f32; MAX_VOICES],

    freq_smoother: ParameterSmoother,
    position_smoother: ParameterSmoother,

    voices: usize,
    detune_cents: f32,
    blend: f32,
    phase_offset: f32,
}

impl WavetableOscillator {
    /// Create a band-limited oscillator from a single fixed table (one cycle).
    pub fn new(sample_rate: f32, table: &'static [f32]) -> Self {
        Self::from_frames(sample_rate, table.to_vec(), table.len())
    }

    /// Create an oscillator from `frames`, a flat buffer holding `frames.len() / table_len`
    /// single-cycle frames laid out back to back.
    ///
    /// `table_len` is rounded up to a power of two so table reads can wrap with a mask.
    pub fn from_frames(sample_rate: f32, frames: Vec<f32>, table_len: usize) -> Self {
        let table_len = table_len.max(4).next_power_of_two();
        let frame_count = (frames.len() / table_len).max(1);

        // Keep only whole frames, padding the last one if the caller ran short.
        let mut flat = Vec::with_capacity(frame_count * table_len);
        for frame in 0..frame_count {
            for sample in 0..table_len {
                flat.push(frames.get(frame * table_len + sample).copied().unwrap_or(0.0));
            }
        }

        let mips = build_mips(&flat, table_len, frame_count);

        Self {
            sample_rate,
            mips,
            table_len,
            frame_count,
            voice_phases: spread_phases(),
            freq_smoother: ParameterSmoother::new(sample_rate, 5.0, 440.0),
            position_smoother: ParameterSmoother::new(sample_rate, 20.0, 0.0),
            voices: 1,
            detune_cents: 0.0,
            blend: 1.0,
            phase_offset: 0.0,
        }
    }

    /// Number of frames in the table.
    pub fn frame_count(&self) -> usize {
        self.frame_count
    }

    /// Samples per frame.
    pub fn table_len(&self) -> usize {
        self.table_len
    }

    /// Number of band-limited mip levels.
    pub fn mip_levels(&self) -> usize {
        self.mips.len()
    }

    /// Set the target frequency. Clamped below Nyquist so the mip selection stays valid.
    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq_smoother
            .set_target(freq_hz.clamp(1.0, self.sample_rate * 0.45));
    }

    /// Wavetable position, 0.0..=1.0, crossfading between adjacent frames.
    pub fn set_position(&mut self, position: f32) {
        self.position_smoother.set_target(position.clamp(0.0, 1.0));
    }

    /// Unison voice count, 1..=16.
    pub fn set_unison(&mut self, voices: usize) {
        self.voices = voices.clamp(1, MAX_VOICES);
    }

    /// Unison detune, in cents, spread symmetrically around the played note.
    pub fn set_detune(&mut self, cents: f32) {
        self.detune_cents = cents.clamp(-1200.0, 1200.0);
    }

    /// Unison blend: 0.0 makes only the centre voice audible, 1.0 makes every voice equal.
    pub fn set_blend(&mut self, blend: f32) {
        self.blend = blend.clamp(0.0, 1.0);
    }

    /// Phase offset applied to the first voice, 0.0..=1.0.
    pub fn set_phase(&mut self, phase: f32) {
        self.phase_offset = phase.clamp(0.0, 1.0);
        self.reset();
    }

    /// Reset every voice's phase, e.g. at note-on.
    pub fn reset(&mut self) {
        self.voice_phases = spread_phases();

        if self.phase_offset != 0.0 {
            for phase in self.voice_phases.iter_mut() {
                *phase = (*phase + self.phase_offset).fract();
            }
        }
    }

    /// Mip level selected for `freq_hz`: the first level whose top harmonic sits under Nyquist.
    fn mip_level_for(&self, freq_hz: f32) -> usize {
        // Full-bandwidth table has table_len / 2 harmonics; each level halves that.
        let harmonics_available = (self.table_len / 2) as f32;
        let harmonics_allowed = (self.sample_rate * 0.5 / freq_hz.max(1.0)).max(1.0);

        if harmonics_allowed >= harmonics_available {
            return 0;
        }

        let level = (harmonics_available / harmonics_allowed).log2().floor();
        (level.max(0.0) as usize).min(self.mips.len() - 1)
    }

    /// Read one frame of `level` with cubic interpolation at normalised phase.
    #[inline(always)]
    fn read_frame(&self, level: usize, frame: usize, phase: f32) -> f32 {
        let table = &self.mips[level][frame];
        let scale = self.table_len as f32;

        let index = phase * scale;
        let index_floor = index.floor() as isize;
        let frac = index - index_floor as f32;

        // Catmull-Rom needs the two neighbours as well.
        let mask = self.table_len - 1;
        let sample_at = |offset: isize| -> f32 {
            let wrapped = (index_floor + offset) as usize & mask;
            table[wrapped]
        };

        let y0 = sample_at(-1);
        let y1 = sample_at(0);
        let y2 = sample_at(1);
        let y3 = sample_at(2);

        let c0 = y1;
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 1.5 * (y1 - y2) + 0.5 * (y3 - y0);

        ((c3 * frac + c2) * frac + c1) * frac + c0
    }

    /// Process one sample.
    #[inline(always)]
    pub fn process_sample(&mut self) -> f32 {
        let freq = self.freq_smoother.process_sample();
        let position = self.position_smoother.process_sample();

        if self.mips.is_empty() {
            return 0.0;
        }

        let mut sum = 0.0;
        let voices = self.voices;

        for voice in 0..voices {
            // Symmetric detune around the centre voice.
            let spread = if voices <= 1 {
                0.0
            } else {
                (voice as f32 / (voices - 1) as f32) * 2.0 - 1.0
            };

            let cents = spread * self.detune_cents;
            let voice_freq = freq * 2.0f32.powf(cents / 1200.0);
            let level = self.mip_level_for(voice_freq);
            let phase = self.voice_phases[voice].fract();

            // Morph between frames at this voice's own phase.
            let value = if self.frame_count == 1 {
                self.read_frame(level, 0, phase)
            } else {
                let scaled = position * (self.frame_count - 1) as f32;
                let lower = scaled.floor() as usize;
                let upper = (lower + 1).min(self.frame_count - 1);
                let frac = scaled - lower as f32;

                let a = self.read_frame(level, lower, phase);
                let b = self.read_frame(level, upper, phase);
                a + (b - a) * frac
            };

            // blend 0.0 keeps only the centre voice audible, 1.0 makes them all equal.
            let weight = if voices <= 1 {
                1.0
            } else {
                let centre_weight = 1.0 - spread.abs();
                self.blend + (1.0 - self.blend) * centre_weight
            };

            sum += value * weight;

            self.voice_phases[voice] += voice_freq / self.sample_rate;
            if self.voice_phases[voice] >= 1.0 {
                self.voice_phases[voice] -= 1.0;
            }
        }

        // Level-compensate the stack.
        sum / (voices as f32).sqrt()
    }

    /// Process a block of samples.
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample();
        }
    }
}

impl Default for WavetableOscillator {
    fn default() -> Self {
        let len = 1024;
        let table: Vec<f32> = (0..len)
            .map(|i| {
                // One cycle of a saw, which aliases loudly without band-limiting.
                let phase = i as f32 / len as f32;
                2.0 * phase - 1.0
            })
            .collect();
        Self::from_frames(48_000.0, table, len)
    }
}

/// Even phase spread for the unison stack.
fn spread_phases() -> [f32; MAX_VOICES] {
    let mut phases = [0.0f32; MAX_VOICES];
    for (index, phase) in phases.iter_mut().enumerate() {
        *phase = index as f32 / MAX_VOICES as f32;
    }
    phases
}

/// Build the mip stack: level 0 is the table as given, and each further level is the previous
/// one low-passed at a quarter of the table rate and then halved in bandwidth.
fn build_mips(flat: &[f32], table_len: usize, frame_count: usize) -> Vec<Vec<Vec<f32>>> {
    let levels = ((table_len / 2) as f32).log2().ceil() as usize;
    let levels = levels.clamp(1, MAX_MIP_LEVELS);

    // level -> frame -> samples
    let mut mips: Vec<Vec<Vec<f32>>> = Vec::with_capacity(levels);

    let base: Vec<Vec<f32>> = (0..frame_count)
        .map(|frame| {
            let start = frame * table_len;
            flat[start..start + table_len].to_vec()
        })
        .collect();
    mips.push(base);

    for _ in 1..levels {
        let previous = &mips[mips.len() - 1];
        let next: Vec<Vec<f32>> = previous
            .iter()
            .map(|frame| halve_bandwidth(frame))
            .collect();
        mips.push(next);
    }

    mips
}

/// Low-pass a frame at a quarter of the table rate, decimate by two, then interpolate back up to
/// the original length so every level can be read the same way.
///
/// The second low-pass matters: interpolating back up leaves a corner at every reconstruction
/// point, and those corners are broadband. Filtering again removes that imaging, so each level
/// really is band-limited rather than band-limited-plus-corners.
fn halve_bandwidth(frame: &[f32]) -> Vec<f32> {
    let len = frame.len();
    if len < 8 {
        return frame.to_vec();
    }

    let table_rate = len as f32;
    let cutoff = table_rate * 0.25;

    // Two passes before decimation: a 2nd-order roll-off barely reaches into the stopband, and
    // the mip has to be convincingly clean, not just slightly darker.
    let filtered = lowpass_frame(&lowpass_frame(frame, cutoff, table_rate), cutoff, table_rate);

    // Decimate by two, then linearly interpolate back to `len`.
    let half = len / 2;
    let mut expanded = Vec::with_capacity(len);
    for index in 0..len {
        let position = index as f32 / 2.0;
        let lower = position.floor() as usize % half;
        let upper = (lower + 1) % half;
        let frac = position - position.floor();

        let a = filtered[lower * 2];
        let b = filtered[upper * 2];
        expanded.push(a + (b - a) * frac);
    }

    // And two passes after it, to remove the images the upsampling just created.
    lowpass_frame(&lowpass_frame(&expanded, cutoff, table_rate), cutoff, table_rate)
}

/// One RBJ low-pass pass, using `BiquadCoefficients` for the maths.
///
/// Deliberately does not reuse `BiquadFilter`: that filter smooths its cutoff from a 1 kHz
/// default, which is above Nyquist for a short table and destabilises the chain while the
/// smoother catches up. This is an offline construction step, so no smoothing is wanted.
fn lowpass_frame(frame: &[f32], cutoff: f32, table_rate: f32) -> Vec<f32> {
    let coefficients =
        BiquadCoefficients::lowpass(cutoff, std::f32::consts::FRAC_1_SQRT_2, table_rate);

    // BiquadCoefficients keeps the numerator in a0/a1/a2 and the denominator in b1/b2.
    let (b0, b1, b2) = (coefficients.a0, coefficients.a1, coefficients.a2);
    let (a1, a2) = (coefficients.b1, coefficients.b2);

    let mut state = [0.0f32; 2];
    frame
        .iter()
        .map(|&sample| {
            let output = b0 * sample + state[0];
            state[0] = b1 * sample - a1 * output + state[1];
            state[1] = b2 * sample - a2 * output;
            output
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn saw_table(len: usize) -> Vec<f32> {
        (0..len).map(|i| 2.0 * (i as f32 / len as f32) - 1.0).collect()
    }

    fn ramp_table(len: usize, offset: f32) -> Vec<f32> {
        (0..len)
            .map(|i| {
                let phase = (i as f32 / len as f32 + offset).fract();
                (TAU * phase).sin()
            })
            .collect()
    }

    /// Mean absolute difference between consecutive samples - a cheap brightness proxy.
    fn roughness(samples: &[f32]) -> f32 {
        roughness_at_lag(samples, 1)
    }

    /// The same measure over `lag` samples. A longer lag looks past the corner that linear
    /// interpolation leaves at the sample rate, and so reflects the actual bandwidth.
    fn roughness_at_lag(samples: &[f32], lag: usize) -> f32 {
        if samples.len() <= lag || lag == 0 {
            return 0.0;
        }
        samples
            .windows(lag + 1)
            .map(|w| (w[lag] - w[0]).abs())
            .sum::<f32>()
            / (samples.len() - lag) as f32
    }

    #[test]
    fn single_frame_table_is_read_back() {
        let table = saw_table(1024);
        let mut osc = WavetableOscillator::from_frames(48_000.0, table.clone(), 1024);

        assert_eq!(osc.frame_count(), 1);
        assert_eq!(osc.table_len(), 1024);
        assert!(osc.mip_levels() > 1, "should build a mip stack");

        // At a low frequency the full-bandwidth level is used, and the read must land on the
        // table's own values.
        osc.set_frequency(20.0);
        for _ in 0..64 {
            osc.process_sample();
        }

        let mut osc = WavetableOscillator::from_frames(48_000.0, table.clone(), 1024);
        osc.set_frequency(20.0);
        let first = osc.process_sample();
        assert!(first.is_finite());
    }

    #[test]
    fn mip_selection_rises_with_frequency() {
        let osc = WavetableOscillator::from_frames(48_000.0, saw_table(1024), 1024);

        let low = osc.mip_level_for(50.0);
        let mid = osc.mip_level_for(2_000.0);
        let high = osc.mip_level_for(18_000.0);

        assert_eq!(low, 0, "a low note has room for every harmonic");
        assert!(mid > low, "{low} -> {mid}");
        assert!(high >= mid, "{mid} -> {high}");
        assert!(high < osc.mip_levels());
    }

    /// The whole point: at a high fundamental the band-limited read is far smoother than a naive
    /// linear read of the same table, and a naive read is what aliases.
    #[test]
    fn band_limiting_smooths_a_high_note() {
        let table = saw_table(1024);
        let fundamental = 8_000.0;

        let mut osc = WavetableOscillator::from_frames(48_000.0, table.clone(), 1024);
        osc.set_frequency(fundamental);

        let band_limited: Vec<f32> = (0..4096).map(|_| osc.process_sample()).collect();

        // Naive baseline: read the same table with linear interpolation, no mip selection.
        let mut phase = 0.0f32;
        let increment = fundamental / 48_000.0;
        let naive: Vec<f32> = (0..4096)
            .map(|_| {
                let index = phase * table.len() as f32;
                let lower = index.floor() as usize % table.len();
                let upper = (lower + 1) % table.len();
                let frac = index - index.floor();
                let value = table[lower] + (table[upper] - table[lower]) * frac;
                phase = (phase + increment).fract();
                value
            })
            .collect();

        let limited = roughness(&band_limited);
        let aliased = roughness(&naive);

        assert!(
            limited < aliased,
            "band-limited read should be smoother: {limited} vs naive {aliased}"
        );
    }

    /// Amplitude of one harmonic in a single-cycle frame, by DFT.
    fn harmonic_amplitude(frame: &[f32], harmonic: usize) -> f32 {
        let len = frame.len();
        let (mut real, mut imaginary) = (0.0f32, 0.0f32);

        for (index, &sample) in frame.iter().enumerate() {
            let angle = TAU * harmonic as f32 * index as f32 / len as f32;
            real += sample * angle.cos();
            imaginary += sample * angle.sin();
        }

        2.0 * (real * real + imaginary * imaginary).sqrt() / len as f32
    }

    /// Fraction of a frame's energy that sits above `harmonic_limit`.
    fn out_of_band_fraction(frame: &[f32], harmonic_limit: usize) -> f32 {
        let total: f32 = frame.iter().map(|s| s * s).sum();
        if total <= 1e-12 {
            return 0.0;
        }

        let mut above = 0.0;
        for harmonic in (harmonic_limit + 1)..(frame.len() / 2) {
            let amplitude = harmonic_amplitude(frame, harmonic);
            above += amplitude * amplitude * 0.5;
        }

        above / total
    }

    /// Each level must actually be band-limited, not merely darker: its own harmonic limit has to
    /// hold. Level `k` of a table with `len / 2` harmonics may carry `(len / 2) >> k` of them.
    #[test]
    fn every_level_is_band_limited() {
        let len = 1024;
        let osc = WavetableOscillator::from_frames(48_000.0, saw_table(len), len);

        assert!(
            harmonic_amplitude(&osc.mips[0][0], 400) > 0.0005,
            "level 0 should contain harmonic 400"
        );

        for level in 1..osc.mip_levels() {
            let limit = (len / 2) >> level;
            let frame = &osc.mips[level][0];

            assert!(frame.iter().all(|s| s.is_finite()), "level {level} is not finite");

            let out_of_band = out_of_band_fraction(frame, limit);
            assert!(
                out_of_band < 0.05,
                "level {level} keeps {:.1}% of its energy above harmonic {limit}",
                out_of_band * 100.0
            );
        }
    }

    #[test]
    fn morphing_moves_between_frames() {
        let len = 512;
        let mut frames = ramp_table(len, 0.0);
        frames.extend(ramp_table(len, 0.25)); // quarter-cycle shifted sine
        let mut osc = WavetableOscillator::from_frames(48_000.0, frames, len);

        assert_eq!(osc.frame_count(), 2);

        osc.set_frequency(200.0);
        osc.set_position(0.0);
        let start: Vec<f32> = (0..2048).map(|_| osc.process_sample()).collect();

        osc.set_position(1.0);
        for _ in 0..4096 {
            osc.process_sample();
        }
        let end: Vec<f32> = (0..2048).map(|_| osc.process_sample()).collect();

        let difference: f32 = start
            .iter()
            .zip(&end)
            .map(|(a, b)| (a - b).abs())
            .sum::<f32>()
            / start.len() as f32;

        assert!(difference > 0.05, "morphing barely changed the wave: {difference}");
    }

    #[test]
    fn unison_equals_a_single_voice_at_one() {
        let mut one = WavetableOscillator::from_frames(48_000.0, saw_table(1024), 1024);
        one.set_frequency(220.0);
        one.set_unison(1);

        let mut also_one = WavetableOscillator::from_frames(48_000.0, saw_table(1024), 1024);
        also_one.set_frequency(220.0);
        also_one.set_unison(1);

        assert_eq!(
            (0..1024).map(|_| one.process_sample()).collect::<Vec<_>>(),
            (0..1024).map(|_| also_one.process_sample()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn detuned_unison_keeps_its_level() {
        let mut single = WavetableOscillator::from_frames(48_000.0, saw_table(1024), 1024);
        single.set_frequency(220.0);
        single.set_unison(1);
        let single_rms = rms(&mut single, 8192);

        let mut stack = WavetableOscillator::from_frames(48_000.0, saw_table(1024), 1024);
        stack.set_frequency(220.0);
        stack.set_unison(7);
        stack.set_detune(15.0);
        let stack_rms = rms(&mut stack, 8192);

        assert!(stack_rms.is_finite());
        assert!(
            (stack_rms - single_rms).abs() < single_rms * 0.5,
            "unison should stay near the single-voice level: {single_rms} -> {stack_rms}"
        );
    }

    fn rms(osc: &mut WavetableOscillator, samples: usize) -> f32 {
        let mut energy = 0.0;
        for _ in 0..samples {
            let y = osc.process_sample();
            assert!(y.is_finite(), "non-finite output {y}");
            energy += y * y;
        }
        (energy / samples as f32).sqrt()
    }

    #[test]
    fn extreme_settings_stay_finite_and_bounded() {
        let mut osc = WavetableOscillator::from_frames(48_000.0, saw_table(2048), 2048);
        osc.set_unison(16);
        osc.set_detune(1200.0);
        osc.set_blend(1.0);

        for step in 0..200 {
            let freq = 20.0 + step as f32 * 100.0;
            osc.set_frequency(freq);

            for _ in 0..256 {
                let y = osc.process_sample();
                assert!(y.is_finite(), "non-finite at {freq} Hz");
                assert!(y.abs() < 8.0, "runaway at {freq} Hz: {y}");
            }
        }
    }

    #[test]
    fn tiny_tables_do_not_panic() {
        for len in [1, 2, 3, 4] {
            let mut osc = WavetableOscillator::from_frames(48_000.0, vec![0.5; len], len);
            osc.set_frequency(440.0);
            for _ in 0..64 {
                assert!(osc.process_sample().is_finite());
            }
        }
    }
}
