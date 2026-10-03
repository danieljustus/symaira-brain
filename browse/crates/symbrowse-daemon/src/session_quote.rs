//! The shared Core Go formatter preserves raw rejected session bytes.
pub(crate) use symbrowse_core::go_quote::quote;

/// Formats the socket-validation diagnostic without discarding Unix argv bytes.
#[must_use]
pub fn invalid_session_message(value: &[u8]) -> String {
    format!(
        "invalid session {}: use 1-64 letters, digits, '.', '_' or '-'",
        symbrowse_core::go_quote::quote_bytes(value)
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use symbrowse_core::go_quote::quote_bytes;

    #[test]
    fn malformed_bytes_remain_distinct_from_real_replacement_scalars() {
        for (value, expected) in [
            (b"bad\xffsession".as_slice(), "\"bad\\xffsession\""),
            (b"bad\xe2\x82session".as_slice(), "\"bad\\xe2\\x82session\""),
            (b"bad\xc0\xafsession".as_slice(), "\"bad\\xc0\\xafsession\""),
            ("bad\u{fffd}session".as_bytes(), "\"bad\u{fffd}session\""),
        ] {
            assert_eq!(quote_bytes(value), expected);
        }
    }

    #[test]
    fn every_valid_unicode_scalar_matches_actual_pinned_go_quote_digest() {
        // Actual Go 1.26.7 strconv.AppendQuote for each valid scalar, each
        // followed by newline; the producing probe is retained in evidence.
        let mut digest = Sha256::new();
        for code in 0..=0x10ffff {
            if let Some(ch) = char::from_u32(code) {
                digest.update(quote(&ch.to_string()).as_bytes());
                digest.update(b"\n");
            }
        }
        assert_eq!(
            format!("{:x}", digest.finalize()),
            "4c752b4c6e90df8c641d6ac02a6da80113943bc8fc476c825f27aef51db2a055"
        );
    }
}
