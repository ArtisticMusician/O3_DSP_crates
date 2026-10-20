//! AIFF / AIFF-C decoding.
//!
//! Handles the uncompressed and float variants people actually ship: `NONE`/`twos` (big-endian
//! PCM), `sowt` (little-endian PCM), `fl32`/`FL32` and `fl64`/`FL64` (IEEE float). Compressed
//! AIFF-C (`ima4`, `ulaw`, ...) is rejected rather than guessed at.
//!
//! The one fiddly part is the sample rate: AIFF stores it as an 80-bit extended-precision float.

use super::{AudioBuffer, AudioLoadError};

const COMPRESSION_NONE: [u8; 4] = *b"NONE";
const COMPRESSION_TWOS: [u8; 4] = *b"twos";
const COMPRESSION_SOWT: [u8; 4] = *b"sowt";
const COMPRESSION_FL32: [u8; 4] = *b"fl32";
const COMPRESSION_FL32_UPPER: [u8; 4] = *b"FL32";
const COMPRESSION_FL64: [u8; 4] = *b"fl64";
const COMPRESSION_FL64_UPPER: [u8; 4] = *b"FL64";

fn u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?))
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?))
}

/// Decode a 10-byte 80-bit IEEE extended-precision value (AIFF's sample rate).
fn extended_to_f32(bytes: &[u8]) -> f32 {
    if bytes.len() < 10 {
        return 0.0;
    }

    let sign = if bytes[0] & 0x80 != 0 { -1.0f64 } else { 1.0f64 };
    let exponent = (((bytes[0] as u16 & 0x7F) << 8) | bytes[1] as u16) as i32;

    let mut mantissa: u64 = 0;
    for byte in &bytes[2..10] {
        mantissa = (mantissa << 8) | *byte as u64;
    }

    if exponent == 0 && mantissa == 0 {
        return 0.0;
    }
    if exponent == 0x7FFF {
        return if mantissa == 0 { sign as f32 * f32::INFINITY } else { f32::NAN };
    }

    // value = mantissa / 2^63 * 2^(exponent - 16383)
    let value = mantissa as f64 * 2.0f64.powi(exponent - 16_383 - 63);
    (sign * value) as f32
}

/// Decode an AIFF or AIFF-C byte stream.
pub fn decode(bytes: &[u8]) -> Result<AudioBuffer, AudioLoadError> {
    if bytes.len() < 12 || &bytes[0..4] != b"FORM" {
        return Err(AudioLoadError::UnknownFormat);
    }

    let form_type = &bytes[8..12];
    if form_type != b"AIFF" && form_type != b"AIFC" {
        return Err(AudioLoadError::UnknownFormat);
    }
    let is_aifc = form_type == b"AIFC";

    let mut channels: usize = 0;
    let mut declared_frames: usize = 0;
    let mut bits: u16 = 0;
    let mut sample_rate: f32 = 0.0;
    let mut compression: Option<[u8; 4]> = None;
    let mut sound_data: Option<&[u8]> = None;

    let mut cursor = 12;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32_at(bytes, cursor + 4).ok_or(AudioLoadError::Truncated)? as usize;
        let body_start = cursor + 8;
        let body_end = body_start.checked_add(size).ok_or(AudioLoadError::Truncated)?;

        if body_end > bytes.len() {
            if id == b"SSND" {
                return Err(AudioLoadError::Truncated);
            }
            break;
        }

        match id {
            b"COMM" => {
                let body = &bytes[body_start..body_end];
                channels = u16_at(body, 0).ok_or(AudioLoadError::Truncated)? as usize;
                declared_frames = u32_at(body, 2).ok_or(AudioLoadError::Truncated)? as usize;
                bits = u16_at(body, 6).ok_or(AudioLoadError::Truncated)?;
                sample_rate = extended_to_f32(
                    body.get(8..18).ok_or(AudioLoadError::Truncated)?,
                );
                if is_aifc {
                    let code = body.get(18..22).ok_or(AudioLoadError::Truncated)?;
                    compression = Some(code.try_into().unwrap());
                }
            }
            b"SSND" => {
                let body = &bytes[body_start..body_end];
                let offset = u32_at(body, 0).ok_or(AudioLoadError::Truncated)? as usize;
                // blockSize at offset 4 is advisory and ignored, as everywhere else.
                let start = 8usize.checked_add(offset).ok_or(AudioLoadError::Truncated)?;
                if start > body.len() {
                    return Err(AudioLoadError::Truncated);
                }
                sound_data = Some(&body[start..]);
            }
            _ => {}
        }

        cursor = body_end + (size & 1);
    }

    if channels == 0 || sample_rate <= 0.0 {
        return Err(AudioLoadError::MissingChunk("COMM"));
    }
    let sound_data = sound_data.ok_or(AudioLoadError::MissingChunk("SSND"))?;

    // What the COMM chunk claims, resolved against what is actually there.
    let compression = compression.unwrap_or(COMPRESSION_NONE);
    let (bytes_per_sample, big_endian, float) = match compression {
        COMPRESSION_NONE | COMPRESSION_TWOS => match bits {
            8 => (1usize, true, false),
            16 => (2, true, false),
            24 => (3, true, false),
            32 => (4, true, false),
            other => return Err(AudioLoadError::UnsupportedBitDepth(other)),
        },
        COMPRESSION_SOWT => match bits {
            8 => (1usize, true, false),
            16 => (2, false, false),
            24 => (3, false, false),
            32 => (4, false, false),
            other => return Err(AudioLoadError::UnsupportedBitDepth(other)),
        },
        COMPRESSION_FL32 | COMPRESSION_FL32_UPPER => (4, true, true),
        COMPRESSION_FL64 | COMPRESSION_FL64_UPPER => (8, true, true),
        other => return Err(AudioLoadError::UnsupportedCompression(other)),
    };

    let frame_bytes = bytes_per_sample * channels;
    let frames = declared_frames.min(sound_data.len() / frame_bytes);

    let mut channel_data = vec![Vec::with_capacity(frames); channels];
    for frame in 0..frames {
        for (channel, output) in channel_data.iter_mut().enumerate() {
            let offset = frame * frame_bytes + channel * bytes_per_sample;
            let raw = &sound_data[offset..offset + bytes_per_sample];
            output.push(decode_sample(raw, bytes_per_sample, big_endian, float));
        }
    }

    Ok(AudioBuffer::from_channels(sample_rate, channel_data))
}

fn decode_sample(raw: &[u8], bytes_per_sample: usize, big_endian: bool, float: bool) -> f32 {
    let ordered: [u8; 8] = if big_endian {
        let mut buffer = [0u8; 8];
        buffer[..bytes_per_sample].copy_from_slice(&raw[..bytes_per_sample]);
        buffer
    } else {
        let mut buffer = [0u8; 8];
        for (index, byte) in raw[..bytes_per_sample].iter().rev().enumerate() {
            buffer[index] = *byte;
        }
        buffer
    };

    match (bytes_per_sample, float) {
        (1, false) => ordered[0] as i8 as f32 / 128.0,
        (2, false) => i16::from_be_bytes([ordered[0], ordered[1]]) as f32 / 32_768.0,
        (3, false) => {
            let value = (ordered[0] as i32) | ((ordered[1] as i32) << 8) | ((ordered[2] as i32) << 16);
            let value = (value << 8) >> 8;
            value as f32 / 8_388_608.0
        }
        (4, false) => i32::from_be_bytes([ordered[0], ordered[1], ordered[2], ordered[3]]) as f32
            / 2_147_483_648.0,
        (4, true) => f32::from_be_bytes([ordered[0], ordered[1], ordered[2], ordered[3]]),
        (8, true) => f64::from_be_bytes(ordered) as f32,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(body.len() + 9);
        out.extend_from_slice(id);
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(body);
        if body.len() % 2 == 1 {
            out.push(0);
        }
        out
    }

    /// Encode a sample rate as an 80-bit extended float.
    fn extended_bits(value: f64) -> [u8; 10] {
        let mut out = [0u8; 10];
        if value == 0.0 {
            return out;
        }

        // normalise into 1.x
        let mut exponent = 16_383i32;
        let mut mantissa = value.abs();
        while mantissa >= 2.0 {
            mantissa /= 2.0;
            exponent += 1;
        }
        while mantissa < 1.0 {
            mantissa *= 2.0;
            exponent -= 1;
        }

        let sign = if value < 0.0 { 0x8000u16 } else { 0 };
        let bits = (sign | exponent as u16).to_be_bytes();
        out[0] = bits[0];
        out[1] = bits[1];
        let frac = (mantissa * 2f64.powi(63)) as u64;
        out[2..10].copy_from_slice(&frac.to_be_bytes());
        out
    }

    fn comm_chunk(channels: u16, frames: u32, bits: u16, rate: f64, compression: Option<[u8; 4]>) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&channels.to_be_bytes());
        body.extend_from_slice(&frames.to_be_bytes());
        body.extend_from_slice(&bits.to_be_bytes());
        body.extend_from_slice(&extended_bits(rate));
        if let Some(code) = compression {
            body.extend_from_slice(&code);
            body.extend_from_slice(&[0, 0]); // empty pstring
        }
        chunk(b"COMM", &body)
    }

    fn ssnd_chunk(offset: u32, data: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(&offset.to_be_bytes());
        body.extend_from_slice(&0u32.to_be_bytes()); // block size
        body.extend_from_slice(&[0u8; 0]);
        let mut full = vec![0u8; offset as usize];
        full.extend_from_slice(data);
        body.extend_from_slice(&full);
        chunk(b"SSND", &body)
    }

    fn aiff(form: &[u8; 4], chunks: Vec<Vec<u8>>) -> Vec<u8> {
        let mut body = Vec::from(*form);
        for c in chunks {
            body.extend_from_slice(&c);
        }
        let mut out = Vec::from(*b"FORM");
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn decodes_16_bit_big_endian() {
        let samples: [i16; 4] = [0, 16_384, -16_384, 32_767];
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_be_bytes()).collect();
        let file = aiff(b"AIFF", vec![
            comm_chunk(1, 4, 16, 44_100.0, None),
            ssnd_chunk(0, &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.sample_rate(), 44_100.0);
        assert_eq!(buffer.channel_count(), 1);
        assert_eq!(buffer.frames(), 4);
        assert!(buffer.sample(0, 0).abs() < 1e-6);
        assert!((buffer.sample(0, 1) - 0.5).abs() < 1e-6);
        assert!((buffer.sample(0, 2) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn reads_the_extended_precision_sample_rate() {
        for rate in [8_000.0, 22_050.0, 44_100.0, 48_000.0, 96_000.0] {
            let data: Vec<u8> = vec![0, 0];
            let file = aiff(b"AIFF", vec![
                comm_chunk(1, 1, 16, rate, None),
                ssnd_chunk(0, &data),
            ]);

            let buffer = super::decode(&file).unwrap();
            assert_eq!(buffer.sample_rate(), rate as f32, "sample rate {rate}");
        }
    }

    #[test]
    fn decodes_stereo_big_endian() {
        let samples: [i16; 4] = [32_767, 0, -32_767, 8_192];
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_be_bytes()).collect();
        let file = aiff(b"AIFF", vec![
            comm_chunk(2, 2, 16, 48_000.0, None),
            ssnd_chunk(0, &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert_eq!(buffer.channel_count(), 2);
        assert_eq!(buffer.frames(), 2);
        assert!((buffer.sample(0, 0) - 0.99997).abs() < 1e-4);
        assert!(buffer.sample(1, 0).abs() < 1e-4);
        assert!((buffer.sample(1, 1) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn aifc_sowt_reads_little_endian() {
        let samples: [i16; 2] = [16_384, -16_384];
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let file = aiff(b"AIFC", vec![
            comm_chunk(1, 2, 16, 44_100.0, Some(COMPRESSION_SOWT)),
            ssnd_chunk(0, &data),
        ]);

        let buffer = super::decode(&file).unwrap();

        assert!((buffer.sample(0, 0) - 0.5).abs() < 1e-6);
        assert!((buffer.sample(0, 1) + 0.5).abs() < 1e-6);
    }

    #[test]
    fn aifc_float_reads_big_endian_ieee() {
        let values: [f32; 3] = [0.25, -0.75, 1.0];
        let data: Vec<u8> = values.iter().flat_map(|v| v.to_be_bytes()).collect();
        let file = aiff(b"AIFC", vec![
            comm_chunk(1, 3, 32, 48_000.0, Some(COMPRESSION_FL32)),
            ssnd_chunk(0, &data),
        ]);

        let buffer = super::decode(&file).unwrap();
        assert_eq!(buffer.channel(0), Some(&values[..]));
    }

    #[test]
    fn the_ssnd_offset_is_honoured() {
        let data: [u8; 2] = [0x40, 0x00]; // 16384
        let file = aiff(b"AIFF", vec![
            comm_chunk(1, 1, 16, 44_100.0, None),
            ssnd_chunk(4, &data),
        ]);

        let buffer = super::decode(&file).unwrap();
        assert_eq!(buffer.frames(), 1);
        assert!((buffer.sample(0, 0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_shorter_ssnd_than_comm_claims_is_clamped_not_panicking() {
        let data: [u8; 2] = [0, 0];
        let file = aiff(b"AIFF", vec![
            comm_chunk(1, 10_000, 16, 44_100.0, None), // claims 10000 frames
            ssnd_chunk(0, &data),
        ]);

        let buffer = super::decode(&file).unwrap();
        assert_eq!(buffer.frames(), 1);
    }

    #[test]
    fn compressed_aifc_is_rejected_not_guessed() {
        let file = aiff(b"AIFC", vec![
            comm_chunk(1, 1, 16, 44_100.0, Some(*b"ima4")),
            ssnd_chunk(0, &[0, 0]),
        ]);

        assert_eq!(
            super::decode(&file),
            Err(AudioLoadError::UnsupportedCompression(*b"ima4"))
        );
    }

    #[test]
    fn missing_chunks_are_reported() {
        let no_sound = aiff(b"AIFF", vec![comm_chunk(1, 1, 16, 44_100.0, None)]);
        assert_eq!(super::decode(&no_sound), Err(AudioLoadError::MissingChunk("SSND")));

        let no_comm = aiff(b"AIFF", vec![ssnd_chunk(0, &[0, 0])]);
        assert_eq!(super::decode(&no_comm), Err(AudioLoadError::MissingChunk("COMM")));
    }

    #[test]
    fn a_non_aiff_form_is_rejected() {
        let file = aiff(b"8SVX", vec![comm_chunk(1, 1, 16, 44_100.0, None)]);
        assert_eq!(super::decode(&file), Err(AudioLoadError::UnknownFormat));
    }

    #[test]
    fn extended_bits_round_trip() {
        for value in [1.0, 2.0, 440.0, 44_100.0, 48_000.0, 192_000.0] {
            let encoded = extended_bits(value);
            assert!(
                (extended_to_f32(&encoded) - value as f32).abs() < 0.01,
                "{value} -> {}",
                extended_to_f32(&encoded)
            );
        }
        assert_eq!(extended_to_f32(&[0u8; 10]), 0.0);
    }
}
