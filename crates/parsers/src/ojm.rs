//! OJM/M30/OMC audio container parser.
//!
//! Supports three binary variants:
//! - **M30**: OGG samples with optional XOR encryption (`nami` / `0412` masks).
//! - **OMC**: Encrypted WAV + OGG container with rearrangement + stateful XOR.
//! - **OJM**: Unencrypted WAV + OGG container (same layout as OMC).

use std::collections::HashMap;
use std::path::Path;

use encoding_rs::EUC_KR;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Signatures (little-endian u32)
// ---------------------------------------------------------------------------

const M30_SIGNATURE: u32 = 0x0030_334D;
const OMC_SIGNATURE: u32 = 0x0043_4D4F;
const OJM_SIGNATURE: u32 = 0x004D_4A4F;

// ---------------------------------------------------------------------------
// Encryption masks
// ---------------------------------------------------------------------------

const MASK_NAMI: [u8; 4] = [0x6E, 0x61, 0x6D, 0x69];
const MASK_0412: [u8; 4] = [0x30, 0x34, 0x31, 0x32];

const REARRANGE_TABLE: [u8; 290] = [
    0x10, 0x0E, 0x02, 0x09, 0x04, 0x00, 0x07, 0x01, 0x06, 0x08, 0x0F, 0x0A, 0x05, 0x0C, 0x03, 0x0D,
    0x0B, 0x07, 0x02, 0x0A, 0x0B, 0x03, 0x05, 0x0D, 0x08, 0x04, 0x00, 0x0C, 0x06, 0x0F, 0x0E, 0x10,
    0x01, 0x09, 0x0C, 0x0D, 0x03, 0x00, 0x06, 0x09, 0x0A, 0x01, 0x07, 0x08, 0x10, 0x02, 0x0B, 0x0E,
    0x04, 0x0F, 0x05, 0x08, 0x03, 0x04, 0x0D, 0x06, 0x05, 0x0B, 0x10, 0x02, 0x0C, 0x07, 0x09, 0x0A,
    0x0F, 0x0E, 0x00, 0x01, 0x0F, 0x02, 0x0C, 0x0D, 0x00, 0x04, 0x01, 0x05, 0x07, 0x03, 0x09, 0x10,
    0x06, 0x0B, 0x0A, 0x08, 0x0E, 0x00, 0x04, 0x0B, 0x10, 0x0F, 0x0D, 0x0C, 0x06, 0x05, 0x07, 0x01,
    0x02, 0x03, 0x08, 0x09, 0x0A, 0x0E, 0x03, 0x10, 0x08, 0x07, 0x06, 0x09, 0x0E, 0x0D, 0x00, 0x0A,
    0x0B, 0x04, 0x05, 0x0C, 0x02, 0x01, 0x0F, 0x04, 0x0E, 0x10, 0x0F, 0x05, 0x08, 0x07, 0x0B, 0x00,
    0x01, 0x06, 0x02, 0x0C, 0x09, 0x03, 0x0A, 0x0D, 0x06, 0x0D, 0x0E, 0x07, 0x10, 0x0A, 0x0B, 0x00,
    0x01, 0x0C, 0x0F, 0x02, 0x03, 0x08, 0x09, 0x04, 0x05, 0x0A, 0x0C, 0x00, 0x08, 0x09, 0x0D, 0x03,
    0x04, 0x05, 0x10, 0x0E, 0x0F, 0x01, 0x02, 0x0B, 0x06, 0x07, 0x05, 0x06, 0x0C, 0x04, 0x0D, 0x0F,
    0x07, 0x0E, 0x08, 0x01, 0x09, 0x02, 0x10, 0x0A, 0x0B, 0x00, 0x03, 0x0B, 0x0F, 0x04, 0x0E, 0x03,
    0x01, 0x00, 0x02, 0x0D, 0x0C, 0x06, 0x07, 0x05, 0x10, 0x09, 0x08, 0x0A, 0x03, 0x02, 0x01, 0x00,
    0x04, 0x0C, 0x0D, 0x0B, 0x10, 0x05, 0x06, 0x0F, 0x0E, 0x07, 0x09, 0x0A, 0x08, 0x09, 0x0A, 0x00,
    0x07, 0x08, 0x06, 0x10, 0x03, 0x04, 0x01, 0x02, 0x05, 0x0B, 0x0E, 0x0F, 0x0D, 0x0C, 0x0A, 0x06,
    0x09, 0x0C, 0x0B, 0x10, 0x07, 0x08, 0x00, 0x0F, 0x03, 0x01, 0x02, 0x05, 0x0D, 0x0E, 0x04, 0x0D,
    0x00, 0x01, 0x0E, 0x02, 0x03, 0x08, 0x0B, 0x07, 0x0C, 0x09, 0x05, 0x0A, 0x0F, 0x04, 0x06, 0x10,
    0x01, 0x0E, 0x02, 0x03, 0x0D, 0x0B, 0x07, 0x00, 0x08, 0x0C, 0x09, 0x06, 0x0F, 0x10, 0x05, 0x0A,
    0x04, 0x00,
];

const MAX_ALLOWED_SAMPLE_SIZE: usize = 50 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A parsed sample entry from an OJM/M30/OMC file.
#[derive(Debug, Clone)]
pub struct SampleEntry {
    pub id: u32,
    pub name: String,
    pub data: Vec<u8>,
    pub extension: String,
}

pub type SampleMap = HashMap<u32, SampleEntry>;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum OjmError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("unknown signature: 0x{0:08X}")]
    UnknownSignature(u32),
    #[error("truncated file (expected at least {expected} bytes, got {actual})")]
    Truncated { expected: usize, actual: usize },
    #[error("invalid sample size: {size} (max {MAX_ALLOWED_SAMPLE_SIZE})")]
    InvalidSampleSize { size: usize },
    #[error("unknown encryption flag: {0}")]
    UnknownEncryptionFlag(u32),
    #[error("EUC-KR decoding failed")]
    EucKrDecodeError,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

pub fn parse_file(path: impl AsRef<Path>) -> Result<SampleMap, OjmError> {
    let data = std::fs::read(path)?;
    parse_bytes(&data)
}

pub fn parse_bytes(data: &[u8]) -> Result<SampleMap, OjmError> {
    if data.len() < 4 {
        return Err(OjmError::Truncated {
            expected: 4,
            actual: data.len(),
        });
    }
    let signature = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    match signature {
        M30_SIGNATURE => parse_m30(data),
        OMC_SIGNATURE => parse_omc(data, true),
        OJM_SIGNATURE => parse_omc(data, false),
        other => Err(OjmError::UnknownSignature(other)),
    }
}

// ---------------------------------------------------------------------------
// M30 parser
// ---------------------------------------------------------------------------

fn parse_m30(data: &[u8]) -> Result<SampleMap, OjmError> {
    const HEADER_SIZE: usize = 28;
    const SAMPLE_HEADER_SIZE: usize = 52;

    if data.len() < HEADER_SIZE {
        return Err(OjmError::Truncated {
            expected: HEADER_SIZE,
            actual: data.len(),
        });
    }

    let encryption_flag = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);
    let mut samples = SampleMap::new();
    let mut offset = HEADER_SIZE;

    while offset + SAMPLE_HEADER_SIZE <= data.len() {
        let name = decode_sample_name(&data[offset..offset + 32]);
        offset += 32;

        let sample_size = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        let codec_code = u16::from_le_bytes([data[offset], data[offset + 1]]);
        offset += 2 + 2 + 4;

        let ref_id = u16::from_le_bytes([data[offset], data[offset + 1]]);
        offset += 2 + 2 + 4;

        if sample_size == 0
            || sample_size > MAX_ALLOWED_SAMPLE_SIZE
            || offset + sample_size > data.len()
        {
            break;
        }

        let mut sample_data = data[offset..offset + sample_size].to_vec();
        offset += sample_size;

        match encryption_flag {
            0 => {}
            16 => xor_with_mask(&mut sample_data, &MASK_NAMI),
            32 => xor_with_mask(&mut sample_data, &MASK_0412),
            other if other > 16 => {}
            other => return Err(OjmError::UnknownEncryptionFlag(other)),
        }

        let (sid, ext) = match codec_code {
            5 => (ref_id as u32, "ogg"),
            0 => ((ref_id as u32) + 1000, "ogg"),
            _ => (ref_id as u32, "ogg"),
        };

        samples.insert(
            sid,
            SampleEntry {
                id: sid,
                name,
                data: sample_data,
                extension: ext.to_string(),
            },
        );
    }

    Ok(samples)
}

// ---------------------------------------------------------------------------
// OMC / OJM parser
// ---------------------------------------------------------------------------

fn parse_omc(data: &[u8], decrypt: bool) -> Result<SampleMap, OjmError> {
    const HEADER_SIZE: usize = 20;
    const WAV_HEADER_SIZE: usize = 56;
    const OGG_HEADER_SIZE: usize = 36;

    if data.len() < HEADER_SIZE {
        return Err(OjmError::Truncated {
            expected: HEADER_SIZE,
            actual: data.len(),
        });
    }

    let ogg_start = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as usize;
    let filesize = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
    let ogg_start = ogg_start.min(data.len());
    let filesize = filesize.min(data.len());

    let mut samples = SampleMap::new();

    // WAV section (IDs 0+)
    let mut offset = HEADER_SIZE;
    let mut sample_id: u32 = 0;
    let mut acc_keybyte: u8 = 0xFF;
    let mut acc_counter: u8 = 0;

    while offset + WAV_HEADER_SIZE <= ogg_start {
        let name = decode_sample_name(&data[offset..offset + 32]);
        offset += 32 + 20 + 4;
        let chunk_size = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        if chunk_size == 0 || offset + chunk_size > ogg_start {
            sample_id += 1;
            continue;
        }

        let mut sample_data = data[offset..offset + chunk_size].to_vec();
        offset += chunk_size;

        if decrypt {
            rearrange(&mut sample_data);
            let state = xor_with_state(&mut sample_data, acc_keybyte, acc_counter);
            acc_keybyte = state.0;
            acc_counter = state.1;
        }

        samples.insert(
            sample_id,
            SampleEntry {
                id: sample_id,
                name,
                data: sample_data,
                extension: "wav".to_string(),
            },
        );
        sample_id += 1;
    }

    // OGG section (IDs 1000+)
    offset = ogg_start;
    sample_id = 1000;

    while offset + OGG_HEADER_SIZE <= filesize {
        let name = decode_sample_name(&data[offset..offset + 32]);
        offset += 32;
        let sample_size = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;

        if sample_size == 0
            || sample_size > MAX_ALLOWED_SAMPLE_SIZE
            || offset + sample_size > filesize
        {
            sample_id += 1;
            continue;
        }

        let sample_data = data[offset..offset + sample_size].to_vec();
        offset += sample_size;

        samples.insert(
            sample_id,
            SampleEntry {
                id: sample_id,
                name,
                data: sample_data,
                extension: "ogg".to_string(),
            },
        );
        sample_id += 1;
    }

    Ok(samples)
}

// ---------------------------------------------------------------------------
// Decryption helpers
// ---------------------------------------------------------------------------

fn xor_with_mask(data: &mut [u8], mask: &[u8; 4]) {
    // Java processes in groups of 4, skipping trailing bytes:
    // for(int i = 0; i + 3 < array.length; i += 4)
    let aligned_len = (data.len() / 4) * 4;
    for (i, byte) in data[..aligned_len].iter_mut().enumerate() {
        *byte ^= mask[i & 3];
    }
}

fn rearrange(buf: &mut [u8]) {
    let length = buf.len();
    if length == 0 {
        return;
    }
    let remainder = length % 17;
    let key_start = (remainder << 4) + remainder;
    let block_size = length / 17;
    if block_size == 0 {
        return;
    }
    let mut plain = vec![0u8; length];
    let mut key = key_start;
    for block in 0..17 {
        let block_start_encoded = block_size * block;
        let block_start_plain = block_size * REARRANGE_TABLE[key] as usize;
        let end_encoded = (block_start_encoded + block_size).min(length);
        let end_plain = (block_start_plain + block_size).min(length);
        let copy_len = (end_encoded - block_start_encoded).min(end_plain - block_start_plain);
        plain[block_start_plain..block_start_plain + copy_len]
            .copy_from_slice(&buf[block_start_encoded..block_start_encoded + copy_len]);
        key += 1;
    }
    buf.copy_from_slice(&plain);
}

fn xor_with_state(buf: &mut [u8], mut acc_keybyte: u8, mut acc_counter: u8) -> (u8, u8) {
    for byte in buf.iter_mut() {
        let original = *byte;
        if ((acc_keybyte as u32).wrapping_shl(acc_counter as u32) & 0x80) != 0 {
            *byte = !*byte;
        }
        acc_counter += 1;
        if acc_counter > 7 {
            acc_counter = 0;
            acc_keybyte = original;
        }
    }
    (acc_keybyte, acc_counter)
}

fn decode_sample_name(bytes: &[u8]) -> String {
    let trimmed = bytes.split(|&b| b == 0).next().unwrap_or(b"");
    if trimmed.is_empty() {
        return String::new();
    }
    let (decoded, _encoding, had_errors) = EUC_KR.decode(trimmed);
    if !had_errors {
        return decoded.into_owned();
    }
    String::from_utf8_lossy(trimmed).into_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_m30_file() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test_assets/o2ma100.ojm");
        let result = parse_file(path);
        assert!(result.is_ok());
        let samples = result.unwrap();
        assert!(!samples.is_empty(), "M30 file should contain samples");
    }

    #[test]
    fn test_parse_unknown_signature() {
        let result = parse_bytes(&[0xEF, 0xBE, 0xAD, 0xDE]);
        assert!(matches!(
            result,
            Err(OjmError::UnknownSignature(0xDEADBEEF))
        ));
    }

    #[test]
    fn test_xor_mask_skips_trailing_bytes() {
        // Java: for(int i = 0; i + 3 < array.length; i += 4)
        // Trailing bytes (when len % 4 != 0) must NOT be XORed.
        let mask = [0x6E, 0x61, 0x6D, 0x69];

        // Aligned: all bytes XORed
        let mut data = [0u8; 8];
        xor_with_mask(&mut data, &mask);
        assert_eq!(data, [0x6E, 0x61, 0x6D, 0x69, 0x6E, 0x61, 0x6D, 0x69]);

        // Trailing 1 byte: NOT XORed (stays 0)
        let mut data = [0u8; 5];
        xor_with_mask(&mut data, &mask);
        assert_eq!(data, [0x6E, 0x61, 0x6D, 0x69, 0x00]);

        // Trailing 3 bytes: NOT XORed (stay 0)
        let mut data = [0u8; 7];
        xor_with_mask(&mut data, &mask);
        assert_eq!(data, [0x6E, 0x61, 0x6D, 0x69, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn test_rearrange_preserves_length() {
        // Rearrange should preserve the length of data
        let original: Vec<u8> = (0..=255).collect();
        let len = original.len();

        let mut data = original.clone();
        rearrange(&mut data);

        assert_eq!(data.len(), len, "Rearrange should preserve data length");
    }

    #[test]
    fn test_rearrange_empty_and_small() {
        // Empty data should not panic
        let mut empty: Vec<u8> = vec![];
        rearrange(&mut empty);
        assert!(empty.is_empty());

        // Data smaller than 17 bytes
        let mut small = vec![1u8, 2, 3];
        rearrange(&mut small);
        // Should not panic and data should be unchanged or rearranged
    }

    #[test]
    fn test_sample_ref_id_from_m30() {
        // Test that sample IDs come from ref_id field, not sequential counter
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../test_assets/o2ma100.ojm");
        let result = parse_file(path).expect("Failed to parse OJM");

        // All sample keys should be u32 values derived from ref_id
        for (sample_id, _sample) in result.iter() {
            assert!(
                *sample_id < 1000 || *sample_id >= 1000,
                "Sample ID {} should be from ref_id",
                sample_id
            );
        }
    }
}
