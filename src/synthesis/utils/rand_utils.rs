//! Random numbers, without a dependency.
//!
//! Audio needs a flat, fast, reproducible stream - not cryptographic quality - so this is Vigna's
//! xorshift64*: eight bytes of state, no allocation, and a period long enough that a year of noise
//! never repeats. Each thread gets its own state, so two threads cannot disturb each other's
//! sequence.

use std::cell::Cell;

/// A reproducible uniform generator. Same seed, same sequence, on every platform.
#[derive(Debug, Clone, Copy)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Seed the generator. Zero is a fixed point of the xor-shift, so it is replaced.
    pub const fn new(seed: u64) -> Self {
        Self {
            state: if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed },
        }
    }

    /// The raw next value.
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in `[0, 1)`, using the top 24 bits so every value is exactly representable.
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        ((self.next_u64() >> 40) as f32) * (1.0 / 16_777_216.0)
    }

    /// Uniform in `[min, max)`.
    #[inline]
    pub fn next_f32_range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }

    /// Uniform in `[0, n)`; `n == 0` gives 0 rather than a division by zero.
    #[inline]
    pub fn next_below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as u32
        }
    }

    /// A coin flip.
    #[inline]
    pub fn next_bool(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }

    /// A random sign: `-1.0` or `1.0`.
    #[inline]
    pub fn next_sign(&mut self) -> f32 {
        if self.next_bool() {
            1.0
        } else {
            -1.0
        }
    }
}

thread_local! {
    /// One generator per thread, started from a fixed seed so a run is reproducible.
    static THREAD_RNG: Cell<Rng> = const { Cell::new(Rng::new(0x5DEE_CE66_D1CE_B00F)) };
}

#[inline]
fn with_rng<T>(body: impl FnOnce(&mut Rng) -> T) -> T {
    THREAD_RNG.with(|cell| {
        let mut rng = cell.get();
        let value = body(&mut rng);
        cell.set(rng);
        value
    })
}

/// Generate a random f32 in range [0, 1)
#[inline]
pub fn random_f32() -> f32 {
    with_rng(|rng| rng.next_f32())
}

/// Generate a random f32 in range [min, max)
#[inline]
pub fn random_f32_range(min: f32, max: f32) -> f32 {
    with_rng(|rng| rng.next_f32_range(min, max))
}

/// Generate a random i32
#[inline]
pub fn random_i32() -> i32 {
    with_rng(|rng| rng.next_u64() as i32)
}

/// Generate a random i32 in range [0, n)
#[inline]
pub fn random_range(n: u32) -> u32 {
    with_rng(|rng| rng.next_below(n))
}

/// Generate a random sign: -1.0 or 1.0
#[inline]
pub fn random_sign() -> f32 {
    with_rng(|rng| rng.next_sign())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_reproduces_the_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);

        for _ in 0..1_000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = Rng::new(1);
        let mut b = Rng::new(2);

        let differing = (0..100).filter(|_| a.next_u64() != b.next_u64()).count();
        assert!(differing > 95, "only {differing} of 100 values differed");
    }

    #[test]
    fn the_zero_seed_is_not_a_fixed_point() {
        let mut rng = Rng::new(0);
        assert_ne!(rng.next_u64(), rng.next_u64());
    }

    #[test]
    fn floats_stay_inside_the_unit_interval() {
        let mut rng = Rng::new(7);

        let mut sum = 0.0f64;
        for _ in 0..100_000 {
            let value = rng.next_f32();
            assert!((0.0..1.0).contains(&value), "out of range: {value}");
            sum += value as f64;
        }

        let mean = sum / 100_000.0;
        assert!((mean - 0.5).abs() < 0.01, "mean {mean} is not flat");
    }

    #[test]
    fn ranges_are_respected() {
        let mut rng = Rng::new(9);

        for _ in 0..10_000 {
            let value = rng.next_f32_range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&value), "{value}");

            let index = rng.next_below(7);
            assert!(index < 7, "{index}");
        }

        assert_eq!(rng.next_below(0), 0, "an empty range must not divide by zero");
    }

    #[test]
    fn signs_are_both_negative_and_positive() {
        let mut rng = Rng::new(11);
        let mut positive = 0;

        for _ in 0..1_000 {
            if rng.next_sign() > 0.0 {
                positive += 1;
            }
        }

        assert!(positive > 400 && positive < 600, "{positive} of 1000 positive");
    }
}
