/// Default partial count for a Spectra-style generator.
/// Index 0 is the fundamental (harmonic 1).
pub const MAX_PARTIALS: usize = 64;

/// Harmonic amplitude vector. Morph, multi, and single modulators write here.
#[derive(Debug, Clone, Copy)]
pub struct HarmonicSpectrum<const N: usize = MAX_PARTIALS> {
    amps: [f32; N],
}

impl<const N: usize> Default for HarmonicSpectrum<N> {
    fn default() -> Self {
        Self { amps: [0.0; N] }
    }
}

impl<const N: usize> HarmonicSpectrum<N> {
    pub fn silence() -> Self {
        Self::default()
    }

    pub fn from_amps(amps: [f32; N]) -> Self {
        Self { amps }
    }

    /// Fundamental only.
    pub fn sine() -> Self {
        let mut this = Self::silence();
        if N > 0 {
            this.amps[0] = 1.0;
        }
        this
    }

    /// Classic falling 1/k saw.
    pub fn saw() -> Self {
        let mut this = Self::silence();
        for k in 0..N {
            this.amps[k] = 1.0 / (k as f32 + 1.0);
        }
        this.normalize_l1(0.7);
        this
    }

    /// Odd harmonics only, 1/k.
    pub fn square() -> Self {
        let mut this = Self::silence();
        for k in 0..N {
            if k % 2 == 0 {
                this.amps[k] = 1.0 / (k as f32 + 1.0);
            }
        }
        this.normalize_l1(0.7);
        this
    }

    /// Odd harmonics, 1/k².
    pub fn triangle() -> Self {
        let mut this = Self::silence();
        for k in 0..N {
            if k % 2 == 0 {
                let n = k as f32 + 1.0;
                let sign = if (k / 2) % 2 == 0 { 1.0 } else { -1.0 };
                this.amps[k] = sign / (n * n);
            }
        }
        this.normalize_l1(0.8);
        this
    }

    #[inline(always)]
    pub fn amps(&self) -> &[f32; N] {
        &self.amps
    }

    #[inline(always)]
    pub fn amps_mut(&mut self) -> &mut [f32; N] {
        &mut self.amps
    }

    #[inline(always)]
    pub fn set_partial(&mut self, index: usize, amp: f32) {
        if let Some(slot) = self.amps.get_mut(index) {
            *slot = amp.clamp(-1.0, 1.0);
        }
    }

    #[inline(always)]
    pub fn partial(&self, index: usize) -> f32 {
        self.amps.get(index).copied().unwrap_or(0.0)
    }

    pub fn len(&self) -> usize {
        N
    }

    /// Scale so sum(|a|) == target. No-op on silence.
    pub fn normalize_l1(&mut self, target: f32) {
        let sum: f32 = self.amps.iter().map(|a| a.abs()).sum();
        if sum > 1e-12 {
            let g = target / sum;
            for a in self.amps.iter_mut() {
                *a *= g;
            }
        }
    }

    /// Linear blend. `t = 0` is `a`, `t = 1` is `b`.
    pub fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let mut out = Self::silence();
        for i in 0..N {
            out.amps[i] = a.amps[i] + (b.amps[i] - a.amps[i]) * t;
        }
        out
    }

    /// Write lerp into `dest` (no extra spectrum alloc).
    pub fn lerp_into(a: &Self, b: &Self, t: f32, dest: &mut Self) {
        let t = t.clamp(0.0, 1.0);
        for i in 0..N {
            dest.amps[i] = a.amps[i] + (b.amps[i] - a.amps[i]) * t;
        }
    }

    /// Even/odd mask. 1.0 leaves that series untouched, 0.0 kills it.
    pub fn apply_even_odd(&mut self, even: f32, odd: f32) {
        let even = even.clamp(0.0, 2.0);
        let odd = odd.clamp(0.0, 2.0);
        for k in 0..N {
            // k=0 is harmonic 1 (odd)
            let harmonic = k + 1;
            if harmonic % 2 == 0 {
                self.amps[k] *= even;
            } else {
                self.amps[k] *= odd;
            }
        }
    }

    /// Spectral tilt. Negative darkens highs, positive brightens.
    pub fn apply_tilt(&mut self, tilt: f32) {
        let tilt = tilt.clamp(-1.0, 1.0);
        if N <= 1 {
            return;
        }
        let last = (N - 1) as f32;
        for k in 1..N {
            let t = k as f32 / last;
            let gain = if tilt >= 0.0 {
                1.0 + tilt * t
            } else {
                1.0 + tilt * t
            };
            self.amps[k] *= gain.max(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_is_fundamental_only() {
        let s = HarmonicSpectrum::<64>::sine();
        assert!((s.partial(0) - 1.0).abs() < 1e-6);
        for k in 1..64 {
            assert_eq!(s.partial(k), 0.0);
        }
    }

    #[test]
    fn square_has_no_evens() {
        let s = HarmonicSpectrum::<16>::square();
        assert!(s.partial(0).abs() > 0.0);
        assert_eq!(s.partial(1), 0.0);
        assert!(s.partial(2).abs() > 0.0);
    }

    #[test]
    fn lerp_endpoints() {
        let a = HarmonicSpectrum::<8>::sine();
        let b = HarmonicSpectrum::<8>::saw();
        let at_a = HarmonicSpectrum::lerp(&a, &b, 0.0);
        let at_b = HarmonicSpectrum::lerp(&a, &b, 1.0);
        assert!((at_a.partial(0) - a.partial(0)).abs() < 1e-6);
        assert!((at_b.partial(3) - b.partial(3)).abs() < 1e-6);
    }
}
