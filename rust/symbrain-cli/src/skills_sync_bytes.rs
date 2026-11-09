//! Skills sync's Go argv view, without changing other CLI codecs.

use std::ffi::{OsStr, OsString};

pub(crate) fn normalize(args: &[OsString]) -> Vec<Vec<u8>> {
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
    {
        use std::os::unix::ffi::OsStrExt;
        value.as_bytes().to_vec()
    }
}

pub(crate) fn trim_target(mut bytes: &[u8]) -> &[u8] {
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
    use super::trim_target;

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
