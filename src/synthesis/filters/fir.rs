use crate::synthesis::utils::Processor;

/// Finite Impulse Response (FIR) Filter.
/// Simple convolution with fixed coefficients (Direct Form).
pub struct FirFilter {
    coefficients: Vec<f32>,
    history: Vec<f32>,
    index: usize,
}

impl FirFilter {
    pub fn new(coefficients: Vec<f32>) -> Self {
        let len = coefficients.len();
        Self {
            coefficients,
            history: vec![0.0; len],
            index: 0,
        }
    }

    pub fn update_coefficients(&mut self, new_coeffs: Vec<f32>) {
        if self.history.len() != new_coeffs.len() {
            self.history.resize(new_coeffs.len(), 0.0);
        }
        self.coefficients = new_coeffs;

        // Empty coefficients make the filter a pass-through; keep the index in range.
        self.index = if self.coefficients.is_empty() {
            0
        } else {
            self.index % self.coefficients.len()
        };
    }
}

impl Processor for FirFilter {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        let len = self.coefficients.len();
        if len == 0 {
            return sample;
        }

        self.history[self.index] = sample;
        let mut out = 0.0;

        let mut idx = self.index;
        for i in 0..len {
            out += self.coefficients[i] * self.history[idx];
            if idx == 0 {
                idx = len - 1;
            } else {
                idx -= 1;
            }
        }

        self.index = (self.index + 1) % len;

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_coefficients_are_a_pass_through() {
        let mut fir = FirFilter::new(vec![0.5, 0.5]);

        // Must not panic (this used to divide by zero), and must pass the signal through.
        fir.update_coefficients(Vec::new());
        assert_eq!(fir.process_sample(0.25), 0.25);
        assert_eq!(fir.process_sample(-1.0), -1.0);
    }

    #[test]
    fn running_average_of_two_taps_averages() {
        let mut fir = FirFilter::new(vec![0.5, 0.5]);

        assert!((fir.process_sample(1.0) - 0.5).abs() < 1e-6);
        assert!((fir.process_sample(0.0) - 0.5).abs() < 1e-6);
        assert!(fir.process_sample(0.0).abs() < 1e-6);
    }
}
