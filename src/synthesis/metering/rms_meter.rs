use crate::synthesis::math_utils::linear_to_db_approx;

/// Sliding-window root-mean-square (RMS) energy estimator using a circular running sum
/// to maintain O(1) updates per sample.
#[derive(Clone, Debug)]
pub struct RmsMeter {
    sample_rate: f32,
    window_ms: f32,
    window_samples: usize,
    buffer: Vec<f32>,
    write_idx: usize,
    running_sum_of_squares: f32,
}

impl RmsMeter {
    pub fn new(sample_rate: f32) -> Self {
        // Pre-allocate for 1000 ms (1 second) maximum window
        let max_samples = (1000.0 * 0.001 * sample_rate).ceil() as usize;

        let mut meter = Self {
            sample_rate,
            window_ms: 300.0, // Default window
            window_samples: 0,
            buffer: vec![0.0; max_samples],
            write_idx: 0,
            running_sum_of_squares: 0.0,
        };
        meter.set_window_ms(300.0);
        meter
    }

    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_idx = 0;
        self.running_sum_of_squares = 0.0;
    }

    pub fn set_window_ms(&mut self, ms: f32) {
        self.window_ms = ms.clamp(1.0, 1000.0);
        self.window_samples = (self.window_ms * 0.001 * self.sample_rate).ceil() as usize;
        // Clamp in case of floating point precision edge cases
        self.window_samples = self.window_samples.min(self.buffer.len());

        // Changing window requires recalculating the running sum to prevent glitches,
        // but for simplicity in real-time context, if window size changes we might just want to let it adapt
        // or fully clear. Since it's a meter, adapting over window time is usually fine.
        // We will just let the read pointer change and sum drift temporarily, or clear it.
        // To be perfectly robust against drift when window changes, we recalculate from buffer.
        self.recalculate_sum();
    }

    fn recalculate_sum(&mut self) {
        let mut sum = 0.0;
        let mut idx = self.write_idx;
        for _ in 0..self.window_samples {
            idx = if idx == 0 {
                self.buffer.len() - 1
            } else {
                idx - 1
            };
            // The buffer already holds squared values
            sum += self.buffer[idx];
        }
        self.running_sum_of_squares = sum;
    }

    #[inline(always)]
    pub fn process_sample(&mut self, sample: f32) {
        let squared = sample * sample;

        // Oldest sample in the window that we are replacing/removing from sum
        let read_idx =
            (self.write_idx + self.buffer.len() - self.window_samples) % self.buffer.len();
        let oldest_squared = self.buffer[read_idx];
        self.buffer[read_idx] = oldest_squared; // This line seems redundant but we actually want to read it.
                                                // Wait, the buffer stores the squared values or raw values?
                                                // Let's store squared values directly to save a multiplication on read.

        // Remove oldest from sum, add newest
        self.running_sum_of_squares -= oldest_squared;
        self.running_sum_of_squares += squared;

        // To prevent floating point drift over long periods, occasionally recalculate or clamp
        if self.running_sum_of_squares < 0.0 {
            self.running_sum_of_squares = 0.0;
        }

        // Store new squared value in buffer
        self.buffer[self.write_idx] = squared;

        self.write_idx = (self.write_idx + 1) % self.buffer.len();
    }

    #[inline(always)]
    pub fn get_rms_db(&self) -> f32 {
        if self.window_samples == 0 || self.running_sum_of_squares <= 1e-12 {
            return -144.0;
        }

        let mean_square = self.running_sum_of_squares / (self.window_samples as f32);
        let rms = mean_square.sqrt();

        if rms <= 1e-6 {
            -144.0
        } else {
            linear_to_db_approx(rms)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RmsMeter;
    use crate::synthesis::math_utils::TWO_PI;

    #[test]
    fn sine_wave_rms_accuracy() {
        let sample_rate = 48_000.0;
        let mut meter = RmsMeter::new(sample_rate);
        meter.set_window_ms(100.0);

        let freq = 1000.0;
        let amplitude = 1.0;

        // A sine wave of amplitude 1.0 has an RMS of 1/sqrt(2) approx 0.707
        // In dB, 20 * log10(0.707) approx -3.01 dB

        // Feed the meter more than the window length
        let num_samples = 10_000;
        for i in 0..num_samples {
            let t = i as f32 / sample_rate;
            let sample = amplitude * (TWO_PI * freq * t).sin();
            meter.process_sample(sample);
        }

        let rms_db = meter.get_rms_db();
        // Should be very close to -3.01 dB
        assert!((rms_db - (-3.01)).abs() < 0.1, "RMS dB was {}", rms_db);
    }
}
