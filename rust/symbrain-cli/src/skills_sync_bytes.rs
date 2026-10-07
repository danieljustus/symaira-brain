//! Skills sync's Go argv view, without changing other CLI codecs.

use std::ffi::{OsStr, OsString};

pub(super) fn normalize(args: &[OsString]) -> Vec<Vec<u8>> {
    let mut terminated = false;
    args.iter()
        .map(|arg| {
            let mut bytes = os_bytes(arg);
            if !terminated {
                if bytes == b"--" {
                    terminated = true;
                } else if bytes.starts_with(b"--") && bytes.len() > 2 {
                    bytes.remove(0);
                }
            }
            bytes
        })
        .collect()
}

fn os_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        wide_bytes(value.encode_wide())
    }
    #[cfg(not(any(unix, windows)))]
    value.to_string_lossy().into_owned().into_bytes()
}

// Go 1.26.7 syscall.UTF16ToString preserves lone surrogates using WTF-8.
// Decode pairs normally; retain each unpaired code unit's three-byte encoding.
#[cfg(any(windows, test))]
fn wide_bytes(wide: impl Iterator<Item = u16>) -> Vec<u8> {
    let mut bytes = Vec::new();
    for rune in char::decode_utf16(wide) {
        match rune {
            Ok(rune) => bytes.extend_from_slice(rune.encode_utf8(&mut [0; 4]).as_bytes()),
            Err(error) => {
                let unit = error.unpaired_surrogate();
                bytes.extend_from_slice(&[
                    0xe0 | (unit >> 12) as u8,
                    0x80 | ((unit >> 6) & 0x3f) as u8,
                    0x80 | (unit & 0x3f) as u8,
                ]);
            }
        }
    }
    bytes
}

pub(super) fn trim_target(mut bytes: &[u8]) -> &[u8] {
    // Decode boundary runes only. Go TrimSpace stops at invalid UTF-8,
    // including WTF-8 surrogate bytes, without replacing interior bytes.
    let whitespace = |bytes: &[u8]| {
        std::str::from_utf8(bytes).is_ok_and(|text| {
            let mut chars = text.chars();
            chars.next().is_some_and(char::is_whitespace) && chars.next().is_none()
        })
    };
    while let Some(width) = (1..=bytes.len().min(4)).find(|&n| whitespace(&bytes[..n])) {
        bytes = &bytes[width..];
    }
    while let Some(width) =
        (1..=bytes.len().min(4)).find(|&n| whitespace(&bytes[bytes.len() - n..]))
    {
        bytes = &bytes[..bytes.len() - width];
    }
    bytes
}

#[cfg(test)]
mod tests {
    use super::{trim_target, wide_bytes};

    #[test]
    fn wide_view_keeps_surrogates_distinct_from_replacement_and_pairs() {
        for (units, expected) in [
            (vec![0xd800], vec![0xed, 0xa0, 0x80]),
            (vec![0xdc00], vec![0xed, 0xb0, 0x80]),
            (vec![0xfffd], vec![0xef, 0xbf, 0xbd]),
            (vec![0xd83d, 0xde00], vec![0xf0, 0x9f, 0x98, 0x80]),
            (
                vec![0xd800, 0xd800, 0xdc00],
                vec![0xed, 0xa0, 0x80, 0xf0, 0x90, 0x80, 0x80],
            ),
        ] {
            assert_eq!(wide_bytes(units.into_iter()), expected);
        }
    }

    #[test]
    fn trim_keeps_wtf8_at_boundaries_and_inside_unicode_space() {
        assert_eq!(trim_target(b" \xed\xa0\x80 \t"), b"\xed\xa0\x80");
        assert_eq!(
            trim_target(b"\xed\xa0\x80 bad \xed\xb0\x80"),
            b"\xed\xa0\x80 bad \xed\xb0\x80"
        );
        assert_eq!(
            trim_target(b"\xc2\xa0\xed\xa0\x80\xe3\x80\x80"),
            b"\xed\xa0\x80"
        );
    }
}
