use super::fft::{Complex, Fft};
use super::Partial;
use std::f32::consts::TAU;

/// Turns a block of audio into a set of sinusoidal partials.
///
/// Hann window, magnitude spectrum, then local maxima with parabolic interpolation so the
/// frequencies are not quantised to bin centres. Amplitudes are corrected for the window's
/// coherent gain, so a partial's amplitude is in the same units as the input signal.
#[derive(Debug, Clone)]
pub struct SpectralAnalyser {
    fft: Fft,
    window: Vec<f32>,
    window_sum: f32,
    spectrum: Vec<Complex>,
    magnitudes: Vec<f32>,
    max_partials: usize,
    /// Peaks quieter than this fraction of the loudest peak are discarded.
    relative_floor: f32,
}

impl SpectralAnalyser {
    /// `frame_size` is rounded up to a power of two. `max_partials` caps how many partials a
    /// single frame can report.
    pub fn new(frame_size: usize, max_partials: usize) -> Self {
        let fft = Fft::new(frame_size);
        let size = fft.size();

        let window: Vec<f32> = (0..size)
            .map(|index| {
                let phase = TAU * index as f32 / size as f32;
                0.5 - 0.5 * phase.cos()
            })
            .collect();
        let window_sum = window.iter().sum();

        Self {
            spectrum: vec![Complex::zero(); size],
            magnitudes: vec![0.0; size / 2 + 1],
            fft,
            window,
            window_sum,
            max_partials: max_partials.max(1),
            relative_floor: 0.01,
        }
    }

    /// FFT size, in samples.
    #[inline]
    pub fn frame_size(&self) -> usize {
        self.fft.size()
    }

    /// How many partials a frame may report.
    #[inline]
    pub fn max_partials(&self) -> usize {
        self.max_partials
    }

    /// Discard peaks below `db` relative to the loudest peak in the frame.
    pub fn set_relative_floor_db(&mut self, db: f32) {
        self.relative_floor = 10.0f32.powf(db.min(0.0) / 20.0);
    }

    /// Analyse one frame. `frame` must be at least [`Self::frame_size`] samples; anything
    /// shorter is analysed as if zero-padded.
    ///
    /// Returns the partials sorted by frequency, loudest-first selection already applied.
    pub fn analyse(&mut self, frame: &[f32], sample_rate: f32) -> Vec<Partial> {
        let size = self.fft.size();

        for index in 0..size {
            let sample = frame.get(index).copied().unwrap_or(0.0);
            self.spectrum[index] = Complex::new(sample * self.window[index], 0.0);
        }

        // `spectrum` is already sized to `size` by construction.
        let mut spectrum = std::mem::take(&mut self.spectrum);
        self.fft.forward(&mut spectrum);

        let half = size / 2;
        for bin in 0..=half {
            self.magnitudes[bin] = spectrum[bin].magnitude();
        }

        let loudest = self.magnitudes[..=half]
            .iter()
            .copied()
            .fold(0.0f32, f32::max);

        if loudest <= f32::MIN_POSITIVE {
            self.spectrum = spectrum;
            return Vec::new();
        }

        let floor = loudest * self.relative_floor;

        // Local maxima, ignoring bin 0 and the Nyquist bin (both are half-width peaks).
        let mut peaks: Vec<(f32, usize)> = Vec::new();
        for bin in 1..half {
            let magnitude = self.magnitudes[bin];
            if magnitude >= floor
                && magnitude > self.magnitudes[bin - 1]
                && magnitude >= self.magnitudes[bin + 1]
            {
                peaks.push((magnitude, bin));
            }
        }

        // Loudest first, then trim.
        peaks.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        peaks.truncate(self.max_partials);

        let bin_hz = sample_rate / size as f32;
        let amplitude_scale = 2.0 / self.window_sum;

        let mut partials: Vec<Partial> = peaks
            .into_iter()
            .map(|(_, bin)| {
                let (left, centre, right) = (
                    self.magnitudes[bin - 1],
                    self.magnitudes[bin],
                    self.magnitudes[bin + 1],
                );

                // Parabolic interpolation: where the true peak sits between bins.
                let denominator = left - 2.0 * centre + right;
                let offset = if denominator.abs() > 1e-12 {
                    (0.5 * (left - right) / denominator).clamp(-0.5, 0.5)
                } else {
                    0.0
                };

                let peak_magnitude = centre - 0.25 * (left - right) * offset;
                let frequency = (bin as f32 + offset) * bin_hz;
                let phase = spectrum[bin].im.atan2(spectrum[bin].re);

                Partial::new(frequency, peak_magnitude * amplitude_scale, phase)
            })
            .collect();

        partials.sort_by(|a, b| {
            a.frequency
                .partial_cmp(&b.frequency)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        self.spectrum = spectrum;
        partials
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RATE: f32 = 48_000.0;

    fn sine(frequency: f32, amplitude: f32, samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|index| {
                (TAU * frequency * index as f32 / SAMPLE_RATE).sin() * amplitude
            })
            .collect()
    }

    #[test]
    fn frame_size_is_rounded_to_a_power_of_two() {
        assert_eq!(SpectralAnalyser::new(2048, 64).frame_size(), 2048);
        assert_eq!(SpectralAnalyser::new(1000, 64).frame_size(), 1024);
    }

    #[test]
    fn finds_a_sine_with_the_right_frequency_and_amplitude() {
        let mut analyser = SpectralAnalyser::new(2048, 64);
        let frame = sine(1000.0, 0.5, 2048);

        let partials = analyser.analyse(&frame, SAMPLE_RATE);

        assert!(!partials.is_empty(), "no partial found for a 1 kHz sine");
        let loudest = partials
            .iter()
            .max_by(|a, b| a.amplitude.partial_cmp(&b.amplitude).unwrap())
            .unwrap();

        assert!(
            (loudest.frequency - 1000.0).abs() < 10.0,
            "frequency {} is not within 1% of 1000 Hz",
            loudest.frequency
        );
        assert!(
            (loudest.amplitude - 0.5).abs() < 0.05,
            "amplitude {} should be ~0.5",
            loudest.amplitude
        );
    }

    #[test]
    fn a_clean_sine_reports_a_single_partial() {
        let mut analyser = SpectralAnalyser::new(2048, 64);
        let frame = sine(440.0, 0.8, 2048);

        let partials = analyser.analyse(&frame, SAMPLE_RATE);

        assert_eq!(
            partials.len(),
            1,
            "expected one peak, got {:?}",
            partials
        );
    }

    #[test]
    fn separates_two_tones() {
        let mut analyser = SpectralAnalyser::new(2048, 64);

        let frame: Vec<f32> = sine(500.0, 0.5, 2048)
            .iter()
            .zip(sine(1500.0, 0.25, 2048))
            .map(|(a, b)| a + b)
            .collect();

        let partials = analyser.analyse(&frame, SAMPLE_RATE);

        assert_eq!(partials.len(), 2, "expected two peaks, got {partials:?}");
        assert!((partials[0].frequency - 500.0).abs() < 10.0, "{:?}", partials[0]);
        assert!((partials[1].frequency - 1500.0).abs() < 15.0, "{:?}", partials[1]);
        // Sorted by frequency, and the 500 Hz tone is the louder one.
        assert!(partials[0].amplitude > partials[1].amplitude);
    }

    #[test]
    fn silence_reports_no_partials() {
        let mut analyser = SpectralAnalyser::new(2048, 64);
        let partials = analyser.analyse(&[0.0; 2048], SAMPLE_RATE);
        assert!(partials.is_empty(), "silence produced {partials:?}");
    }

    #[test]
    fn a_short_frame_is_zero_padded_rather_than_panicking() {
        let mut analyser = SpectralAnalyser::new(2048, 64);
        let partials = analyser.analyse(&sine(1000.0, 0.5, 256), SAMPLE_RATE);
        assert!(partials.len() <= 1);
    }

    #[test]
    fn partial_cap_is_respected() {
        let mut analyser = SpectralAnalyser::new(4096, 3);

        // A bunch of separated tones.
        let mut frame = vec![0.0f32; 4096];
        for harmonic in 1..=8 {
            for (slot, value) in frame.iter_mut().zip(sine(harmonic as f32 * 300.0, 0.3, 4096)) {
                *slot += value;
            }
        }

        let partials = analyser.analyse(&frame, SAMPLE_RATE);
        assert_eq!(partials.len(), 3, "cap of 3 not respected: {partials:?}");
    }

    #[test]
    fn the_floor_discards_quiet_peaks() {
        let mut analyser = SpectralAnalyser::new(2048, 64);
        analyser.set_relative_floor_db(-60.0);

        // 1 kHz at 0.5 plus a 3 kHz tone well below the -60 dB floor.
        let frame: Vec<f32> = sine(1000.0, 0.5, 2048)
            .iter()
            .zip(sine(3000.0, 0.0002, 2048))
            .map(|(a, b)| a + b)
            .collect();

        let partials = analyser.analyse(&frame, SAMPLE_RATE);
        assert!(
            partials.iter().all(|p| (p.frequency - 3000.0).abs() > 50.0),
            "the -60 dB tone should have been discarded: {partials:?}"
        );
    }

    #[test]
    fn frequencies_are_not_quantised_to_bin_centres() {
        let mut analyser = SpectralAnalyser::new(2048, 64);
        // 1013.5 Hz sits deliberately between bins (bin spacing is 23.4 Hz).
        let frame = sine(1013.5, 0.5, 2048);

        let partials = analyser.analyse(&frame, SAMPLE_RATE);
        let loudest = partials
            .iter()
            .max_by(|a, b| a.amplitude.partial_cmp(&b.amplitude).unwrap())
            .unwrap();

        assert!(
            (loudest.frequency - 1013.5).abs() < 2.0,
            "parabolic interpolation should land near 1013.5 Hz, got {}",
            loudest.frequency
        );
    }
}
