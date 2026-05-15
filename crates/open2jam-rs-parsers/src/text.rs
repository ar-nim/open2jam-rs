//! Text decoding utilities for the O2JM/OJN file formats.
//!
//! These files contain metadata encoded in EUC-KR (Korean) which needs
//! decoding to UTF-8 for display in the UI.

use encoding_rs::EUC_KR;

/// Decode a null-terminated byte string that may be EUC-KR or UTF-8.
///
/// Strips trailing null bytes, tries EUC-KR first (most Korean charts use this),
/// then falls back to UTF-8 lossy conversion.
pub fn decode_c_string(bytes: &[u8]) -> String {
    let trimmed = bytes.split(|&b| b == 0).next().unwrap_or(b"");
    if trimmed.is_empty() {
        return String::new();
    }
    // Try EUC-KR first (Korean charts)
    let (decoded, _encoding, had_errors) = EUC_KR.decode(trimmed);
    if !had_errors {
        let s = decoded.into_owned();
        if !s.is_empty() {
            return s;
        }
    }
    // Fallback to UTF-8
    String::from_utf8_lossy(trimmed).into_owned()
}
