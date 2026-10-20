//! Getting audio in and out of the crate, and manipulating it as a buffer.
//!
//! [`AudioBuffer`] is the shared in-memory representation: de-interleaved `f32` channels plus a
//! sample rate. [`wav`] and [`aiff`] decode files into it, [`edit`] operates on it offline.

pub mod aiff;
pub mod edit;
pub mod wav;

use std::fmt;

/// Why a file could not be decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioLoadError {
    /// No bytes at all.
    Empty,
    /// Not a format this crate reads (missing RIFF/FORM magic).
    UnknownFormat,
    /// A header ended before it should have.
    Truncated,
    /// A container we know, carrying a codec we do not decode.
    UnsupportedFormat(u16),
    /// An AIFF compression type (`ima4`, `ulaw`, ...) we do not decode.
    UnsupportedCompression([u8; 4]),
    /// A sample width we do not decode.
    UnsupportedBitDepth(u16),
    /// A required chunk was absent.
    MissingChunk(&'static str),
    /// The file could not be read.
    Io(String),
}

impl fmt::Display for AudioLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(formatter, "no audio data"),
            Self::UnknownFormat => write!(formatter, "not a WAV or AIFF file"),
            Self::Truncated => write!(formatter, "file ends inside a header or chunk"),
            Self::UnsupportedFormat(code) => write!(formatter, "unsupported codec (format {code})"),
            Self::UnsupportedCompression(code) => write!(
                formatter,
                "unsupported compression ({})",
                String::from_utf8_lossy(code)
            ),
            Self::UnsupportedBitDepth(bits) => write!(formatter, "{bits}-bit samples are not supported"),
            Self::MissingChunk(chunk) => write!(formatter, "missing {chunk} chunk"),
            Self::Io(message) => write!(formatter, "{message}"),
        }
    }
}

impl std::error::Error for AudioLoadError {}

/// De-interleaved audio held in memory.
///
/// Every channel has the same number of frames. `f32` samples are nominally in -1.0..=1.0, but
/// nothing is clamped on load - a file can legally contain overs, and squashing them here would
/// make it impossible to tell a hot master from a clip.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    sample_rate: f32,
    channels: Vec<Vec<f32>>,
}

impl AudioBuffer {
    /// An empty buffer with `channels` channels and `frames` silent frames each.
    pub fn new(sample_rate: f32, channels: usize, frames: usize) -> Self {
        Self {
            sample_rate: sample_rate.max(1.0),
            channels: vec![vec![0.0; frames]; channels.max(1)],
        }
    }

    /// Build from per-channel data. Channels shorter than the longest are zero-padded, so the
    /// buffer's invariant (every channel is the same length) always holds.
    pub fn from_channels(sample_rate: f32, channels: Vec<Vec<f32>>) -> Self {
        let frames = channels.iter().map(Vec::len).max().unwrap_or(0);
        let channels: Vec<Vec<f32>> = channels
            .into_iter()
            .map(|mut channel| {
                channel.resize(frames, 0.0);
                channel
            })
            .collect();

        Self {
            sample_rate: sample_rate.max(1.0),
            channels: if channels.is_empty() {
                vec![Vec::new()]
            } else {
                channels
            },
        }
    }

    /// Build from interleaved frames (frame-major: L R L R ...).
    pub fn from_interleaved(sample_rate: f32, channel_count: usize, interleaved: &[f32]) -> Self {
        let channel_count = channel_count.max(1);
        let frames = interleaved.len() / channel_count;

        let mut channels = vec![vec![0.0; frames]; channel_count];
        for (index, &sample) in interleaved.iter().take(frames * channel_count).enumerate() {
            channels[index % channel_count][index / channel_count] = sample;
        }

        Self {
            sample_rate: sample_rate.max(1.0),
            channels,
        }
    }

    /// A single mono buffer.
    pub fn from_mono(sample_rate: f32, samples: Vec<f32>) -> Self {
        Self::from_channels(sample_rate, vec![samples])
    }

    /// Layout as frame-major interleaved samples.
    pub fn to_interleaved(&self) -> Vec<f32> {
        let mut output = Vec::with_capacity(self.frames() * self.channel_count());
        for frame in 0..self.frames() {
            for channel in &self.channels {
                output.push(channel[frame]);
            }
        }
        output
    }

    #[inline]
    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Frames (samples per channel).
    #[inline]
    pub fn frames(&self) -> usize {
        self.channels.first().map(Vec::len).unwrap_or(0)
    }

    #[inline]
    pub fn channel_count(&self) -> usize {
        self.channels.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.frames() == 0
    }

    pub fn duration_seconds(&self) -> f32 {
        self.frames() as f32 / self.sample_rate
    }

    #[inline]
    pub fn channels(&self) -> &[Vec<f32>] {
        &self.channels
    }

    #[inline]
    pub fn channel(&self, index: usize) -> Option<&[f32]> {
        self.channels.get(index).map(Vec::as_slice)
    }

    #[inline]
    pub fn channel_mut(&mut self, index: usize) -> Option<&mut [f32]> {
        self.channels.get_mut(index).map(Vec::as_mut_slice)
    }

    /// Out-of-range reads return silence rather than panicking, so playback cannot blow up on a
    /// buffer whose length changed under it.
    #[inline]
    pub fn sample(&self, channel: usize, frame: usize) -> f32 {
        self.channels
            .get(channel)
            .and_then(|data| data.get(frame))
            .copied()
            .unwrap_or(0.0)
    }

    #[inline]
    pub fn set_sample(&mut self, channel: usize, frame: usize, value: f32) {
        if let Some(data) = self.channels.get_mut(channel) {
            if let Some(slot) = data.get_mut(frame) {
                *slot = value;
            }
        }
    }

    /// Mix every channel down to one. Channels are summed and divided by the channel count, so a
    /// correlated stereo file does not come out 6 dB hot.
    pub fn monophonic(&self) -> Vec<f32> {
        let frames = self.frames();
        let mut output = vec![0.0f32; frames];
        if frames == 0 {
            return output;
        }

        for channel in &self.channels {
            for (slot, &value) in output.iter_mut().zip(channel) {
                *slot += value;
            }
        }

        let scale = 1.0 / self.channels.len() as f32;
        for slot in &mut output {
            *slot *= scale;
        }
        output
    }

    /// Loudest absolute sample anywhere in the buffer.
    pub fn peak(&self) -> f32 {
        self.channels
            .iter()
            .flat_map(|channel| channel.iter())
            .fold(0.0f32, |peak, sample| peak.max(sample.abs()))
    }

    /// Replace every channel with `channel_count` copies of `data`.
    pub fn set_channels(&mut self, sample_rate: f32, channel_count: usize, data: Vec<f32>) {
        let channel_count = channel_count.max(1);
        self.sample_rate = sample_rate.max(1.0);
        self.channels = vec![data; channel_count];
    }
}

/// Read and decode a WAV or AIFF file, chosen by its magic bytes.
pub fn load(path: &std::path::Path) -> Result<AudioBuffer, AudioLoadError> {
    let bytes = std::fs::read(path).map_err(|error| AudioLoadError::Io(error.to_string()))?;
    decode(&bytes)
}

/// Decode WAV or AIFF from memory, chosen by the container magic.
pub fn decode(bytes: &[u8]) -> Result<AudioBuffer, AudioLoadError> {
    if bytes.len() < 12 {
        return Err(if bytes.is_empty() {
            AudioLoadError::Empty
        } else {
            AudioLoadError::Truncated
        });
    }

    match &bytes[0..4] {
        b"RIFF" => wav::decode(bytes),
        b"FORM" => aiff::decode(bytes),
        _ => Err(AudioLoadError::UnknownFormat),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interleaved_round_trips() {
        let interleaved = [0.0, 1.0, 0.25, -0.25, 0.5, -0.5];
        let buffer = AudioBuffer::from_interleaved(48_000.0, 2, &interleaved);

        assert_eq!(buffer.channel_count(), 2);
        assert_eq!(buffer.frames(), 3);
        assert_eq!(buffer.channel(0), Some(&[0.0, 0.25, 0.5][..]));
        assert_eq!(buffer.channel(1), Some(&[1.0, -0.25, -0.5][..]));
        assert_eq!(buffer.to_interleaved(), interleaved);
    }

    #[test]
    fn uneven_channels_are_padded_to_the_longest() {
        let buffer = AudioBuffer::from_channels(44_100.0, vec![vec![1.0, 2.0, 3.0], vec![4.0]]);

        assert_eq!(buffer.frames(), 3);
        assert_eq!(buffer.channel(1), Some(&[4.0, 0.0, 0.0][..]));
    }

    #[test]
    fn out_of_range_reads_are_silent_not_panics() {
        let buffer = AudioBuffer::new(48_000.0, 2, 4);

        assert_eq!(buffer.sample(0, 99), 0.0);
        assert_eq!(buffer.sample(9, 0), 0.0);

        let mut buffer = buffer;
        buffer.set_sample(9, 9, 1.0); // must not panic
    }

    #[test]
    fn mono_mixdown_averages_rather_than_sums() {
        let buffer = AudioBuffer::from_channels(48_000.0, vec![vec![1.0, 0.0], vec![1.0, 0.0]]);
        assert_eq!(buffer.monophonic(), vec![1.0, 0.0]);
    }

    #[test]
    fn duration_and_peak() {
        let buffer = AudioBuffer::from_mono(2.0, vec![0.5, -1.5, 0.0]);
        assert_eq!(buffer.duration_seconds(), 1.5);
        assert_eq!(buffer.peak(), 1.5);
    }

    #[test]
    fn empty_input_is_reported_as_empty() {
        assert_eq!(decode(&[]), Err(AudioLoadError::Empty));
        assert_eq!(decode(&[0u8; 4]), Err(AudioLoadError::Truncated));
        assert_eq!(decode(b"XXXX0000WAVE"), Err(AudioLoadError::UnknownFormat));
    }
}
