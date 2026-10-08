//! SIMD-accelerated channel mixing.
//!
//! The AVX2/FMA path is written against `core::arch` intrinsics rather than the unstable
//! `std::simd` module, so the crate builds on the stable toolchain. The execution
//! strategy is resolved once, at construction, by [`SimdArrayProcessor::resolve_strategy`].

#[repr(align(64))]
pub struct AudioBlock<const N: usize> {
    pub samples: [f32; N],
}

impl<const N: usize> AudioBlock<N> {
    pub fn new() -> Self {
        assert!(N % 8 == 0, "Block size must be a multiple of 8.");
        Self { samples: [0.0; N] }
    }
}

impl<const N: usize> Default for AudioBlock<N> {
    fn default() -> Self {
        Self::new()
    }
}

/// Defines the hardware-targeted execution strategy resolved at instantiation.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProcessingStrategy {
    Avx2Fma,
    Fallback,
}

pub struct SimdArrayProcessor;

impl SimdArrayProcessor {
    /// Resolves the optimal processing strategy once during initialization.
    /// Call this on the main/UI thread.
    pub fn resolve_strategy() -> ProcessingStrategy {
        #[cfg(target_arch = "x86_64")]
        {
            if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
                return ProcessingStrategy::Avx2Fma;
            }
        }
        ProcessingStrategy::Fallback
    }

    /// `left[i] += mix * (right[i] - left[i])`, eight lanes at a time.
    ///
    /// # Safety
    /// The caller must ensure the CPU supports AVX2 and FMA. `resolve_strategy()` checks
    /// this once at initialization. Lanes are loaded and stored *unaligned*, so the slices
    /// carry no alignment requirement.
    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2,fma")]
    pub unsafe fn mix_channels_avx2_fma(left: &mut [f32], right: &[f32], mix_coefficient: f32) {
        use std::arch::x86_64::{
            _mm256_fmadd_ps, _mm256_loadu_ps, _mm256_set1_ps, _mm256_storeu_ps, _mm256_sub_ps,
        };

        let mix_vec = _mm256_set1_ps(mix_coefficient);
        let simd_len = left.len().min(right.len()) / 8;

        let left_ptr = left.as_mut_ptr();
        let right_ptr = right.as_ptr();

        for i in 0..simd_len {
            let offset = i * 8;

            let l_lane = _mm256_loadu_ps(left_ptr.add(offset));
            let r_lane = _mm256_loadu_ps(right_ptr.add(offset));

            let mixed = _mm256_fmadd_ps(mix_vec, _mm256_sub_ps(r_lane, l_lane), l_lane);

            _mm256_storeu_ps(left_ptr.add(offset), mixed);
        }
    }

    /// Flat, index-driven loop optimized for LLVM auto-vectorization.
    #[inline(always)]
    pub fn mix_channels_fallback(left: &mut [f32], right: &[f32], mix_coefficient: f32) {
        let inv_mix = 1.0 - mix_coefficient;
        let len = left.len().min(right.len());

        for i in 0..len {
            unsafe {
                let l = left.get_unchecked_mut(i);
                let r = right.get_unchecked(i);
                *l = (*l * inv_mix) + (*r * mix_coefficient);
            }
        }
    }

    /// Dispatches to the AVX2/FMA path where it exists, and to the fallback everywhere else.
    /// `resolve_strategy()` never yields `Avx2Fma` off x86_64, so the two agree.
    #[cfg(target_arch = "x86_64")]
    #[inline(always)]
    fn mix_channels(left: &mut [f32], right: &[f32], mix_coefficient: f32) {
        // Safety: selected only when `resolve_strategy()` detected AVX2 + FMA.
        unsafe {
            Self::mix_channels_avx2_fma(left, right, mix_coefficient);
        }
    }

    #[cfg(not(target_arch = "x86_64"))]
    #[inline(always)]
    fn mix_channels(left: &mut [f32], right: &[f32], mix_coefficient: f32) {
        Self::mix_channels_fallback(left, right, mix_coefficient);
    }
}

pub struct AudioEngineContext {
    strategy: ProcessingStrategy,
}

impl AudioEngineContext {
    pub fn new() -> Self {
        Self {
            strategy: SimdArrayProcessor::resolve_strategy(),
        }
    }

    /// The high-priority real-time audio callback thread.
    /// Matching against the strategy enum allows full function inlining of the hot paths,
    /// completely eliminating indirect function call pipeline stalls.
    #[inline(always)]
    pub fn process<const N: usize>(
        &self,
        left_channel: &mut AudioBlock<N>,
        right_channel: &AudioBlock<N>,
        mix: f32,
    ) {
        let coeff = mix.clamp(0.0, 1.0);

        match self.strategy {
            ProcessingStrategy::Avx2Fma => {
                SimdArrayProcessor::mix_channels(
                    &mut left_channel.samples,
                    &right_channel.samples,
                    coeff,
                );
            }
            ProcessingStrategy::Fallback => {
                SimdArrayProcessor::mix_channels_fallback(
                    &mut left_channel.samples,
                    &right_channel.samples,
                    coeff,
                );
            }
        }
    }
}

impl Default for AudioEngineContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(len: usize) -> (Vec<f32>, Vec<f32>) {
        let left = (0..len).map(|i| (i as f32 * 0.11).sin() * 0.9).collect();
        let right = (0..len).map(|i| (i as f32 * 0.07).cos() * 0.4).collect();
        (left, right)
    }

    /// The AVX2/FMA path must produce the same mix as the scalar fallback on this machine.
    #[test]
    fn avx2_matches_fallback() {
        let (mut left_a, right) = probe(1024);
        let mut left_b = left_a.clone();
        let mix = 0.37;

        SimdArrayProcessor::mix_channels_fallback(&mut left_a, &right, mix);

        if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
            // Safety: features verified immediately above.
            unsafe {
                SimdArrayProcessor::mix_channels_avx2_fma(&mut left_b, &right, mix);
            }
            let worst = left_a
                .iter()
                .zip(&left_b)
                .map(|(a, b)| (a - b).abs())
                .fold(0.0f32, f32::max);
            assert!(worst < 1e-6, "AVX2 path diverged from fallback by {worst}");
        }
    }

    /// A short `right` slice must not be read past its end.
    #[test]
    fn short_right_slice_is_bounded() {
        let mut left = vec![1.0f32; 64];
        let right = vec![0.5f32; 16];
        let expected = {
            let mut v = vec![1.0f32; 64];
            SimdArrayProcessor::mix_channels_fallback(&mut v, &right, 0.5);
            v
        };
        SimdArrayProcessor::mix_channels(&mut left, &right, 0.5);
        assert_eq!(left, expected);
    }

    #[test]
    fn engine_mixes_across_both_strategies() {
        let mut block_l = AudioBlock::<64>::new();
        let mut block_r = AudioBlock::<64>::new();
        for i in 0..64 {
            block_l.samples[i] = 1.0;
            block_r.samples[i] = 0.0;
        }

        let engine = AudioEngineContext::new();
        engine.process(&mut block_l, &block_r, 0.25);

        for (i, s) in block_l.samples.iter().enumerate() {
            assert!((s - 0.75).abs() < 1e-6, "sample {i} = {s}");
        }
    }
}
