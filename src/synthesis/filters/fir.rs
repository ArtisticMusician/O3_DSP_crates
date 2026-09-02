use crate::synthesis::utils::Processor;

/// Direct-form FIR. Coefficient length is fixed after construction unless
/// `update_coefficients` is called from a non-real-time thread.
pub struct FirFilter {
    coefficients: Vec<f32>,
    history: Vec<f32>,
    index: usize,
}

impl FirFilter {
    pub fn new(coefficients: Vec<f32>) -> Self {
        let len = coefficients.len().max(1);
        let coefficients = if coefficients.is_empty() {
            vec![1.0]
        } else {
            coefficients
        };
        Self {
            coefficients,
            history: vec![0.0; len],
            index: 0,
        }
    }

    /// Not real-time safe if the new length differs from the current length.
    pub fn update_coefficients(&mut self, new_coeffs: Vec<f32>) {
        if new_coeffs.is_empty() {
            return;
        }
        if self.history.len() != new_coeffs.len() {
            self.history = vec![0.0; new_coeffs.len()];
            self.index = 0;
        }
        self.coefficients = new_coeffs;
        if self.index >= self.coefficients.len() {
            self.index = 0;
        }
    }
}

impl Processor for FirFilter {
    #[inline(always)]
    fn process_sample(&mut self, sample: f32) -> f32 {
        let len = self.coefficients.len();
        self.history[self.index] = sample;

        let mut out = 0.0;
        let mut idx = self.index;
        for coeff in self.coefficients.iter() {
            out += *coeff * self.history[idx];
            idx = if idx == 0 { len - 1 } else { idx - 1 };
        }

        self.index += 1;
        if self.index == len {
            self.index = 0;
        }
        out
    }
}
