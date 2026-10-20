//! Offline audio editing - the buffer operations between loading a file and feeding a sampler.
//!
//! Everything here works on whole [`AudioBuffer`]s, not per-sample DSP. The split is deliberate:
//! these run when audio is being prepared, so they are allowed to allocate and are not
//! real-time-safe. Sample-rate conversion is not duplicated here - [`crate::synthesis::utils::HermiteResampler`]
//! already owns that.

use super::AudioBuffer;

/// Shape of a fade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FadeCurve {
    /// Straight line. Two of these sum to a constant 1.0, so levels dip in a crossfade.
    Linear,
    /// Sine/cosine quarter-wave. Power sums to a constant, which is what you want when
    /// crossfading uncorrelated material.
    EqualPower,
}

impl FadeCurve {
    /// Rising gain at position `t` in 0..=1.
    fn fade_in_gain(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => t,
            Self::EqualPower => (t * std::f32::consts::FRAC_PI_2).sin(),
        }
    }

    /// Falling gain at position `t` in 0..=1, the mirror of [`Self::fade_in_gain`].
    fn fade_out_gain(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Self::Linear => 1.0 - t,
            Self::EqualPower => (t * std::f32::consts::FRAC_PI_2).cos(),
        }
    }
}

/// Normalised position of `index` within `length` frames: 0.0 at the first frame and 1.0 at the
/// last, so a fade lands exactly on its endpoint rather than one step short.
fn position(index: usize, length: usize) -> f32 {
    if length <= 1 {
        1.0
    } else {
        index as f32 / (length - 1) as f32
    }
}

/// Keep frames `start..end`. Out-of-range values are clamped, and a reversed range gives an
/// empty buffer rather than a panic.
pub fn trim(buffer: &AudioBuffer, start: usize, end: usize) -> AudioBuffer {
    let frames = buffer.frames();
    let start = start.min(frames);
    let end = end.clamp(start, frames);

    let channels = buffer
        .channels()
        .iter()
        .map(|channel| channel[start..end].to_vec())
        .collect();

    AudioBuffer::from_channels(buffer.sample_rate(), channels)
}

/// Remove frames `start..end`, joining what is left.
pub fn cut(buffer: &AudioBuffer, start: usize, end: usize) -> AudioBuffer {
    let frames = buffer.frames();
    let start = start.min(frames);
    let end = end.clamp(start, frames);

    let channels = buffer
        .channels()
        .iter()
        .map(|channel| {
            let mut data = channel[..start].to_vec();
            data.extend_from_slice(&channel[end..]);
            data
        })
        .collect();

    AudioBuffer::from_channels(buffer.sample_rate(), channels)
}

/// Insert `insert` at `frame`.
pub fn insert(buffer: &AudioBuffer, frame: usize, insert: &AudioBuffer) -> Option<AudioBuffer> {
    if buffer.channel_count() != insert.channel_count() || buffer.sample_rate() != insert.sample_rate() {
        return None;
    }

    let frame = frame.min(buffer.frames());
    let channels = buffer
        .channels()
        .iter()
        .zip(insert.channels())
        .map(|(target, source)| {
            let mut data = target[..frame].to_vec();
            data.extend_from_slice(source);
            data.extend_from_slice(&target[frame..]);
            data
        })
        .collect();

    Some(AudioBuffer::from_channels(buffer.sample_rate(), channels))
}

/// Join two buffers end to end. Requires matching channel count and sample rate.
pub fn concatenate(first: &AudioBuffer, second: &AudioBuffer) -> Option<AudioBuffer> {
    if first.channel_count() != second.channel_count()
        || first.sample_rate() != second.sample_rate()
    {
        return None;
    }

    let channels = first
        .channels()
        .iter()
        .zip(second.channels())
        .map(|(a, b)| {
            let mut data = a.clone();
            data.extend_from_slice(b);
            data
        })
        .collect();

    Some(AudioBuffer::from_channels(first.sample_rate(), channels))
}

/// Sum two buffers with independent gains. The result is as long as the longer input; the shorter
/// one is treated as silence past its end.
pub fn mix(a: &AudioBuffer, b: &AudioBuffer, gain_a: f32, gain_b: f32) -> Option<AudioBuffer> {
    if a.channel_count() != b.channel_count() || a.sample_rate() != b.sample_rate() {
        return None;
    }

    let frames = a.frames().max(b.frames());
    let mut channels = Vec::with_capacity(a.channel_count());

    for channel in 0..a.channel_count() {
        let left = a.channel(channel).unwrap_or(&[]);
        let right = b.channel(channel).unwrap_or(&[]);
        let data: Vec<f32> = (0..frames)
            .map(|frame| {
                let l = left.get(frame).copied().unwrap_or(0.0) * gain_a;
                let r = right.get(frame).copied().unwrap_or(0.0) * gain_b;
                l + r
            })
            .collect();
        channels.push(data);
    }

    Some(AudioBuffer::from_channels(a.sample_rate(), channels))
}

/// Multiply everything by a linear gain.
pub fn apply_gain(buffer: &mut AudioBuffer, gain: f32) {
    for channel in 0..buffer.channel_count() {
        if let Some(data) = buffer.channel_mut(channel) {
            for sample in data.iter_mut() {
                *sample *= gain;
            }
        }
    }
}

/// Multiply everything by a gain in dB.
pub fn apply_gain_db(buffer: &mut AudioBuffer, db: f32) {
    apply_gain(buffer, 10.0f32.powf(db / 20.0));
}

/// Scale the buffer so its loudest sample sits at `target_peak`. Returns the linear gain applied.
///
/// Silence is left alone and reports a gain of 1.0 - asking for a normalised silent buffer is not
/// an error, it just cannot be done.
pub fn normalize(buffer: &mut AudioBuffer, target_peak: f32) -> f32 {
    let peak = buffer.peak();
    if peak <= f32::MIN_POSITIVE {
        return 1.0;
    }

    let gain = target_peak / peak;
    apply_gain(buffer, gain);
    gain
}

/// Reverse playback order, per channel.
pub fn reverse(buffer: &mut AudioBuffer) {
    for channel in 0..buffer.channel_count() {
        if let Some(data) = buffer.channel_mut(channel) {
            data.reverse();
        }
    }
}

/// Ramp up over the first `frames` frames.
pub fn fade_in(buffer: &mut AudioBuffer, frames: usize, curve: FadeCurve) {
    let total = buffer.frames();
    let length = frames.min(total);
    if length == 0 {
        return;
    }

    for channel in 0..buffer.channel_count() {
        if let Some(data) = buffer.channel_mut(channel) {
            for (index, sample) in data.iter_mut().take(length).enumerate() {
                *sample *= curve.fade_in_gain(position(index, length));
            }
        }
    }
}

/// Ramp down over the last `frames` frames.
pub fn fade_out(buffer: &mut AudioBuffer, frames: usize, curve: FadeCurve) {
    let total = buffer.frames();
    let length = frames.min(total);
    if length == 0 {
        return;
    }

    let start = total - length;
    for channel in 0..buffer.channel_count() {
        if let Some(data) = buffer.channel_mut(channel) {
            for index in 0..length {
                data[start + index] *= curve.fade_out_gain(position(index, length));
            }
        }
    }
}

/// Overlap `frames` of the end of `a` with the start of `b`.
///
/// The result is `a` and `b` joined with that overlap removed, so an equal-power crossfade holds
/// its level through the join instead of dipping.
pub fn crossfade(
    a: &AudioBuffer,
    b: &AudioBuffer,
    frames: usize,
    curve: FadeCurve,
) -> Option<AudioBuffer> {
    if a.channel_count() != b.channel_count() || a.sample_rate() != b.sample_rate() {
        return None;
    }

    let overlap = frames.min(a.frames()).min(b.frames());
    let mut channels = Vec::with_capacity(a.channel_count());

    for channel in 0..a.channel_count() {
        let left = a.channel(channel).unwrap_or(&[]);
        let right = b.channel(channel).unwrap_or(&[]);

        let mut data = left[..left.len() - overlap].to_vec();

        for index in 0..overlap {
            let t = position(index, overlap);
            let from_left = left[left.len() - overlap + index] * curve.fade_out_gain(t);
            let from_right = right[index] * curve.fade_in_gain(t);
            data.push(from_left + from_right);
        }

        data.extend_from_slice(&right[overlap..]);
        channels.push(data);
    }

    Some(AudioBuffer::from_channels(a.sample_rate(), channels))
}

/// Subtract each channel's mean, removing a constant offset.
pub fn remove_dc_offset(buffer: &mut AudioBuffer) {
    for channel in 0..buffer.channel_count() {
        if let Some(data) = buffer.channel_mut(channel) {
            if data.is_empty() {
                continue;
            }
            let mean = data.iter().sum::<f32>() / data.len() as f32;
            for sample in data.iter_mut() {
                *sample -= mean;
            }
        }
    }
}

/// Find runs of frames where every channel stays below `threshold_db`, at least `min_frames` long.
///
/// Runs are returned as `(start, end)` frame indices, end exclusive. A run that touches the start
/// or the end of the buffer is reported too, which is how you find the useful region of a take.
pub fn silence_ranges(
    buffer: &AudioBuffer,
    threshold_db: f32,
    min_frames: usize,
) -> Vec<(usize, usize)> {
    let threshold = 10.0f32.powf(threshold_db / 20.0);
    let frames = buffer.frames();
    let mut ranges = Vec::new();
    let mut run_start: Option<usize> = None;

    for frame in 0..frames {
        let loudest = buffer
            .channels()
            .iter()
            .map(|channel| channel[frame].abs())
            .fold(0.0f32, f32::max);

        let silent = loudest <= threshold;
        match (silent, run_start) {
            (true, None) => run_start = Some(frame),
            (false, Some(start)) => {
                if frame - start >= min_frames.max(1) {
                    ranges.push((start, frame));
                }
                run_start = None;
            }
            _ => {}
        }
    }

    if let Some(start) = run_start {
        if frames - start >= min_frames.max(1) {
            ranges.push((start, frames));
        }
    }

    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ramp(sample_rate: f32, frames: usize) -> AudioBuffer {
        AudioBuffer::from_mono(sample_rate, (0..frames).map(|i| i as f32 / frames as f32).collect())
    }

    #[test]
    fn trim_keeps_the_requested_range() {
        let buffer = ramp(48_000.0, 100);
        let trimmed = trim(&buffer, 10, 20);

        assert_eq!(trimmed.frames(), 10);
        assert!((trimmed.sample(0, 0) - 0.1).abs() < 1e-6);
    }

    #[test]
    fn trim_clamps_rather_than_panicking() {
        let buffer = ramp(48_000.0, 100);

        assert_eq!(trim(&buffer, 90, 500).frames(), 10);
        assert_eq!(trim(&buffer, 500, 600).frames(), 0);
        assert_eq!(trim(&buffer, 60, 20).frames(), 0, "reversed range");
    }

    #[test]
    fn cut_removes_the_middle() {
        let buffer = AudioBuffer::from_mono(48_000.0, vec![0.0, 1.0, 2.0, 3.0, 4.0]);
        let cut_buffer = cut(&buffer, 1, 3);

        assert_eq!(cut_buffer.channel(0), Some(&[0.0, 3.0, 4.0][..]));
    }

    #[test]
    fn insert_splices_at_the_frame() {
        let base = AudioBuffer::from_mono(48_000.0, vec![1.0, 2.0, 3.0]);
        let middle = AudioBuffer::from_mono(48_000.0, vec![9.0]);

        let spliced = insert(&base, 2, &middle).unwrap();
        assert_eq!(spliced.channel(0), Some(&[1.0, 2.0, 9.0, 3.0][..]));

        let mismatched = AudioBuffer::new(48_000.0, 2, 1);
        assert!(insert(&base, 0, &mismatched).is_none());
    }

    #[test]
    fn concatenate_joins_in_order() {
        let a = AudioBuffer::from_mono(48_000.0, vec![1.0, 2.0]);
        let b = AudioBuffer::from_mono(48_000.0, vec![3.0]);

        let joined = concatenate(&a, &b).unwrap();
        assert_eq!(joined.channel(0), Some(&[1.0, 2.0, 3.0][..]));

        let other_rate = AudioBuffer::from_mono(44_100.0, vec![1.0]);
        assert!(concatenate(&a, &other_rate).is_none());
    }

    #[test]
    fn mix_applies_per_input_gain() {
        let a = AudioBuffer::from_mono(48_000.0, vec![1.0, 1.0]);
        let b = AudioBuffer::from_mono(48_000.0, vec![1.0, 0.5]);

        // frame 0: 0.5*1.0 + 2.0*1.0 = 2.5, frame 1: 0.5*1.0 + 2.0*0.5 = 1.5
        let mixed = mix(&a, &b, 0.5, 2.0).unwrap();
        assert_eq!(mixed.channel(0), Some(&[2.5, 1.5][..]));
    }

    #[test]
    fn gain_db_matches_the_linear_equivalent() {
        let mut buffer = AudioBuffer::from_mono(48_000.0, vec![1.0]);
        apply_gain_db(&mut buffer, -6.0206);

        assert!((buffer.sample(0, 0) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn normalize_sets_the_peak_and_reports_the_gain() {
        let mut buffer = AudioBuffer::from_mono(48_000.0, vec![0.25, -0.5]);
        let gain = normalize(&mut buffer, 1.0);

        assert!((gain - 2.0).abs() < 1e-6);
        assert!((buffer.peak() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalising_silence_is_a_no_op() {
        let mut buffer = AudioBuffer::from_mono(48_000.0, vec![0.0; 16]);
        let gain = normalize(&mut buffer, 1.0);

        assert_eq!(gain, 1.0);
        assert_eq!(buffer.peak(), 0.0);
    }

    #[test]
    fn reverse_flips_each_channel() {
        let mut buffer = AudioBuffer::from_mono(48_000.0, vec![1.0, 2.0, 3.0]);
        reverse(&mut buffer);

        assert_eq!(buffer.channel(0), Some(&[3.0, 2.0, 1.0][..]));
    }

    #[test]
    fn fades_reach_both_ends() {
        let mut buffer = AudioBuffer::from_mono(48_000.0, vec![1.0; 100]);
        fade_in(&mut buffer, 100, FadeCurve::Linear);
        fade_out(&mut buffer, 100, FadeCurve::Linear);

        assert!(buffer.sample(0, 0).abs() < 1e-6, "fade in must start at silence");
        assert!((buffer.sample(0, 99)).abs() < 1e-6, "fade out must end at silence");
    }

    #[test]
    fn equal_power_fade_is_louder_at_the_midpoint_than_linear() {
        let mut linear = AudioBuffer::from_mono(48_000.0, vec![1.0; 100]);
        let mut power = linear.clone();

        fade_in(&mut linear, 100, FadeCurve::Linear);
        fade_in(&mut power, 100, FadeCurve::EqualPower);

        assert!((linear.sample(0, 49) - 0.49).abs() < 0.03);
        assert!((power.sample(0, 49) - 0.6946).abs() < 0.03);
    }

    #[test]
    fn a_linear_crossfade_holds_the_level_of_identical_material() {
        // Linear fades sum to a constant 1.0, which is the right curve when the two sides are
        // the same material (a seamless loop, say).
        let a = AudioBuffer::from_mono(48_000.0, vec![1.0; 100]);
        let b = AudioBuffer::from_mono(48_000.0, vec![1.0; 100]);

        let joined = crossfade(&a, &b, 20, FadeCurve::Linear).unwrap();

        assert_eq!(joined.frames(), 180);
        let worst = (0..joined.frames())
            .map(|frame| (joined.sample(0, frame) - 1.0).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-5, "level moved by {worst} through the crossfade");
    }

    #[test]
    fn an_equal_power_crossfade_peaks_3db_on_correlated_material() {
        // Power sums to a constant, so two identical constants add to sqrt(2) in the middle.
        // That is the correct behaviour for equal power - it is why you reach for it on
        // uncorrelated material, and why the linear curve exists for correlated material.
        let a = AudioBuffer::from_mono(48_000.0, vec![1.0; 100]);
        let b = AudioBuffer::from_mono(48_000.0, vec![1.0; 100]);

        let joined = crossfade(&a, &b, 21, FadeCurve::EqualPower).unwrap();

        let middle = joined.sample(0, 90);
        assert!(
            (middle - std::f32::consts::SQRT_2).abs() < 0.01,
            "midpoint should be sqrt(2), got {middle}"
        );
        assert!((joined.sample(0, 0) - 1.0).abs() < 1e-5, "starts at programme level");
        assert!((joined.sample(0, joined.frames() - 1) - 1.0).abs() < 1e-5, "ends at programme level");
    }

    #[test]
    fn crossfade_shorter_than_the_overlap_is_clamped() {
        let a = AudioBuffer::from_mono(48_000.0, vec![1.0; 5]);
        let b = AudioBuffer::from_mono(48_000.0, vec![1.0; 5]);

        let joined = crossfade(&a, &b, 500, FadeCurve::Linear).unwrap();
        assert_eq!(joined.frames(), 5);
    }

    #[test]
    fn dc_offset_removal_centres_the_signal() {
        let mut buffer = AudioBuffer::from_mono(48_000.0, vec![0.5, 1.5, -0.5, 0.5]);
        remove_dc_offset(&mut buffer);

        let mean = buffer.channel(0).unwrap().iter().sum::<f32>() / 4.0;
        assert!(mean.abs() < 1e-6, "mean {mean}");
    }

    #[test]
    fn silence_ranges_finds_leading_trailing_and_gaps() {
        let mut samples = vec![0.0; 10]; // leading
        samples.extend_from_slice(&[0.5; 5]);
        samples.extend_from_slice(&[0.0; 20]); // gap
        samples.extend_from_slice(&[0.5; 5]);
        samples.extend_from_slice(&[0.0; 4]); // trailing

        let buffer = AudioBuffer::from_mono(48_000.0, samples);
        let ranges = silence_ranges(&buffer, -60.0, 2);

        assert_eq!(ranges, vec![(0, 10), (15, 35), (40, 44)]);
    }

    #[test]
    fn silence_ranges_respects_the_minimum_length() {
        let mut samples = vec![0.5; 5];
        samples.extend_from_slice(&[0.0; 1]);
        samples.extend_from_slice(&[0.5; 5]);

        let buffer = AudioBuffer::from_mono(48_000.0, samples);
        assert!(silence_ranges(&buffer, -60.0, 4).is_empty());
    }
}
