use crate::synthesis::utils::ParameterSmoother;

/// One-cycle wavetable oscillator with linear interpolation.
/// Table must be non-empty. Power-of-two lengths use a bitmask wrap.
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

    pub fn set_table(&mut self, table: &'static [f32]) {
        self.table = table;
        if self.table.is_empty() {
            self.phase = 0.0;
        }
    }

    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.freq_smoother.set_target(freq_hz.clamp(0.0, self.sample_rate * 0.45));
    }

    #[inline(always)]
    pub fn process_sample(&mut self) -> f32 {
        let table_len = self.table.len();
        if table_len == 0 {
            return 0.0;
        }

        let freq = self.freq_smoother.process_sample();
        let table_len_f = table_len as f32;
        let index = self.phase * table_len_f;
        let index_trunc = index as usize;
        let index_frac = index - index_trunc as f32;

        let (i0, i1) = if table_len.is_power_of_two() {
            let mask = table_len - 1;
            (index_trunc & mask, (index_trunc + 1) & mask)
        } else {
            let i0 = if index_trunc >= table_len { 0 } else { index_trunc };
            let i1 = i0 + 1;
            (i0, if i1 >= table_len { 0 } else { i1 })
        };

        let output = self.table[i0] + (self.table[i1] - self.table[i0]) * index_frac;

        self.phase += freq / self.sample_rate;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
        } else if self.phase < 0.0 {
            self.phase -= self.phase.floor();
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
