// use crate::synthesis::utils::ParameterSmoother;
use crate::synthesis::utils::ParameterSmoother;

/// A simple wavetable oscillator supporting 1D linear interpolation.
/// Assumes a pre-computed wavetable.
#[derive(Debug, Clone)]
pub struct WavetableOscillator {
    table: Vec<f32>,
    phase: f32,
    sample_rate: f32,

    freq_smoother: ParameterSmoother,
}

impl WavetableOscillator {
    pub fn new(sample_rate: f32, table: Vec<f32>) -> Self {
        Self {
            table,
            phase: 0.0,
            sample_rate,
            freq_smoother: ParameterSmoother::new(sample_rate, 5.0, 440.0),
        }
    }

    /// Update the internal wavetable (e.g., when switching waveforms).
    pub fn set_table(&mut self, table: Vec<f32>) {
        self.table = table;
    }

    /// Set the target frequency for the oscillator.
    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq_smoother.set_target(freq_hz.max(1.0));
    }

    #[inline(always)]
    pub fn process_sample(&mut self) -> f32 {
        let freq = self.freq_smoother.process_sample();

        let table_len = self.table.len() as f32;
        let phase_increment = freq / self.sample_rate;

        // Calculate current index in the table
        let index = self.phase * table_len;

        // Linear interpolation
        let index_trunc = index as usize;
        let index_frac = index - index_trunc as f32;

        let next_index = if index_trunc + 1 >= self.table.len() { 0 } else { index_trunc + 1 };

        let sample1 = self.table[index_trunc];
        let sample2 = self.table[next_index];

        let output = sample1 + (sample2 - sample1) * index_frac;

        // Advance phase
        self.phase += phase_increment;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        output
    }

    #[inline]
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample();
        }
    }
}
