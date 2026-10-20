//! RIFF/WAVE decoding.
//!
//! Handles PCM (8-bit unsigned, 16/24/32-bit signed), IEEE float (32/64-bit) and the
//! `WAVE_FORMAT_EXTENSIBLE` wrapper, which is what most modern 24-bit and multichannel files use.
//! Chunks may appear in any order and unknown chunks (LIST, fact, bext, ...) are skipped, including
//! their pad byte.

use super::{AudioBuffer, AudioLoadError};

const FORMAT_PCM: u16 = 1;
const FORMAT_FLOAT: u16 = 3;
const FORMAT_EXTENSIBLE: u16 = 0xFFFE;

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

/// Decode a RIFF/WAVE byte stream.
pub fn decode(bytes: &[u8]) -> Result<AudioBuffer, AudioLoadError> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(AudioLoadError::UnknownFormat);
    }

    let mut format: Option<u16> = None;
    let mut channels: usize = 0;
    let mut sample_rate: f32 = 0.0;
    let mut bits: u16 = 0;
    let mut data: Option<&[u8]> = None;

    let mut cursor = 12;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32_at(bytes, cursor + 4).ok_or(AudioLoadError::Truncated)? as usize;
        let body_start = cursor + 8;
        let body_end = body_start.checked_add(size).ok_or(AudioLoadError::Truncated)?;

        // A chunk may legally be the last thing in the file with its pad byte missing, so only
        // require the body itself to be present.
        if body_end > bytes.len() {
            if id == b"data" {
                return Err(AudioLoadError::Truncated);
            }
            break;
        }

        match id {
            b"fmt " => {
                let header = &bytes[body_start..body_end];
                let mut code = u16_at(header, 0).ok_or(AudioLoadError::Truncated)?;
                channels = u16_at(header, 2).ok_or(AudioLoadError::Truncated)? as usize;
                let rate = u32_at(header, 4).ok_or(AudioLoadError::Truncated)?;
                bits = u16_at(header, 14).ok_or(AudioLoadError::Truncated)?;
                sample_rate = rate as f32;

                // WAVE_FORMAT_EXTENSIBLE hides the real codec in the first two bytes of the
                // sub-format GUID, at offset 24.
                if code == FORMAT_EXTENSIBLE {
                    if header.len() < 26 {
                        return Err(AudioLoadError::Truncated);
                    }
                    code = u16_at(header, 24).ok_or(AudioLoadError::Truncated)?;
                }

                format = Some(code);
            }
            b"data" => {
                data = Some(&bytes[body_start..body_end]);
            }
            _ => {}
        }

        // Chunks are word-aligned: an odd size is followed by a pad byte.
        cursor = body_end + (size & 1);
    }

    let format = format.ok_or(AudioLoadError::MissingChunk("fmt "))?;
    let data = data.ok_or(AudioLoadError::MissingChunk("data"))?;

    if channels == 0 {
        return Err(AudioLoadError::Truncated);
    }
    if sample_rate <= 0.0 {
        return Err(AudioLoadError::Truncated);
    }

    let bytes_per_sample = match (format, bits) {
        (FORMAT_PCM, 8) => 1,
        (FORMAT_PCM, 16) => 2,
        (FORMAT_PCM, 24) => 3,
        (FORMAT_PCM, 32) => 4,
        (FORMAT_FLOAT, 32) => 4,
        (FORMAT_FLOAT, 64) => 8,
        (FORMAT_PCM | FORMAT_FLOAT, other) => {
            return Err(AudioLoadError::UnsupportedBitDepth(other))
        }
        (other, _) => return Err(AudioLoadError::UnsupportedFormat(other)),
    };

    let frame_bytes = bytes_per_sample * channels;
    let frames = data.len() / frame_bytes;

    let mut channel_data = vec![Vec::with_capacity(frames); channels];
    for frame in 0..frames {
        for (channel, output) in channel_data.iter_mut().enumerate() {
            let offset = frame * frame_bytes + channel * bytes_per_sample;
            let raw = &data[offset..offset + bytes_per_sample];
            output.push(decode_sample(raw, format, bits));
        }
    }

    Ok(AudioBuffer::from_channels(sample_rate, channel_data))
}

fn decode_sample(raw: &[u8], format: u16, bits: u16) -> f32 {
    match (format, bits) {
        // 8-bit PCM is unsigned in WAV, offset by 128.
        (FORMAT_PCM, 8) => (raw[0] as f32 - 128.0) / 128.0,
        (FORMAT_PCM, 16) => i16::from_le_bytes([raw[0], raw[1]]) as f32 / 32_768.0,
        (FORMAT_PCM, 24) => {
            // Sign-extend the 24-bit little-endian value.
            let value = (raw[0] as i32) | ((raw[1] as i32) << 8) | ((raw[2] as i32) << 16);
            let value = (value << 8) >> 8;
            value as f32 / 8_388_608.0
        }
        (FORMAT_PCM, 32) => {
            i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as f32 / 2_147_483_648.0
        }
        (FORMAT_FLOAT, 32) => f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]),
        (FORMAT_FLOAT, 64) => {
            f64::from_le_bytes([raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7]])
                as f32
        }
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 9);
        out.extend_from_slice(id);
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    fn fmt_chunk(format: u16, channels: u16, sample_rate: u32, bits: u16) -> Vec<u8> {
        let block_align = channels * (bits / 8);
        let mut body = Vec::new();
        body.extend_from_slice(&format.to_le_bytes());
        body.extend_from_slice(&channels.to_le_bytes());
        body.extend_from_slice(&sample_rate.to_le_bytes());
        body.extend_from_slice(&(sample_rate * block_align as u32).to_le_bytes());
        body.extend_from_slice(&block_align.to_le_bytes());
        body.extend_from_slice(&bits.to_le_bytes());
        if format == FORMAT_EXTENSIBLE {
            body.extend_from_slice(&22u16.to_le_bytes()); // cbSize
            body.extend_from_slice(&(bits as u16).to_le_bytes()); // valid bits
            body.extend_from_slice(&0u32.to_le_bytes()); // channel mask
            // KSDATAFORMAT_SUBTYPE_PCM
            body.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
        }
        chunk(b"fmt ", &body)
    }

    fn wav(chunks: Vec<Vec<u8>>) -> Vec<u8> {
        let mut body = Vec::from(*b"WAVE");
        for c in chunks {
            body.extend_from_slice(&c);
        }
        let mut out = Vec::from(*b"RIFF");
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn decodes_16_bit_stereo() {
        let samples: [i16; 6] = [0, 32767, -32768, 16384, -16384, 0];
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let file = wav(vec![
            fmt_chunk(FORMAT_PCM, 2, 44_100, 16),
            chunk(b"data", &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.sample_rate(), 44_100.0);
        assert_eq!(buffer.channel_count(), 2);
        assert_eq!(buffer.frames(), 3);
        assert_eq!(buffer.channel(0), Some(&[0.0, -1.0, -0.5][..]));
        assert!((buffer.channel(1).unwrap()[0] - 0.99997).abs() < 1e-4);
        assert!((buffer.channel(1).unwrap()[1] - 0.5).abs() < 1e-6);
        assert!(buffer.channel(1).unwrap()[2].abs() < 1e-6);
    }

    #[test]
    fn decodes_8_bit_as_unsigned() {
        // 128 is the 8-bit zero point.
        let file = wav(vec![
            fmt_chunk(FORMAT_PCM, 1, 22_050, 8),
            chunk(b"data", &[128, 255, 0]),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.frames(), 3);
        assert!((buffer.sample(0, 0)).abs() < 1e-6);
        assert!((buffer.sample(0, 1) - 0.9921875).abs() < 1e-6);
        assert!((buffer.sample(0, 2) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn decodes_24_bit_with_sign_extension() {
        // -1.0, 0.0, +1.0 at 24 bits, little-endian.
        let data = [0x00, 0x00, 0x80, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0x7F];
        let file = wav(vec![
            fmt_chunk(FORMAT_PCM, 1, 48_000, 24),
            chunk(b"data", &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.frames(), 3);
        assert!((buffer.sample(0, 0) + 1.0).abs() < 1e-6, "{}", buffer.sample(0, 0));
        assert!(buffer.sample(0, 1).abs() < 1e-6);
        assert!((buffer.sample(0, 2) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decodes_32_bit_float() {
        let values: [f32; 3] = [0.5, -0.25, 1.0];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        let file = wav(vec![
            fmt_chunk(FORMAT_FLOAT, 1, 96_000, 32),
            chunk(b"data", &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.sample_rate(), 96_000.0);
        assert_eq!(buffer.channel(0), Some(&values[..]));
    }

    #[test]
    fn extensible_wrapper_resolves_to_pcm() {
        let data: [u8; 4] = [0x00, 0x40, 0x00, 0xC0]; // 16384, -16384
        let file = wav(vec![
            fmt_chunk(FORMAT_EXTENSIBLE, 1, 48_000, 16),
            chunk(b"data", &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.frames(), 2);
        assert!((buffer.sample(0, 0) - 0.5).abs() < 1e-6);
        assert!((buffer.sample(0, 1) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn unknown_chunks_are_skipped_even_when_odd_sized() {
        // A 3-byte LIST chunk forces the pad byte to be handled correctly.
        let data: [u8; 2] = [0xFF, 0x7F];
        let file = wav(vec![
            chunk(b"LIST", b"abc"),
            fmt_chunk(FORMAT_PCM, 1, 48_000, 16),
            chunk(b"fact", &[4, 0, 0, 0]),
            chunk(b"data", &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.frames(), 1);
        assert!((buffer.sample(0, 0) - 0.9999695).abs() < 1e-5);
    }

    #[test]
    fn data_before_fmt_still_works() {
        let data: [u8; 2] = [0x00, 0x40];
        let file = wav(vec![
            chunk(b"data", &data),
            fmt_chunk(FORMAT_PCM, 1, 48_000, 16),
        ]);

        let buffer = super::decode(&file).unwrap();
        assert!((buffer.sample(0, 0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn compressed_codecs_are_rejected_not_guessed() {
        let file = wav(vec![
            fmt_chunk(6, 1, 8_000, 8), // A-law
            chunk(b"data", &[0, 0]),
        ]);

        assert_eq!(super::decode(&file), Err(AudioLoadError::UnsupportedFormat(6)));
    }

    #[test]
    fn unsupported_bit_depth_is_rejected() {
        let file = wav(vec![
            fmt_chunk(FORMAT_PCM, 1, 8_000, 12),
            chunk(b"data", &[0, 0]),
        ]);

        assert_eq!(
            super::decode(&file),
            Err(AudioLoadError::UnsupportedBitDepth(12))
        );
    }

    #[test]
    fn missing_chunks_are_reported() {
        let no_data = wav(vec![fmt_chunk(FORMAT_PCM, 1, 48_000, 16)]);
        assert_eq!(super::decode(&no_data), Err(AudioLoadError::MissingChunk("data")));

        let data: [u8; 2] = [0, 0];
        let no_fmt = wav(vec![chunk(b"data", &data)]);
        assert_eq!(super::decode(&no_fmt), Err(AudioLoadError::MissingChunk("fmt ")));
    }

    #[test]
    fn a_truncated_data_chunk_is_an_error() {
        let mut file = wav(vec![
            fmt_chunk(FORMAT_PCM, 1, 48_000, 16),
            chunk(b"data", &[0u8; 64]),
        ]);
        file.truncate(file.len() - 40);

        assert_eq!(super::decode(&file), Err(AudioLoadError::Truncated));
    }

    #[test]
    fn a_partial_final_frame_is_dropped() {
        // 3 bytes of 16-bit mono: one whole frame, one stray byte.
        let file = wav(vec![
            fmt_chunk(FORMAT_PCM, 1, 48_000, 16),
            chunk(b"data", &[0x00, 0x40, 0x11]),
        ]);

        let buffer = super::decode(&file).unwrap();
        assert_eq!(buffer.frames(), 1);
    }
}
