//! Lowercase hexadecimal encoding for digests.
//!
//! `sha2` 0.11 returns `hybrid_array::Array` outputs that no longer implement
//! `LowerHex`, so digest call sites encode through this helper. The output is
//! byte-identical to the former `{:x}` formatting of the digest.

use std::fmt::Write as _;

/// Encodes `bytes` as lowercase hexadecimal, two characters per byte.
#[must_use]
pub fn lower(bytes: impl AsRef<[u8]>) -> String {
    let bytes = bytes.as_ref();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // Writing to a `String` cannot fail.
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::lower;

    #[test]
    fn encodes_lowercase_with_zero_padding() {
        assert_eq!(lower([]), "");
        assert_eq!(lower([0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
        assert_eq!(lower(b"abc"), "616263");
    }
}
