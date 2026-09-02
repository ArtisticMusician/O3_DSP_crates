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
        self.index = self.index % self.coefficients.len();
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
