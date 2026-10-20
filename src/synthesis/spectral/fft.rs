use std::f32::consts::TAU;

/// A complex number, enough for an FFT and nothing more.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Complex {
    pub re: f32,
    pub im: f32,
}

impl Complex {
    #[inline(always)]
    pub const fn new(re: f32, im: f32) -> Self {
        Self { re, im }
    }

    #[inline(always)]
    pub const fn zero() -> Self {
        Self { re: 0.0, im: 0.0 }
    }

    #[inline(always)]
    pub fn magnitude(self) -> f32 {
        (self.re * self.re + self.im * self.im).sqrt()
    }

    #[inline(always)]
    fn add(self, other: Self) -> Self {
        Self::new(self.re + other.re, self.im + other.im)
    }

    #[inline(always)]
    fn sub(self, other: Self) -> Self {
        Self::new(self.re - other.re, self.im - other.im)
    }

    #[inline(always)]
    fn mul(self, other: Self) -> Self {
        Self::new(
            self.re * other.re - self.im * other.im,
            self.re * other.im + self.im * other.re,
        )
    }
}

/// Radix-2 Cooley-Tukey FFT.
///
/// Dependency-free and allocation-free after construction: the bit-reversal permutation and the
/// twiddle factors are precomputed once, so `forward`/`inverse` are just butterflies over the
/// caller's buffer. Not the fastest FFT in existence - it is the one that keeps this crate's
/// dependency list at zero.
///
/// Sizes are rounded up to a power of two (minimum 4).
#[derive(Debug, Clone)]
pub struct Fft {
    size: usize,
    /// cos(2*pi*k/n) for k in 0..size
    cos_table: Vec<f32>,
    /// sin(2*pi*k/n) for k in 0..size
    sin_table: Vec<f32>,
    reversed: Vec<usize>,
}

impl Fft {
    /// Create an FFT of `size` points, rounded up to a power of two.
    pub fn new(size: usize) -> Self {
        let size = size.max(4).next_power_of_two();
        let bits = size.trailing_zeros();

        let mut cos_table = Vec::with_capacity(size);
        let mut sin_table = Vec::with_capacity(size);
        for k in 0..size {
            let angle = TAU * k as f32 / size as f32;
            cos_table.push(angle.cos());
            sin_table.push(angle.sin());
        }

        let reversed = (0..size)
            .map(|index| (index as u32).reverse_bits() >> (32 - bits))
            .map(|index| index as usize)
            .collect();

        Self {
            size,
            cos_table,
            sin_table,
            reversed,
        }
    }

    /// Number of points.
    #[inline]
    pub fn size(&self) -> usize {
        self.size
    }

    /// Forward transform, in place. `buffer.len()` must equal [`Self::size`].
    pub fn forward(&self, buffer: &mut [Complex]) {
        self.transform(buffer, false);
    }

    /// Inverse transform, in place, scaled by `1 / size`.
    pub fn inverse(&self, buffer: &mut [Complex]) {
        self.transform(buffer, true);
    }

    /// Forward transform of a real signal. `output.len()` must equal [`Self::size`].
    pub fn forward_real(&self, input: &[f32], output: &mut [Complex]) {
        for (slot, &sample) in output.iter_mut().zip(input) {
            *slot = Complex::new(sample, 0.0);
        }
        for slot in output.iter_mut().skip(input.len()) {
            *slot = Complex::zero();
        }

        self.transform(output, false);
    }

    fn transform(&self, buffer: &mut [Complex], inverse: bool) {
        let size = self.size;
        if buffer.len() != size {
            return;
        }

        // Bit-reversal permutation.
        for index in 0..size {
            let target = self.reversed[index];
            if target > index {
                buffer.swap(index, target);
            }
        }

        // Butterflies, doubling the block length each pass.
        let mut block = 2;
        while block <= size {
            let half = block / 2;
            let stride = size / block;

            for start in (0..size).step_by(block) {
                let mut twiddle_index = 0;

                for offset in 0..half {
                    let cos = self.cos_table[twiddle_index];
                    let sin = self.sin_table[twiddle_index];
                    let twiddle = if inverse {
                        Complex::new(cos, sin)
                    } else {
                        Complex::new(cos, -sin)
                    };

                    let even = buffer[start + offset];
                    let odd = buffer[start + offset + half].mul(twiddle);

                    buffer[start + offset] = even.add(odd);
                    buffer[start + offset + half] = even.sub(odd);

                    twiddle_index += stride;
                }
            }

            block <<= 1;
        }

        if inverse {
            let scale = 1.0 / size as f32;
            for value in buffer.iter_mut() {
                value.re *= scale;
                value.im *= scale;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_signal(size: usize) -> Vec<f32> {
        // A mix of a few tones plus a little deterministic "noise", so the round trip is not
        // trivially symmetric.
        (0..size)
            .map(|i| {
                let t = i as f32 / size as f32;
                (TAU * t * 3.0).sin() * 0.5
                    + (TAU * t * 11.0).cos() * 0.25
                    + ((i * 2654435761usize) % 1000) as f32 / 2000.0
                    - 0.25
            })
            .collect()
    }

    #[test]
    fn round_trip_restores_the_signal() {
        for size in [4, 8, 64, 1024] {
            let fft = Fft::new(size);
            let original = test_signal(size);

            let mut buffer: Vec<Complex> = original.iter().map(|&s| Complex::new(s, 0.0)).collect();
            fft.forward(&mut buffer);
            fft.inverse(&mut buffer);

            let worst = buffer
                .iter()
                .zip(&original)
                .map(|(c, &o)| (c.re - o).abs())
                .fold(0.0f32, f32::max);

            assert!(worst < 1e-4, "size {size} round trip drifted by {worst}");
            assert!(buffer.iter().all(|c| c.im.abs() < 1e-4), "size {size} left imaginary residue");
        }
    }

    #[test]
    fn a_bin_centred_sine_lands_in_that_bin() {
        let size = 256;
        let bin = 16;
        let fft = Fft::new(size);

        let input: Vec<f32> = (0..size)
            .map(|i| (TAU * bin as f32 * i as f32 / size as f32).sin())
            .collect();

        let mut buffer = vec![Complex::zero(); size];
        fft.forward_real(&input, &mut buffer);

        // A unit sine lands in bin+/-1 with magnitude size/2.
        let expected = size as f32 / 2.0;
        assert!(
            (buffer[bin].magnitude() - expected).abs() < 1.0,
            "bin {bin} magnitude {}",
            buffer[bin].magnitude()
        );

        // Everything else must be near zero.
        for index in 0..size {
            if index != bin && index != size - bin {
                assert!(
                    buffer[index].magnitude() < 1.0,
                    "leakage into bin {index}: {}",
                    buffer[index].magnitude()
                );
            }
        }
    }

    #[test]
    fn dc_lands_in_bin_zero() {
        let size = 64;
        let fft = Fft::new(size);

        let mut buffer = vec![Complex::new(1.0, 0.0); size];
        fft.forward(&mut buffer);

        assert!((buffer[0].re - size as f32).abs() < 1e-3, "dc bin {}", buffer[0].re);
        for index in 1..size {
            assert!(buffer[index].magnitude() < 1e-3, "leakage into bin {index}");
        }
    }

    #[test]
    fn sizes_are_rounded_up_to_a_power_of_two() {
        assert_eq!(Fft::new(0).size(), 4);
        assert_eq!(Fft::new(3).size(), 4);
        assert_eq!(Fft::new(1000).size(), 1024);
        assert_eq!(Fft::new(1024).size(), 1024);
    }

    #[test]
    fn a_wrong_sized_buffer_is_ignored_rather_than_slicing_out_of_bounds() {
        let fft = Fft::new(64);
        let mut tiny = vec![Complex::new(1.0, 2.0); 8];
        fft.forward(&mut tiny);

        assert_eq!(tiny[0], Complex::new(1.0, 2.0), "short buffer should be left alone");
    }

    #[test]
    fn linearity_holds() {
        let size = 128;
        let fft = Fft::new(size);

        let a: Vec<f32> = (0..size).map(|i| (i as f32 * 0.1).sin()).collect();
        let b: Vec<f32> = (0..size).map(|i| (i as f32 * 0.037).cos()).collect();

        let transform = |signal: &[f32]| {
            let mut buffer = vec![Complex::zero(); size];
            fft.forward_real(signal, &mut buffer);
            buffer
        };

        let sum: Vec<f32> = a.iter().zip(&b).map(|(x, y)| x + y).collect();
        let expected = transform(&sum);
        let separate: Vec<Complex> = transform(&a)
            .iter()
            .zip(transform(&b))
            .map(|(x, y)| x.add(y))
            .collect();

        for (index, (one, other)) in expected.iter().zip(&separate).enumerate() {
            assert!(
                (one.re - other.re).abs() < 1e-3 && (one.im - other.im).abs() < 1e-3,
                "bin {index} not linear"
            );
        }
    }
}
