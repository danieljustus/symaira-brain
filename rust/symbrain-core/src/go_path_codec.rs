//! Go1.26 syscall WTF-8 preserves every Windows UTF-16 path unit.
use std::ffi::{OsStr, OsString};

/// Reads raw Unix bytes or Go-compatible Windows WTF-8 without replacement.
#[must_use]
pub fn os_bytes(value: &OsStr) -> Vec<u8> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        encode_wide(&value.encode_wide().collect::<Vec<_>>())
    }
    #[cfg(not(any(unix, windows)))]
    {
        value.as_encoded_bytes().to_vec()
    }
}

/// Converts Go string bytes back to filesystem-native units.
#[must_use]
pub fn from_bytes(bytes: &[u8]) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes.to_vec())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStringExt;
        OsString::from_wide(&decode_wide(bytes))
    }
    #[cfg(not(any(unix, windows)))]
    {
        OsString::from(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(any(windows, test))]
fn encode_wide(units: &[u16]) -> Vec<u8> {
    let mut result = Vec::new();
    for decoded in std::char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(ch) => {
                let mut buf = [0; 4];
                result.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
            }
            Err(error) => {
                let value = error.unpaired_surrogate();
                result.extend_from_slice(&[
                    0xed,
                    0x80 | u8::try_from((value >> 6) & 0x3f).expect("six masked bits"),
                    0x80 | u8::try_from(value & 0x3f).expect("six masked bits"),
                ]);
            }
        }
    }
    result
}

#[cfg(any(windows, test))]
fn decode_wide(bytes: &[u8]) -> Vec<u16> {
    let mut result = Vec::new();
    let mut remaining = bytes;
    while !remaining.is_empty() {
        if remaining.len() >= 3
            && remaining[0] == 0xed
            && (0xa0..=0xbf).contains(&remaining[1])
            && (0x80..=0xbf).contains(&remaining[2])
        {
            result.push(
                (u16::from(remaining[0] & 0x0f) << 12)
                    | (u16::from(remaining[1] & 0x3f) << 6)
                    | u16::from(remaining[2] & 0x3f),
            );
            remaining = &remaining[3..];
            continue;
        }
        let width = match remaining[0] {
            0..=0x7f => 1,
            0xc2..=0xdf => 2,
            0xe0..=0xef => 3,
            0xf0..=0xf4 => 4,
            _ => 0,
        };
        if width > 0
            && remaining.len() >= width
            && let Ok(text) = std::str::from_utf8(&remaining[..width])
        {
            result.extend(text.encode_utf16());
            remaining = &remaining[width..];
            continue;
        }
        result.push(0xfffd);
        remaining = &remaining[1..];
    }
    result
}

#[cfg(test)]
mod tests {
    use super::{decode_wide, encode_wide};
    #[test]
    fn sdk_wtf8_keeps_lone_surrogates_and_valid_pairs_distinct() {
        for units in [
            vec![0xd800],
            vec![0xdc00],
            vec![0xd800, 0xdc00],
            vec![0x61, 0xd800, 0x2f, 0xdc00, 0xfffd],
        ] {
            assert_eq!(decode_wide(&encode_wide(&units)), units);
        }
        assert_eq!(encode_wide(&[0xd800]), [0xed, 0xa0, 0x80]);
        assert_eq!(decode_wide(&[0xff, 0xed, 0xa0, 0x80]), [0xfffd, 0xd800]);
    }
}
