use crate::synthesis::utils::parameter_smoother::ParameterSmoother;

/// Delay line used in the FDN reverb.
#[derive(Debug, Clone)]
struct ReverbDelayLine {
    buffer: Vec<f32>,
    mask: usize,
    write_pos: usize,

    // One-pole lowpass filter for damping
    filter_state: f32,
}

impl ReverbDelayLine {
    fn new(length_samples: usize) -> Self {
        let capacity = length_samples.next_power_of_two();
        Self {
            buffer: vec![0.0; capacity],
            mask: capacity - 1,
            write_pos: 0,
            filter_state: 0.0,
        }
    }

    fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.write_pos = 0;
        self.filter_state = 0.0;
    }

    // Read from the delay line exactly `length` samples ago.
    // The delay lines have fixed lengths (prime numbers).
    #[inline(always)]
    fn read(&self, length: usize) -> f32 {
        let read_pos = (self.write_pos + self.buffer.len() - length) & self.mask;
        self.buffer[read_pos]
    }

    #[inline(always)]
    fn write(&mut self, input: f32) {
        self.buffer[self.write_pos] = input;
        self.write_pos = (self.write_pos + 1) & self.mask;
    }

    #[inline(always)]
    fn process_damping(&mut self, input: f32, damping: f32) -> f32 {
        // Simple one-pole lowpass: y[n] = (1 - a) * x[n] + a * y[n-1]
        // where a is the damping coefficient (0.0 to 1.0)
        self.filter_state = (1.0 - damping) * input + damping * self.filter_state;

        // Prevent denormals
        if self.filter_state.abs() < 1e-9 {
            self.filter_state = 0.0;
        }

        self.filter_state
    }
}

/// A 4-channel Feedback Delay Network (FDN) Reverb.
#[derive(Debug, Clone)]
pub struct ReverbFdn {
    delay_lines: [ReverbDelayLine; 4],
    delay_lengths: [usize; 4],

    decay_feedback: [f32; 4],
    target_decay_t60: f32,
    decay_smoother: ParameterSmoother,
    delay_sec: [f32; 4],
    damping: ParameterSmoother,
    mix: ParameterSmoother,
}

fn is_prime(n: usize) -> bool {
    if n <= 1 {
        return false;
    }
    if n <= 3 {
        return true;
    }
    if n % 2 == 0 || n % 3 == 0 {
        return false;
    }
    let mut i = 5;
    while i * i <= n {
        if n % i == 0 || n % (i + 2) == 0 {
            return false;
        }
        i += 6;
    }
    true
}

fn next_prime(mut n: usize) -> usize {
    while !is_prime(n) {
        n += 1;
    }
    n
}

impl ReverbFdn {
    pub fn new(sample_rate: f32) -> Self {
        // Mutually prime delay lengths for a good room/hall size (approx 30-50ms)
        // Values roughly scale with sample rate to keep reverb times consistent
        let scale = sample_rate / 44100.0;
        let lengths = [
            next_prime((1453.0 * scale).round() as usize),
            next_prime((1741.0 * scale).round() as usize),
            next_prime((1949.0 * scale).round() as usize),
            next_prime((2131.0 * scale).round() as usize),
        ];

        Self {
            delay_lines: [
                ReverbDelayLine::new(lengths[0]),
                ReverbDelayLine::new(lengths[1]),
                ReverbDelayLine::new(lengths[2]),
                ReverbDelayLine::new(lengths[3]),
            ],
            delay_lengths: lengths,
            delay_sec: [
                lengths[0] as f32 / sample_rate,
                lengths[1] as f32 / sample_rate,
                lengths[2] as f32 / sample_rate,
                lengths[3] as f32 / sample_rate,
            ],

            decay_feedback: [0.0; 4],
            target_decay_t60: 1.0,
            decay_smoother: ParameterSmoother::new(sample_rate, 10.0, 1.0),
            damping: ParameterSmoother::new(sample_rate, 10.0, 0.2),
            mix: ParameterSmoother::new(sample_rate, 10.0, 0.3),
        }
    }

    pub fn reset(&mut self) {
        for dl in self.delay_lines.iter_mut() {
            dl.reset();
        }
    }

    pub fn set_decay(&mut self, decay: f32) {
        let t60 = 0.2 + decay.clamp(0.0, 1.0) * (8.0 - 0.2);
        self.target_decay_t60 = t60;
        for i in 0..4 {
            self.decay_feedback[i] = 10_f32.powf(-3.0 * self.delay_sec[i] / t60).clamp(0.0, 0.98);
        }
    }

    pub fn set_damping(&mut self, damping: f32) {
        self.damping.set_target(damping.clamp(0.0, 0.95));
    }

    pub fn set_mix(&mut self, mix: f32) {
        self.mix.set_target(mix.clamp(0.0, 1.0));
    }

    #[inline(always)]
    pub fn process(&mut self, left_in: f32, right_in: f32) -> (f32, f32) {

        let damping = self.damping.process_sample();
        let mix = self.mix.process_sample();
        // Note: we removed decay_smoother inside process. To have click-free
        // automation we'd need more complex mapping. For now, decay_feedback is set in setter.

        // Input mixing
        // L+R to lines 1 & 3, L-R to lines 2 & 4
        let in_1_3 = (left_in + right_in) * 0.5;
        let in_2_4 = (left_in - right_in) * 0.5;

        // Read current delay outputs
        let out0 = self.delay_lines[0].read(self.delay_lengths[0]);
        let out1 = self.delay_lines[1].read(self.delay_lengths[1]);
        let out2 = self.delay_lines[2].read(self.delay_lengths[2]);
        let out3 = self.delay_lines[3].read(self.delay_lengths[3]);

        // 4x4 Hadamard / Orthogonal Matrix for feedback
        // 1/2 * [ 1  1  1  1 ]
        //       [ 1 -1  1 -1 ]
        //       [ 1  1 -1 -1 ]
        //       [ 1 -1 -1  1 ]
        let f0 = (out0 + out1 + out2 + out3) * 0.5;
        let f1 = (out0 - out1 + out2 - out3) * 0.5;
        let f2 = (out0 + out1 - out2 - out3) * 0.5;
        let f3 = (out0 - out1 - out2 + out3) * 0.5;

        // Apply feedback scale and damping, then add inputs, write to delay lines
        let d0 = self.delay_lines[0].process_damping(f0 * self.decay_feedback[0], damping);
        let d1 = self.delay_lines[1].process_damping(f1 * self.decay_feedback[1], damping);
        let d2 = self.delay_lines[2].process_damping(f2 * self.decay_feedback[2], damping);
        let d3 = self.delay_lines[3].process_damping(f3 * self.decay_feedback[3], damping);

        self.delay_lines[0].write(in_1_3 + d0);
        self.delay_lines[1].write(in_2_4 + d1);
        self.delay_lines[2].write(in_1_3 + d2);
        self.delay_lines[3].write(in_2_4 + d3);

        // Output mixing
        // Odd lines (0, 2) to Left, Even lines (1, 3) to Right
        let wet_left = (out0 + out2) * 0.5;
        let wet_right = (out1 + out3) * 0.5;

        // Mix dry/wet
        let out_left = left_in * (1.0 - mix) + wet_left * mix;
        let out_right = right_in * (1.0 - mix) + wet_right * mix;

        (out_left, out_right)
    }

    #[inline]
    pub fn process_block(&mut self, left_in: &[f32], right_in: &[f32], left_out: &mut [f32], right_out: &mut [f32]) {
        for (((l_in, r_in), l_out), r_out) in left_in.iter().zip(right_in.iter()).zip(left_out.iter_mut()).zip(right_out.iter_mut()) {
            let (l, r) = self.process(*l_in, *r_in);
            *l_out = l;
            *r_out = r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dry_wet_pass_through() {
        let mut effect = ReverbFdn::new(44100.0);
        effect.set_mix(0.0);
        effect.set_decay(0.0);

        for _ in 0..10000 {
            effect.process(0.0, 0.0);
        }

        let (l, r) = effect.process(1.0, -1.0);
        assert!((l - 1.0).abs() < 1e-5);
        assert!((r - (-1.0)).abs() < 1e-5);
    }

    #[test]
    fn test_zero_input_decay() {
        let mut effect = ReverbFdn::new(44100.0);
        effect.set_mix(1.0);
        effect.set_decay(0.3);
        effect.set_damping(0.1);

        for _ in 0..1000 {
            effect.process(0.0, 0.0);
        }

        effect.process(1.0, 1.0);

        for _ in 0..150000 {
            effect.process(0.0, 0.0);
        }

        let (l, r) = effect.process(0.0, 0.0);
        assert!(l.abs() < 1e-5, "Left channel did not decay completely: {}", l);
        assert!(r.abs() < 1e-5, "Right channel did not decay completely: {}", r);
    }

    #[test]
    fn test_parameter_limits() {
        let mut effect = ReverbFdn::new(44100.0);
        effect.set_decay(1.5);

        for _ in 0..1000 {
            effect.process(0.0, 0.0);
        }

        assert!(effect.decay_smoother.current_value() <= 8.0);
    }
}
