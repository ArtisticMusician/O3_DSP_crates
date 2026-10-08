use crate::synthesis::utils::Bezier;

/// Random / probabilistic envelope.
///
/// A free-running envelope that re-rolls its own shape every segment: each segment picks a random
/// length and a random destination level from the configured ranges, then glides there using the
/// Bezier curve set by [`Self::set_shape`]. It never stops, so it works as a modulation source the
/// way an LFO does, but with a different contour every time.
///
/// The generator carries its own xorshift state, so a given seed always produces the same
/// sequence - reproducible without touching the global RNG.
#[derive(Debug, Clone, Copy)]
pub struct RandomEnvelope {
    sample_rate: f32,

    seed: u32,
    rng_state: u32,

    // Segment in flight
    from_value: f32,
    to_value: f32,
    value: f32,
    segment_length: u32,
    segment_position: u32,

    // Ranges
    min_level: f32,
    max_level: f32,
    min_segment_ms: f32,
    max_segment_ms: f32,

    // Curve applied across each segment
    shape: f32,
}

impl Default for RandomEnvelope {
    fn default() -> Self {
        Self::new(48000.0)
    }
}

impl RandomEnvelope {
    /// Create a new random envelope.
    pub fn new(sample_rate: f32) -> Self {
        let mut envelope = Self {
            sample_rate,
            seed: 0x51ED_2701,
            rng_state: 0x51ED_2701,
            from_value: 0.0,
            to_value: 0.0,
            value: 0.0,
            segment_length: 1,
            segment_position: 0,
            min_level: 0.0,
            max_level: 1.0,
            min_segment_ms: 50.0,
            max_segment_ms: 500.0,
            shape: 0.0,
        };

        envelope.restart();
        envelope
    }

    /// Use a fixed seed. Same seed, same sequence - reproducible.
    pub fn set_seed(&mut self, seed: u32) {
        self.seed = if seed == 0 { 0x9E37_79B9 } else { seed };
        self.rng_state = self.seed;
        self.restart();
    }

    /// Levels the envelope is allowed to visit. The segment in flight is pulled into the new
    /// range rather than being allowed to finish outside it.
    pub fn set_level_range(&mut self, min: f32, max: f32) {
        self.min_level = min.min(max);
        self.max_level = max.max(min);

        self.from_value = self.from_value.clamp(self.min_level, self.max_level);
        self.to_value = self.to_value.clamp(self.min_level, self.max_level);
        self.value = self.value.clamp(self.min_level, self.max_level);
    }

    /// Range for a segment's length, in milliseconds.
    pub fn set_segment_range_ms(&mut self, min_ms: f32, max_ms: f32) {
        self.min_segment_ms = min_ms.min(max_ms).max(1.0);
        self.max_segment_ms = max_ms.max(min_ms).max(1.0);
    }

    /// Curve applied across each segment:
    /// `-1.0` = logarithmic (fast start), `0.0` = linear, `1.0` = exponential (slow start).
    pub fn set_shape(&mut self, shape: f32) {
        self.shape = shape.clamp(-1.0, 1.0);
    }

    /// Current value without advancing.
    pub fn value(&self) -> f32 {
        self.value
    }

    /// Restart from a fresh segment, beginning somewhere inside the level range.
    pub fn restart(&mut self) {
        self.rng_state = self.seed;

        // Begin inside the range: starting at zero would leave the range on the first segment.
        self.value = self.min_level + (self.max_level - self.min_level) * self.next_unit();
        self.from_value = self.value;
        self.segment_position = 0;
        self.segment_length = 1;
        self.roll_next_segment();
    }

    #[inline(always)]
    fn next_random_u32(&mut self) -> u32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        x
    }

    /// Uniform in 0.0..1.0.
    #[inline(always)]
    fn next_unit(&mut self) -> f32 {
        (self.next_random_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Pick a fresh destination level and segment length, and start gliding towards it.
    fn roll_next_segment(&mut self) {
        self.from_value = self.value;

        let level = self.min_level + (self.max_level - self.min_level) * self.next_unit();
        self.to_value = level;

        let span_ms = self.max_segment_ms - self.min_segment_ms;
        let length_ms = self.min_segment_ms + span_ms * self.next_unit();
        self.segment_length = ((length_ms * 0.001 * self.sample_rate).round() as i64).max(1) as u32;
        self.segment_position = 0;
    }

    /// Process a single sample.
    #[inline(always)]
    pub fn process_sample(&mut self) -> f32 {
        if self.segment_position >= self.segment_length {
            self.roll_next_segment();
        }

        self.segment_position += 1;

        let progress = self.segment_position as f32 / self.segment_length as f32;
        let curved = Bezier::shaped(progress, self.shape);

        self.value = self.from_value + (self.to_value - self.from_value) * curved;
        self.value
    }

    /// Process a block of samples.
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(envelope: &mut RandomEnvelope, samples: usize) -> Vec<f32> {
        (0..samples).map(|_| envelope.process_sample()).collect()
    }

    #[test]
    fn same_seed_reproduces_the_sequence() {
        let mut a = RandomEnvelope::new(48_000.0);
        a.set_seed(1234);
        let mut b = RandomEnvelope::new(48_000.0);
        b.set_seed(1234);

        assert_eq!(run(&mut a, 20_000), run(&mut b, 20_000));
    }

    #[test]
    fn different_seeds_differ() {
        let mut a = RandomEnvelope::new(48_000.0);
        a.set_seed(1);
        let mut b = RandomEnvelope::new(48_000.0);
        b.set_seed(2);

        assert_ne!(run(&mut a, 20_000), run(&mut b, 20_000));
    }

    #[test]
    fn every_value_stays_inside_the_level_range() {
        let mut envelope = RandomEnvelope::new(48_000.0);
        envelope.set_seed(99);
        envelope.set_level_range(0.25, 0.75);
        envelope.set_segment_range_ms(5.0, 40.0);

        for value in run(&mut envelope, 100_000) {
            assert!(value.is_finite(), "non-finite value {value}");
            assert!(
                (0.25 - 1e-4..=0.75 + 1e-4).contains(&value),
                "value {value} left the configured range"
            );
        }
    }

    #[test]
    fn the_envelope_actually_moves() {
        let mut envelope = RandomEnvelope::new(48_000.0);
        envelope.set_seed(7);
        envelope.set_level_range(0.0, 1.0);
        envelope.set_segment_range_ms(20.0, 80.0);

        let values = run(&mut envelope, 24_000);
        let lowest = values.iter().copied().fold(f32::MAX, f32::min);
        let highest = values.iter().copied().fold(f32::MIN, f32::max);

        assert!(highest - lowest > 0.2, "envelope barely moved: {lowest}..{highest}");
    }

    #[test]
    fn segments_have_the_requested_length() {
        let mut envelope = RandomEnvelope::new(1_000.0);
        envelope.set_seed(5);
        envelope.set_segment_range_ms(10.0, 10.0); // fixed 10 ms = 10 samples at 1 kHz

        // A segment takes 10 samples, so the direction of travel must change on that boundary.
        // 2000 samples = 200 segments, and targets are random, so roughly half of them turn.
        let values = run(&mut envelope, 2000);
        let mut turning_points = 0;
        for window in values.windows(3) {
            let (a, b, c) = (window[0], window[1], window[2]);
            if (b - a).signum() != (c - b).signum() {
                turning_points += 1;
            }
        }

        assert!(
            turning_points >= 40,
            "expected a new segment every 10 samples, got {turning_points} turns in 200 segments"
        );
    }

    #[test]
    fn shape_bends_the_segment() {
        let mut slow = RandomEnvelope::new(48_000.0);
        slow.set_seed(42);
        slow.set_shape(1.0);
        slow.set_segment_range_ms(200.0, 200.0);
        slow.set_level_range(0.0, 1.0);

        let mut fast = RandomEnvelope::new(48_000.0);
        fast.set_seed(42);
        fast.set_shape(-1.0);
        fast.set_segment_range_ms(200.0, 200.0);
        fast.set_level_range(0.0, 1.0);

        // Same seed, same destinations; only the curve differs, so the area under the curve differs.
        let slow_area: f32 = run(&mut slow, 4_800).iter().sum();
        let fast_area: f32 = run(&mut fast, 4_800).iter().sum();

        assert!(
            (slow_area - fast_area).abs() > 1.0,
            "shape had no effect: {slow_area} vs {fast_area}"
        );
    }
}
