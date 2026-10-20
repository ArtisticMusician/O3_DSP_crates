use crate::synthesis::spectral::Partial;
use crate::synthesis::utils::ParameterSmoother;
use std::f32::consts::TAU;

const DEFAULT_GLIDE_MS: f32 = 10.0;

#[derive(Debug, Clone)]
struct Voice {
    phase: f32,
    frequency: ParameterSmoother,
    amplitude: ParameterSmoother,
}

impl Voice {
    fn new(sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            frequency: ParameterSmoother::new(sample_rate, DEFAULT_GLIDE_MS, 0.0),
            amplitude: ParameterSmoother::new(sample_rate, DEFAULT_GLIDE_MS, 0.0),
        }
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.frequency.reset(0.0);
        self.amplitude.reset(0.0);
    }
}

/// A bank of sine partials - the resynthesis half of additive synthesis.
///
/// Partials are assigned to voices by index, so feed this a list that is stable frame to frame
/// (the analyser returns them sorted by frequency, which is what makes that work). When a voice
/// changes frequency or amplitude it glides rather than jumping, which is what stops the frame
/// updates from clicking; a voice that is currently silent snaps its frequency instead of
/// gliding to the new partial, because there is no pitch to hear sliding from.
///
/// Voices are summed without normalisation, so the caller decides the output level. The output
/// can exceed +/-1 for dense spectra - that is inherent to additive synthesis, not a bug.
#[derive(Debug, Clone)]
pub struct SineBank {
    voices: Vec<Voice>,
    sample_rate: f32,
    glide_ms: f32,
    active_voices: usize,
}

impl SineBank {
    /// `capacity` is how many simultaneous partials the bank can hold.
    pub fn new(sample_rate: f32, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            voices: (0..capacity).map(|_| Voice::new(sample_rate)).collect(),
            sample_rate: sample_rate.max(1.0),
            glide_ms: DEFAULT_GLIDE_MS,
            active_voices: 0,
        }
    }

    /// How many partials the bank can hold at once.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.voices.len()
    }

    /// How many voices were assigned a partial by the last [`Self::set_partials`].
    #[inline]
    pub fn active_voices(&self) -> usize {
        self.active_voices
    }

    #[inline]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// How long a voice takes to reach a new frequency or amplitude.
    pub fn set_glide_ms(&mut self, glide_ms: f32) {
        self.glide_ms = glide_ms.clamp(0.0, 1_000.0);
        for voice in &mut self.voices {
            voice.frequency.set_time(self.sample_rate, self.glide_ms);
            voice.amplitude.set_time(self.sample_rate, self.glide_ms);
        }
    }

    /// Point the bank at a new set of partials.
    ///
    /// Any voice beyond the end of `partials` is faded out rather than cut, and anything past
    /// [`Self::capacity`] is dropped.
    pub fn set_partials(&mut self, partials: &[Partial]) {
        let nyquist = self.sample_rate * 0.5;
        let count = partials.len().min(self.voices.len());

        for (voice, partial) in self.voices.iter_mut().zip(partials).take(count) {
            let target = partial.frequency.clamp(0.0, nyquist);

            // A silent voice has no audible pitch to glide from, so it jumps.
            if voice.amplitude.current_value().abs() < 1e-6 {
                voice.frequency.reset(target);
                voice.phase = 0.0;
            } else {
                voice.frequency.set_target(target);
            }

            voice.amplitude.set_target(partial.amplitude);
        }

        for voice in self.voices.iter_mut().skip(count) {
            voice.amplitude.set_target(0.0);
        }

        self.active_voices = count;
    }

    /// Silence every voice, keeping its position in the bank.
    pub fn silence(&mut self) {
        for voice in &mut self.voices {
            voice.amplitude.set_target(0.0);
        }
        self.active_voices = 0;
    }

    /// Zero the bank: phases, smoothers and assignment.
    pub fn reset(&mut self) {
        for voice in &mut self.voices {
            voice.reset();
        }
        self.active_voices = 0;
    }

    /// One sample of the summed bank.
    pub fn process_sample(&mut self) -> f32 {
        let sample_rate = self.sample_rate;
        let mut output = 0.0;

        for voice in &mut self.voices {
            let amplitude = voice.amplitude.process_sample();
            let frequency = voice.frequency.process_sample();

            if frequency > 0.0 {
                voice.phase += frequency / sample_rate;
                if voice.phase >= 1.0 {
                    voice.phase -= voice.phase.floor();
                }
            }

            output += (TAU * voice.phase).sin() * amplitude;
        }

        output
    }

    /// Fill a buffer.
    pub fn process_block(&mut self, output: &mut [f32]) {
        for sample in output.iter_mut() {
            *sample = self.process_sample();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    /// Count upward zero crossings - a cheap, independent way to measure pitch without reusing
    /// the FFT this bank is meant to be checked against.
    fn frequency_from_zero_crossings(buffer: &[f32], sample_rate: f32) -> f32 {
        let crossings = buffer
            .windows(2)
            .filter(|pair| pair[0] <= 0.0 && pair[1] > 0.0)
            .count();
        crossings as f32 * sample_rate / buffer.len() as f32
    }

    fn rms(buffer: &[f32]) -> f32 {
        (buffer.iter().map(|s| s * s).sum::<f32>() / buffer.len() as f32).sqrt()
    }

    fn render(bank: &mut SineBank, samples: usize) -> Vec<f32> {
        (0..samples).map(|_| bank.process_sample()).collect()
    }

    #[test]
    fn one_partial_is_a_sine_at_that_frequency() {
        let mut bank = SineBank::new(SAMPLE_RATE, 32);
        bank.set_partials(&[Partial::new(440.0, 0.5, 0.0)]);
        bank.set_glide_ms(1.0);

        // Let the glide settle, then measure.
        render(&mut bank, 4800);
        let output = render(&mut bank, SAMPLE_RATE as usize);

        let frequency = frequency_from_zero_crossings(&output, SAMPLE_RATE);
        assert!(
            (frequency - 440.0).abs() < 5.0,
            "expected ~440 Hz, measured {frequency} Hz"
        );

        // A 0.5 amplitude sine has an RMS of 0.5/sqrt(2).
        let measured = rms(&output);
        assert!(
            (measured - 0.3536).abs() < 0.02,
            "expected RMS ~0.354, measured {measured}"
        );
    }

    #[test]
    fn two_partials_sum_without_normalisation() {
        let mut bank = SineBank::new(SAMPLE_RATE, 32);
        bank.set_partials(&[Partial::new(440.0, 0.5, 0.0), Partial::new(1320.0, 0.25, 0.0)]);
        bank.set_glide_ms(1.0);

        render(&mut bank, 4800);
        let output = render(&mut bank, SAMPLE_RATE as usize);

        // Uncorrelated sines: RMS = sqrt((0.5^2 + 0.25^2)/2).
        let expected = ((0.25 + 0.0625) / 2.0f32).sqrt();
        let measured = rms(&output);
        assert!(
            (measured - expected).abs() < 0.02,
            "expected RMS ~{expected}, measured {measured}"
        );

        // The 3rd harmonic's peaks do not line up with the fundamental's, so the sum peaks well
        // below the naive 0.75: maximising 0.5*sin(x) + 0.25*sin(3x) gives ~0.538.
        let peak = output.iter().fold(0.0f32, |acc, s| acc.max(s.abs()));
        assert!((peak - 0.538).abs() < 0.01, "peak {peak}");
    }

    #[test]
    fn a_removed_partial_fades_instead_of_cutting() {
        let mut bank = SineBank::new(SAMPLE_RATE, 32);
        bank.set_glide_ms(5.0);
        bank.set_partials(&[Partial::new(440.0, 0.5, 0.0)]);
        render(&mut bank, 4800);

        // Empty set: everything targets zero.
        bank.set_partials(&[]);
        let fade = render(&mut bank, 4800);
        let after = render(&mut bank, 4800);

        assert!(rms(&fade) > 0.0, "should still be sounding during the fade");
        assert!(
            rms(&after) < 1e-3,
            "should be silent once the glide completes, RMS {}",
            rms(&after)
        );
    }

    #[test]
    fn silent_voices_snap_to_their_new_frequency() {
        let mut bank = SineBank::new(SAMPLE_RATE, 8);
        bank.set_glide_ms(200.0); // long glide, so a glide would be obvious
        bank.set_partials(&[Partial::new(200.0, 0.5, 0.0)]);
        render(&mut bank, 100); // barely sounded
        bank.silence();
        // The amplitude smoother is exponential, so at 200 ms it needs ~127k samples to fall
        // under the 1e-6 silence threshold. Fade fast instead, then restore the long glide.
        bank.set_glide_ms(1.0);
        render(&mut bank, 4_800); // fully faded to silence
        bank.set_glide_ms(200.0); // long glide again, so a glide would be obvious

        bank.set_partials(&[Partial::new(2000.0, 0.5, 0.0)]);
        let output = render(&mut bank, 4800);

        let frequency = frequency_from_zero_crossings(&output, SAMPLE_RATE);
        assert!(
            (frequency - 2000.0).abs() < 40.0,
            "a silent voice should jump to 2000 Hz, measured {frequency} Hz"
        );
    }

    #[test]
    fn capacity_is_enforced() {
        let mut bank = SineBank::new(SAMPLE_RATE, 4);
        let partials: Vec<Partial> = (1..=16)
            .map(|i| Partial::new(i as f32 * 200.0, 0.1, 0.0))
            .collect();

        bank.set_partials(&partials);

        assert_eq!(bank.capacity(), 4);
        assert_eq!(bank.active_voices(), 4);
    }

    #[test]
    fn frequencies_are_clamped_to_nyquist() {
        let mut bank = SineBank::new(SAMPLE_RATE, 4);
        bank.set_partials(&[Partial::new(40_000.0, 0.5, 0.0)]);
        bank.set_glide_ms(1.0);
        render(&mut bank, 4800);

        let output = render(&mut bank, 4800);
        let frequency = frequency_from_zero_crossings(&output, SAMPLE_RATE);

        assert!(
            frequency <= SAMPLE_RATE * 0.5 + 1.0,
            "aliased above Nyquist: {frequency} Hz"
        );
    }

    #[test]
    fn reset_returns_the_bank_to_silence() {
        let mut bank = SineBank::new(SAMPLE_RATE, 8);
        bank.set_glide_ms(1.0);
        bank.set_partials(&[Partial::new(440.0, 0.9, 0.0)]);
        render(&mut bank, 4800);

        bank.reset();
        bank.set_partials(&[]);
        let output = render(&mut bank, 64);

        assert!(output.iter().all(|s| *s == 0.0), "reset should zero the voices");
    }
}
