use crate::synthesis::utils::ParameterSmoother;

/// A simple wavetable oscillator supporting 1D linear interpolation.
/// Assumes a pre-computed wavetable that contains exactly 1 cycle.
#[derive(Debug, Clone)]
pub struct WavetableOscillator {
    table: &'static [f32],
    phase: f32,
    sample_rate: f32,

    freq_smoother: ParameterSmoother,
}

impl WavetableOscillator {
    pub fn new(sample_rate: f32, table: &'static [f32]) -> Self {
        Self {
            table,
            phase: 0.0,
            sample_rate,
            freq_smoother: ParameterSmoother::new(sample_rate, 5.0, 440.0),
        }
    }

    /// Update the internal wavetable reference (e.g., when switching waveforms).
    /// Does not allocate.
    pub fn set_table(&mut self, table: &'static [f32]) {
        self.table = table;
    }

    /// Set the target frequency for the oscillator.
    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq_smoother.set_target(freq_hz.max(1.0));
    }

    #[inline(always)]
    pub fn process_sample(&mut self) -> f32 {
        let table_len = self.table.len();
        if table_len == 0 {
            return 0.0;
        }

        let freq = self.freq_smoother.process_sample();
        let phase_increment = freq / self.sample_rate;

        let table_len_f = table_len as f32;

        // Calculate current index in the table
        let index = self.phase * table_len_f;

        // Linear interpolation
        let index_trunc = index as usize;
        let index_frac = index - index_trunc as f32;

        // Use mask if table length is power of 2, otherwise branch
        let next_index = if table_len.is_power_of_two() {
            (index_trunc + 1) & (table_len - 1)
        } else {
            if index_trunc + 1 >= table_len { 0 } else { index_trunc + 1 }
        };

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
