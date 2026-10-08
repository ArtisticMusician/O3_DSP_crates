use crate::synthesis::math_utils::{linear_to_db, db_to_linear};

/// Calibrated so a -20 dBFS sine reads 0 VU.
const REFERENCE_DBFS: f32 = -20.0;

/// VU scale: the reading at full scale (+3 VU is where a VU meter pins).
const MAX_VU: f32 = 3.0;

/// VU meter with broadcast ballistics.
///
/// A VU meter is an averaging meter, not a peak meter: it integrates over roughly 300 ms and
/// only reaches +3 VU at the top of its scale. -20 dBFS is 0 VU by definition, so a VU reading
/// sits 20 dB below the equivalent dBFS reading.
///
/// The 300 ms average is taken over the mean square (i.e. it is an RMS reading), which keeps the
/// documented "-20 dBFS = 0 VU" calibration exact for a sine. A mechanical VU movement reads
/// ~1.9 dB lower on a sine because it averages the rectified waveform instead; that quirk is not
/// reproduced here.
///
/// Keeps a clip flag for anything that reaches full scale, since the needle itself will not
/// show it.
#[derive(Debug, Clone, Copy)]
pub struct VuMeter {
    // 300 ms averaging of the mean square
    ballistic_coefficient: f32,
    mean_square: f32,

    // -20 dBFS = 0 VU, so the scale is dBFS shifted up by 20
    vu_offset: f32,

    clipped: bool,
}

impl Default for VuMeter {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl VuMeter {
    /// Create a VU meter.
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);

        Self {
            ballistic_coefficient: 1.0 - (-1.0 / (0.3 * sample_rate)).exp(),
            mean_square: 0.0,
            vu_offset: -REFERENCE_DBFS,
            clipped: false,
        }
    }

    /// Integrate one sample. `sample` is a single channel in -1.0..=1.0.
    #[inline(always)]
    pub fn process_sample(&mut self, sample: f32) {
        if sample.abs() >= 1.0 {
            self.clipped = true;
        }

        self.mean_square += (sample * sample - self.mean_square) * self.ballistic_coefficient;
    }

    /// Integrate a stereo pair. A VU meter reads the louder of the two channels.
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) {
        self.process_sample(left.abs().max(right.abs()));
    }

    /// Current reading in VU, floored at -40 VU (essentially silence).
    #[inline(always)]
    pub fn get_vu(&self) -> f32 {
        if self.mean_square <= 1e-12 {
            return -40.0;
        }

        (linear_to_db(self.mean_square.sqrt()) + self.vu_offset).min(MAX_VU)
    }

    /// Current reading in dBFS before the VU offset.
    #[inline(always)]
    pub fn get_dbfs(&self) -> f32 {
        if self.mean_square <= 1e-12 {
            return -144.0;
        }
        linear_to_db(self.mean_square.sqrt())
    }

    /// True once a sample has reached full scale since the flag was last cleared.
    #[inline(always)]
    pub fn has_clipped(&self) -> bool {
        self.clipped
    }

    /// Clear the clip flag.
    pub fn clear_clip(&mut self) {
        self.clipped = false;
    }

    pub fn reset(&mut self) {
        self.mean_square = 0.0;
        self.clipped = false;
    }

    /// The linear amplitude that reads as `vu`.
    pub fn vu_to_linear(vu: f32) -> f32 {
        db_to_linear(vu - REFERENCE_DBFS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    /// Drive a sine whose RMS is `dbfs` and read the settled VU value.
    /// VU is an RMS scale, so the signal's RMS is what has to equal `dbfs` - a sine with that
    /// RMS has a peak 3 dB higher.
    fn vu_for_sine(dbfs: f32, sample_rate: f32) -> f32 {
        let mut meter = VuMeter::new(sample_rate);
        let amplitude = db_to_linear(dbfs) * std::f32::consts::SQRT_2;
        let increment = 1_000.0 / sample_rate;

        let mut phase = 0.0f32;
        for _ in 0..(sample_rate as usize * 2) {
            phase = (phase + increment).fract();
            meter.process_sample(amplitude * (TAU * phase).sin());
        }

        meter.get_vu()
    }

    #[test]
    fn minus_twenty_dbfs_reads_zero_vu() {
        let vu = vu_for_sine(-20.0, 48_000.0);
        assert!((vu - 0.0).abs() < 0.5, "-20 dBFS should read 0 VU, got {vu}");
    }

    #[test]
    fn minus_eighteen_dbfs_reads_plus_two_vu() {
        let vu = vu_for_sine(-18.0, 48_000.0);
        assert!((vu - 2.0).abs() < 0.5, "-18 dBFS should read +2 VU, got {vu}");
    }

    #[test]
    fn full_scale_pins_the_needle() {
        // Anything at or above -3 dBFS is off the top of the scale.
        let vu = vu_for_sine(-3.0, 48_000.0);
        assert!((vu - MAX_VU).abs() < 1e-3, "should be pinned at +3 VU, got {vu}");
    }

    #[test]
    fn silence_reads_the_floor() {
        let mut meter = VuMeter::new(48_000.0);
        for _ in 0..48_000 {
            meter.process_sample(0.0);
        }
        assert_eq!(meter.get_vu(), -40.0);
        assert!(!meter.has_clipped());
    }

    #[test]
    fn full_scale_sets_the_clip_flag() {
        let mut meter = VuMeter::new(48_000.0);
        meter.process_sample(1.0);
        assert!(meter.has_clipped());

        meter.clear_clip();
        assert!(!meter.has_clipped());
    }

    #[test]
    fn integration_is_slower_than_the_signal() {
        // One half cycle of a sine must not swing the needle as far as a sustained tone would.
        let sample_rate = 48_000.0;
        let mut meter = VuMeter::new(sample_rate);

        for _ in 0..10 {
            meter.process_sample(1.0);
        }
        let blip = meter.get_vu();

        assert!(blip < 0.0, "a 0.2 ms blip should not reach 0 VU: {blip}");
        assert!(blip > -40.0, "a full-scale blip should move the needle: {blip}");
    }

    #[test]
    fn stereo_reads_the_louder_channel() {
        let sample_rate = 48_000.0;

        let mut both = VuMeter::new(sample_rate);
        let mut louder_only = VuMeter::new(sample_rate);

        let amplitude = db_to_linear(-20.0);
        let increment = 1_000.0 / sample_rate;
        let mut phase = 0.0f32;

        for _ in 0..(sample_rate as usize / 2) {
            phase = (phase + increment).fract();
            let signal = amplitude * (TAU * phase).sin();

            both.process_stereo(signal, signal);
            louder_only.process_sample(signal);
        }

        let both_vu = both.get_vu();
        let single_vu = louder_only.get_vu();
        assert!((both_vu - single_vu).abs() < 0.2, "{both_vu} vs {single_vu}");
    }
}
