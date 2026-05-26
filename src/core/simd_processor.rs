#![feature(portable_simd)]
use std::simd::f32x8;

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
        #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
        {
            if std::is_x86_feature_detected!("avx2") && std::is_x86_feature_detected!("fma") {
                return ProcessingStrategy::Avx2Fma;
            }
        }
        ProcessingStrategy::Fallback
    }

    /// Aligned AVX2 + FMA loop. Marked inline to allow fusion with the enum match block.
    #[target_feature(enable = "avx2,fma")]
    #[inline]
    pub unsafe fn mix_channels_avx2_fma(left: &mut [f32], right: &[f32], mix_coefficient: f32) {
        let mix_vec = f32x8::splat(mix_coefficient);
        let simd_len = left.len() / 8;
        
        let left_ptr = left.as_mut_ptr() as *mut f32x8;
        let right_ptr = right.as_ptr() as *const f32x8;

        for i in 0..simd_len {
            let current_left = left_ptr.add(i);
            
            let mut l_lane = std::ptr::read(current_left);
            let r_lane = std::ptr::read(right_ptr.add(i));

            l_lane = mix_vec.fma(r_lane - l_lane, l_lane);

            std::ptr::write(current_left, l_lane);
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
    pub fn process<const N: usize>(&self, left_channel: &mut AudioBlock<N>, right_channel: &AudioBlock<N>, mix: f32) {
        let coeff = mix.clamp(0.0, 1.0);
        
        match self.strategy {
            ProcessingStrategy::Avx2Fma => {
                // Safety: Validated by resolve_strategy() at initialization
                unsafe {
                    SimdArrayProcessor::mix_channels_avx2_fma(
                        &mut left_channel.samples,
                        &right_channel.samples,
                        coeff,
                    );
                }
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