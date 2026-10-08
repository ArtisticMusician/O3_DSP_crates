/// Random number utilities using rand 0.8 API.
///
/// rand 0.8 uses:
/// - `rand::thread_rng()` returns a ThreadRng (not generic)
/// - `rng.gen_range(min..max)` for range generation
/// - `rng.gen::<T>()` for typed generation
/// - Need to import `rand::Rng` trait

use rand::{Rng, thread_rng};

/// Generate a random f32 in range [0, 1)
#[inline]
pub fn random_f32() -> f32 {
    thread_rng().gen::<f32>()
}

/// Generate a random f32 in range [min, max)
#[inline]
pub fn random_f32_range(min: f32, max: f32) -> f32 {
    thread_rng().gen_range(min..max)
}

/// Generate a random i32
#[inline]
pub fn random_i32() -> i32 {
    thread_rng().gen::<i32>()
}

/// Generate a random i32 in range [0, n)
#[inline]
pub fn random_range(n: u32) -> u32 {
    thread_rng().gen_range(0..n)
}

/// Generate a random sign: -1.0 or 1.0
#[inline]
pub fn random_sign() -> f32 {
    if thread_rng().gen::<bool>() {
        1.0
    } else {
        -1.0
    }
}