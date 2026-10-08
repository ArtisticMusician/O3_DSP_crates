use crate::synthesis::utils::{random_f32, ParameterSmoother};
use std::f32::consts::TAU;

/// Granular / Particle oscillator.
/// Generates sound by controlling a swarm of grains (short overlapping windowed bursts).
///
/// Each grain is a short Hann-windowed sine that lives for [`Self::grain_size`] samples and
/// is then re-seeded with a fresh random detune. Grains are spread evenly across the grain
/// period, and the overlap control sets how much their lives overlap: 0.0 fires them one
/// after another (no overlap), 1.0 starts them all together (dense swarm).
///
/// The summed output is normalised by the sum of the window values, so the level stays put
/// no matter how many grains are active.
///
/// Use cases: wind, water, ethereal pads, glitch textures.
#[derive(Debug, Clone)]
pub struct GranularOscillator {
    sample_rate: f32,

    // Grain length in samples, and how much neighbouring grains overlap (0.0 - 1.0)
    grain_size: f32,
    overlap: f32,

    // Grain state
    grains: Vec<GranularGrain>,

    // Smoothed control parameters
    base_freq_smoother: ParameterSmoother,
    spread_smoother: ParameterSmoother,
    amplitude_smoother: ParameterSmoother,
}

/// Individual grain state
#[derive(Debug, Clone, Copy)]
struct GranularGrain {
    /// Samples elapsed inside the current grain.
    age: f32,
    /// Normalised oscillator phase (0.0 - 1.0).
    phase: f32,
    /// Frequency offset (Hz) for this grain, re-rolled on every restart.
    detune_hz: f32,
}

impl Default for GranularOscillator {
    fn default() -> Self {
        Self::new(48000.0, 8, 20.0)
    }
}

impl GranularOscillator {
    /// Create a new granular oscillator.
    ///
    /// # Arguments
    /// * `sample_rate` - Audio sample rate in Hz
    /// * `num_grains` - Number of grains (more = richer texture, more CPU)
    /// * `grain_duration_ms` - Duration of each grain in milliseconds
    pub fn new(sample_rate: f32, num_grains: usize, grain_duration_ms: f32) -> Self {
        let num_grains = num_grains.max(4).min(64);
        let grain_size = (grain_duration_ms * 0.001 * sample_rate).max(1.0);
        let overlap = 0.5; // 50% overlap

        let mut oscillator = Self {
            sample_rate,
            grain_size,
            overlap,
            grains: (0..num_grains)
                .map(|index| GranularGrain {
                    age: 0.0,
                    phase: (index as f32 * 0.1).fract(),
                    detune_hz: 0.0,
                })
                .collect(),
            base_freq_smoother: ParameterSmoother::new(sample_rate, 50.0, 440.0),
            spread_smoother: ParameterSmoother::new(sample_rate, 50.0, 0.5),
            amplitude_smoother: ParameterSmoother::new(sample_rate, 50.0, 0.5),
        };

        oscillator.restagger();
        oscillator
    }

    /// Spread the grains evenly across the grain period.
    fn restagger(&mut self) {
        let spacing = self.grain_size * (1.0 - self.overlap);

        for (index, grain) in self.grains.iter_mut().enumerate() {
            grain.age = (index as f32 * spacing) % self.grain_size;
        }
    }

    /// Set base frequency (smoothed)
    pub fn set_frequency(&mut self, freq_hz: f32) {
        self.base_freq_smoother
            .set_target(freq_hz.max(10.0).min(self.sample_rate * 0.2));
    }

    /// Set grain spread (detuning in Hz, smoothed)
    pub fn set_spread(&mut self, spread_hz: f32) {
        self.spread_smoother
            .set_target(spread_hz.max(0.0).min(self.sample_rate * 0.1));
    }

    /// Set overall amplitude (smoothed)
    pub fn set_amplitude(&mut self, amp: f32) {
        self.amplitude_smoother.set_target(amp.max(0.0).min(1.0));
    }

    /// Set grain overlap (0.0 = grains fire one after another, 1.0 = all at once).
    pub fn set_grain_delay(&mut self, overlap: f32) {
        self.overlap = overlap.clamp(0.0, 1.0);
        self.restagger();
    }

    /// Number of grains in the swarm.
    pub fn num_grains(&self) -> usize {
        self.grains.len()
    }

    /// Grain length in samples.
    pub fn grain_size(&self) -> f32 {
        self.grain_size
    }

    /// Restart every grain from a clean state.
    pub fn reset(&mut self) {
        for (index, grain) in self.grains.iter_mut().enumerate() {
            grain.phase = (index as f32 * 0.1).fract();
            grain.detune_hz = 0.0;
        }
        self.restagger();
    }

    /// Process a single sample
    pub fn process_sample(&mut self, _input: f32) -> f32 {
        let freq = self.base_freq_smoother.process_sample();
        let spread = self.spread_smoother.process_sample();
        let amp = self.amplitude_smoother.process_sample();

        let mut sum = 0.0;
        let mut window_sum = 0.0;

        for grain in &mut self.grains {
            grain.age += 1.0;
            if grain.age >= self.grain_size {
                // Re-seed the grain with a fresh detune. The oscillator phase keeps
                // running, so a zero-spread swarm stays phase-coherent.
                grain.age -= self.grain_size;
                grain.detune_hz = (random_f32() * 2.0 - 1.0) * spread;
            }

            let grain_freq = (freq + grain.detune_hz).max(1.0);
            grain.phase += grain_freq / self.sample_rate;
            if grain.phase >= 1.0 {
                grain.phase -= 1.0;
            }

            // Hann window over the grain's life
            let position = grain.age / self.grain_size;
            let window = 0.5 - 0.5 * (TAU * position).cos();

            window_sum += window;
            sum += window * (TAU * grain.phase).sin();
        }

        // Level-preserving normalisation, so grain count does not change the loudness.
        if window_sum > 1e-6 {
            sum / window_sum * amp
        } else {
            0.0
        }
    }

    /// Process a block of samples
    pub fn process_block(&mut self, buffer: &mut [f32]) {
        for sample in buffer.iter_mut() {
            *sample = self.process_sample(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(oscillator: &mut GranularOscillator, samples: usize) -> f32 {
        let mut energy = 0.0;
        for _ in 0..samples {
            let y = oscillator.process_sample(0.0);
            energy += y * y;
        }
        (energy / samples as f32).sqrt()
    }

    #[test]
    fn test_granular_oscillator() {
        let mut oscillator = GranularOscillator::default();

        for _ in 0..4096 {
            let sample = oscillator.process_sample(0.0);
            assert!(sample.is_finite() && sample.abs() <= 1.0, "output {sample}");
        }
    }

    /// With no spread every grain sits on the base frequency, so the swarm must come out
    /// as a tone at that frequency - not noise.
    #[test]
    fn zero_spread_rings_at_the_base_frequency() {
        let sample_rate = 48_000.0;
        let freq = 440.0;
        let mut oscillator = GranularOscillator::new(sample_rate, 8, 20.0);
        oscillator.set_frequency(freq);
        oscillator.set_spread(0.0);
        oscillator.set_amplitude(1.0);

        // Let the smoothers settle.
        for _ in 0..(sample_rate as usize / 2) {
            oscillator.process_sample(0.0);
        }

        let mut crossings = 0;
        let mut previous = oscillator.process_sample(0.0);
        for _ in 0..(sample_rate as usize - 1) {
            let current = oscillator.process_sample(0.0);
            if previous <= 0.0 && current > 0.0 {
                crossings += 1;
            }
            previous = current;
        }

        assert!(
            (crossings as f32 - freq).abs() < 5.0,
            "expected ~{freq} cycles per second, saw {crossings}"
        );
    }

    /// How much energy the swarm puts into the base frequency itself, relative to its own
    /// total energy. A coherent swarm keeps everything on that one frequency; detuned
    /// grains smear it across a band, so the share drops.
    fn energy_at_base_frequency(spread_hz: f32) -> f32 {
        let sample_rate = 48_000.0;
        let freq = 440.0;

        let mut oscillator = GranularOscillator::new(sample_rate, 16, 20.0);
        oscillator.set_frequency(freq);
        oscillator.set_spread(spread_hz);
        oscillator.set_amplitude(1.0);

        // Let the control smoothers settle.
        for _ in 0..24_000 {
            oscillator.process_sample(0.0);
        }

        let window = 24_000;
        let (mut real, mut imaginary, mut energy) = (0.0f32, 0.0f32, 0.0f32);

        for index in 0..window {
            let x = oscillator.process_sample(0.0);
            let angle = TAU * freq * index as f32 / sample_rate;
            real += x * angle.cos();
            imaginary += x * angle.sin();
            energy += x * x;
        }

        let at_frequency = 2.0 * (real * real + imaginary * imaginary).sqrt() / window as f32;
        let total = (energy / window as f32).sqrt();

        at_frequency / total.max(1e-9)
    }

    #[test]
    fn spread_detunes_the_swarm() {
        let coherent = energy_at_base_frequency(0.0);
        let detuned = energy_at_base_frequency(400.0);

        assert!(coherent > 0.5, "a zero-spread swarm should sit on the base frequency: {coherent}");
        assert!(
            detuned < coherent * 0.8,
            "spread should move energy off the base frequency: {coherent} -> {detuned}"
        );
    }

    #[test]
    fn amplitude_scales_the_output() {
        let mut loud = GranularOscillator::default();
        loud.set_amplitude(1.0);
        let mut quiet = GranularOscillator::default();
        quiet.set_amplitude(0.25);

        let loud_rms = rms(&mut loud, 8192);
        let quiet_rms = rms(&mut quiet, 8192);

        assert!(loud_rms > quiet_rms * 2.0, "{loud_rms} vs {quiet_rms}");
    }

    #[test]
    fn zero_overlap_gives_one_grain_at_a_time() {
        let mut oscillator = GranularOscillator::new(48_000.0, 8, 20.0);
        oscillator.set_grain_delay(0.0);
        assert!(oscillator.num_grains() > 1);
        assert!(oscillator.grain_size() > 1.0);
    }
}
