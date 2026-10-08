#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseType {
    White,
    Pink,
    Brown,
    /// Smooth lattice noise: gentle, band-limited, no hiss.
    Perlin,
    /// Fractal (fBm) noise: `octaves` layers of Perlin noise at doubling frequency.
    Fractal,
}

/// A high-performance, real-time safe noise generator free of spectral anomalies
/// and dynamic DC offset accumulation.
#[derive(Debug, Clone, Copy)]
pub struct NoiseGenerator {
    rng_state: u32,

    // Fixed seed, used to hash the Perlin lattice so a given seed always gives the same field
    seed: u32,

    // Pink Noise (Voss-McCartney) State Memory
    pink_rows: [f32; 12],
    pink_running_sum: f32,
    pink_index: u32,

    // Brownian Noise State Memory
    brown_accumulator: f32,

    // Perlin / fractal state
    perlin_position: f32,
    perlin_increment: f32,
    octaves: u32,
}

impl NoiseGenerator {
    pub fn new(seed: u32) -> Self {
        let safe_seed = if seed == 0 { 0xACE1_B2A7 } else { seed };

        Self {
            rng_state: safe_seed,
            seed: safe_seed,
            pink_rows: [0.0; 12],
            pink_running_sum: 0.0,
            pink_index: 0,
            brown_accumulator: 0.0,
            perlin_position: 0.0,
            perlin_increment: 0.01,
            octaves: 4,
        }
    }

    /// How far the Perlin lattice advances per sample. The generator carries no sample rate, so
    /// this is per sample: `0.01` drifts slowly, `0.5` moves fast.
    pub fn set_perlin_increment(&mut self, increment: f32) {
        self.perlin_increment = increment.max(0.0);
    }

    /// Number of layers summed by [`NoiseType::Fractal`], clamped to 1..=8.
    pub fn set_octaves(&mut self, octaves: u32) {
        self.octaves = octaves.clamp(1, 8);
    }

    /// Deterministic value for one Perlin lattice point, in [-1, 1). Hashing the index (rather
    /// than drawing from the RNG stream) keeps the field stable and reproducible per seed.
    #[inline(always)]
    fn lattice_value(&self, index: i32) -> f32 {
        let mut hash = (index as u32).wrapping_mul(0x9E37_79B1) ^ self.seed;
        hash ^= hash >> 15;
        hash = hash.wrapping_mul(0x85EB_CA6B);
        hash ^= hash >> 13;

        ((hash >> 9) as f32 / (1u32 << 23) as f32) * 2.0 - 1.0
    }

    /// Smooth lattice (value) noise at an arbitrary position.
    #[inline(always)]
    fn perlin_at(&self, position: f32) -> f32 {
        let base = position.floor();
        let fraction = position - base;

        let left = self.lattice_value(base as i32);
        let right = self.lattice_value(base as i32 + 1);

        // Smoothstep between the two lattice points.
        let smooth = fraction * fraction * (3.0 - 2.0 * fraction);
        left + (right - left) * smooth
    }

    /// Fast, branchless 32-bit Xorshift Pseudo-Random Number Generator.
    /// Safely isolates internal state generation from global OS allocator locks.
    #[inline(always)]
    fn next_random_u32(&mut self) -> u32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x;
        x
    }

    /// Generates a float uniformly distributed between [-1.0, 1.0) using high-entropy IEEE 754 type-punning.
    /// Completely division-free, perfectly zero-centered, and optimized for single-cycle hardware FMS execution.
    #[inline(always)]
    fn next_white_sample(&mut self) -> f32 {
        let rand_int = self.next_random_u32();

        // 1. Shift right by 9 to leverage the highest-entropy upper bits for the mantissa
        let mantissa = rand_int >> 9;

        // 2. Inject bits into the [1.0, 2.0) exponent window layout (0x3F80_0000)
        let ieee_fraction = 0x3F80_0000 | mantissa;

        // 3. Type-pun raw bit layout directly into an f32 register
        let float_val = f32::from_bits(ieee_fraction);

        // 4. Stable FMS Mapping: Preserves sub-slice quantization density and forces an exact
        //    zero-centered midpoint while allowing LLVM to fold the tree into a single native FMS instruction.
        (float_val - 1.5) * 2.0
    }

    /// Renders the next audio sample matching the target noise category. 100% Real-time safe.
    #[inline(always)]
    pub fn next_sample(&mut self, noise_type: NoiseType) -> f32 {
        match noise_type {
            NoiseType::White => self.next_white_sample(),

            NoiseType::Pink => {
                // Advance the index fully across the integer boundary to keep
                // the trailing_zeros() execution sequence uncorrupted.
                self.pink_index = self.pink_index.wrapping_add(1);

                let trailing_zeros = self.pink_index.trailing_zeros() as usize;

                if trailing_zeros < self.pink_rows.len() {
                    let new_white = self.next_white_sample();

                    // Substitute row values cleanly inside the running accumulator
                    self.pink_running_sum -= self.pink_rows[trailing_zeros];
                    self.pink_rows[trailing_zeros] = new_white;
                    self.pink_running_sum += new_white;
                }

                let white_floor = self.next_white_sample();
                let raw_pink =
                    (self.pink_running_sum + white_floor) / (self.pink_rows.len() as f32 + 1.0);

                // Scale factor to normalize output amplitude cleanly without peak clipping
                raw_pink * 2.5
            }

            NoiseType::Brown => {
                let white = self.next_white_sample();

                // Integrate the step with a continuous 0.995 leak factor to prevent DC drift
                self.brown_accumulator = (0.995 * self.brown_accumulator) + (white * 0.08);
                self.brown_accumulator = self.brown_accumulator.clamp(-1.0, 1.0);

                self.brown_accumulator * 2.0
            }

            NoiseType::Perlin => {
                self.perlin_position += self.perlin_increment;
                self.perlin_at(self.perlin_position)
            }

            NoiseType::Fractal => {
                self.perlin_position += self.perlin_increment;

                // Sum layers at doubling frequency and halving amplitude (fBm).
                let mut sum = 0.0;
                let mut amplitude = 1.0;
                let mut frequency = 1.0;
                let mut normalisation = 0.0;

                for _ in 0..self.octaves {
                    sum += amplitude * self.perlin_at(self.perlin_position * frequency);
                    normalisation += amplitude;
                    amplitude *= 0.5;
                    frequency *= 2.0;
                }

                sum / normalisation.max(1e-6)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(generator: &mut NoiseGenerator, noise_type: NoiseType, samples: usize) -> Vec<f32> {
        (0..samples).map(|_| generator.next_sample(noise_type)).collect()
    }

    #[test]
    fn white_noise_stays_in_range() {
        let mut generator = NoiseGenerator::new(1);
        for sample in run(&mut generator, NoiseType::White, 100_000) {
            assert!(sample.is_finite() && (-1.0..1.0).contains(&sample), "white {sample}");
        }
    }

    #[test]
    fn perlin_stays_in_range() {
        let mut generator = NoiseGenerator::new(7);
        generator.set_perlin_increment(0.05);

        for sample in run(&mut generator, NoiseType::Perlin, 100_000) {
            assert!(sample.is_finite(), "perlin non-finite");
            assert!((-1.0..=1.0).contains(&sample), "perlin {sample}");
        }
    }

    #[test]
    fn fractal_stays_in_range() {
        let mut generator = NoiseGenerator::new(3);
        generator.set_perlin_increment(0.02);
        generator.set_octaves(6);

        for sample in run(&mut generator, NoiseType::Fractal, 100_000) {
            assert!(sample.is_finite(), "fractal non-finite");
            assert!((-1.0..=1.0).contains(&sample), "fractal {sample}");
        }
    }

    #[test]
    fn noise_is_reproducible_for_a_seed() {
        for noise_type in [NoiseType::White, NoiseType::Perlin, NoiseType::Fractal] {
            let mut a = NoiseGenerator::new(42);
            let mut b = NoiseGenerator::new(42);
            assert_eq!(run(&mut a, noise_type, 5_000), run(&mut b, noise_type, 5_000));
        }
    }

    #[test]
    fn different_seeds_give_different_perlin_fields() {
        let mut a = NoiseGenerator::new(11);
        let mut b = NoiseGenerator::new(12);
        assert_ne!(run(&mut a, NoiseType::Perlin, 5_000), run(&mut b, NoiseType::Perlin, 5_000));
    }

    /// One octave of fractal noise IS plain Perlin noise.
    #[test]
    fn fractal_with_one_octave_equals_perlin() {
        let mut fractal = NoiseGenerator::new(21);
        fractal.set_perlin_increment(0.03);
        fractal.set_octaves(1);

        let mut perlin = NoiseGenerator::new(21);
        perlin.set_perlin_increment(0.03);

        assert_eq!(
            run(&mut fractal, NoiseType::Fractal, 5_000),
            run(&mut perlin, NoiseType::Perlin, 5_000)
        );
    }

    #[test]
    fn more_octaves_means_more_detail() {
        // Count direction changes. Sample-to-sample movement saturates (it can never exceed the
        // layer's amplitude), but the number of wiggles keeps climbing with the octave count.
        let extrema = |octaves: u32| {
            let mut generator = NoiseGenerator::new(5);
            generator.set_perlin_increment(0.05);
            generator.set_octaves(octaves);

            let values = run(&mut generator, NoiseType::Fractal, 20_000);
            values
                .windows(3)
                .filter(|w| (w[1] - w[0]).signum() != (w[2] - w[1]).signum())
                .count()
        };

        let one = extrema(1);
        let six = extrema(6);

        assert!(
            six > one * 3 / 2,
            "more octaves should add detail: {one} -> {six} turns"
        );
    }
}
