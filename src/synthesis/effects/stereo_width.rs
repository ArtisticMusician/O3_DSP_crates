use crate::synthesis::utils::ParameterSmoother;

/// Stereo width processor (mid/side).
///
/// Encodes the stereo pair to mid (`(l + r) / 2`) and side (`(l - r) / 2`), scales the side
/// signal and decodes again:
/// - `width = 0.0` collapses to mono
/// - `width = 1.0` is a passthrough
/// - `width = 2.0` doubles the stereo spread
///
/// Because only the side signal is scaled, the mono sum (`mid`) is untouched, so the result
/// stays mono-compatible at any width.
#[derive(Debug, Clone, Copy)]
pub struct StereoWidth {
    width: ParameterSmoother,
}

impl Default for StereoWidth {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl StereoWidth {
    /// Create a new stereo width processor. Starts at unity width.
    pub fn new(sample_rate: f32) -> Self {
        Self {
            width: ParameterSmoother::new(sample_rate, 10.0, 1.0),
        }
    }

    /// Set the stereo width: 0.0 = mono, 1.0 = unchanged, 2.0 = extra wide (smoothed).
    pub fn set_width(&mut self, width: f32) {
        self.width.set_target(width.clamp(0.0, 2.0));
    }

    /// Current width without advancing the smoother.
    pub fn width(&self) -> f32 {
        self.width.current_value()
    }

    /// Process one stereo sample pair.
    #[inline(always)]
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        let width = self.width.process_sample();

        let mid = (left + right) * 0.5;
        let side = (left - right) * 0.5 * width;

        (mid + side, mid - side)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: impl Iterator<Item = f32>) -> f32 {
        let collected: Vec<f32> = samples.collect();
        (collected.iter().map(|s| s * s).sum::<f32>() / collected.len() as f32).sqrt()
    }

    #[test]
    fn unity_width_passes_through() {
        let mut width = StereoWidth::new(48_000.0);

        let mut phase = 0.0f32;
        for _ in 0..4096 {
            phase = (phase + 220.0 / 48_000.0).fract();
            let l = (std::f32::consts::TAU * phase).sin();
            let r = (std::f32::consts::TAU * phase * 0.5).cos();

            let (out_l, out_r) = width.process_stereo(l, r);
            assert!((out_l - l).abs() < 1e-5, "left changed: {l} -> {out_l}");
            assert!((out_r - r).abs() < 1e-5, "right changed: {r} -> {out_r}");
        }
    }

    #[test]
    fn zero_width_collapses_to_mono() {
        let mut width = StereoWidth::new(48_000.0);
        width.set_width(0.0);

        let mut last = (0.0, 0.0);
        for i in 0..24_000 {
            last = width.process_stereo((i as f32 * 0.01).sin(), (i as f32 * 0.037).cos());
        }

        assert!(
            (last.0 - last.1).abs() < 1e-4,
            "should be mono: {last:?}"
        );
    }

    #[test]
    fn wide_setting_raises_the_side_signal_only() {
        let left = |i: usize| (i as f32 * 0.01).sin();
        let right = |i: usize| (i as f32 * 0.013).cos();

        let mut unity = StereoWidth::new(48_000.0);
        let mut wide = StereoWidth::new(48_000.0);
        wide.set_width(2.0);

        let count = 8192;
        let mut unity_side = Vec::with_capacity(count);
        let mut wide_side = Vec::with_capacity(count);
        let mut unity_mid = Vec::with_capacity(count);
        let mut wide_mid = Vec::with_capacity(count);

        for i in 0..count {
            let (l, r) = (left(i), right(i));

            let (ul, ur) = unity.process_stereo(l, r);
            unity_side.push((ul - ur) * 0.5);
            unity_mid.push((ul + ur) * 0.5);

            let (wl, wr) = wide.process_stereo(l, r);
            wide_side.push((wl - wr) * 0.5);
            wide_mid.push((wl + wr) * 0.5);
        }

        assert!(
            rms(wide_side.into_iter()) > rms(unity_side.into_iter()) * 1.5,
            "width 2 should widen the side signal"
        );
        assert!(
            (rms(wide_mid.into_iter()) - rms(unity_mid.into_iter())).abs() < 1e-4,
            "mid must be untouched by width"
        );
    }

    #[test]
    fn mono_input_stays_mono_at_any_width() {
        for setting in [0.0, 0.5, 1.0, 2.0] {
            let mut width = StereoWidth::new(48_000.0);
            width.set_width(setting);

            let mut last = (0.0, 0.0);
            for i in 0..4096 {
                let x = (i as f32 * 0.02).sin();
                last = width.process_stereo(x, x);
            }

            assert!(
                (last.0 - last.1).abs() < 1e-6,
                "mono broke at width {setting}: {last:?}"
            );
        }
    }
}
