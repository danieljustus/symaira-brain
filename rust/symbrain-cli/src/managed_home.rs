//! The managed owner boundary follows Go filepath.Join/Clean, never symlinks.
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

const HOME_VARIABLE: &str = "HOME";

pub(crate) fn bin_dir() -> Option<PathBuf> {
    let home = std::env::var_os(HOME_VARIABLE).filter(|home| !home.is_empty())?;
    Some(managed_path(&home))
}

fn managed_path(home: &std::ffi::OsStr) -> PathBuf {
    let mut joined = symbrain_core::config::os_bytes(home).into_owned();
    for part in [b".symaira".as_slice(), b"bin".as_slice()] {
        if joined.last() != Some(&b'/') {
            joined.push(b'/');
        }
        joined.extend_from_slice(part);
    }
    let bytes = clean(&joined);
    PathBuf::from(OsString::from_vec(bytes))
}

fn clean(original: &[u8]) -> Vec<u8> {
    let rooted = original.first() == Some(&b'/');
    let mut parts: Vec<&[u8]> = Vec::new();
    for part in original.split(|byte| *byte == b'/') {
        match part {
            b"" | b"." => {}
            b".." if parts.last().is_some_and(|last| *last != b"..") => {
                parts.pop();
            }
            b".." if rooted => {}
            _ => parts.push(part),
        }
    }
    let mut result = Vec::new();
    if rooted {
        result.push(b'/');
    }
    for part in parts {
        if !result.is_empty() && result.last() != Some(&b'/') {
            result.push(b'/');
        }
        result.extend_from_slice(part);
    }
    if result.is_empty() {
        result.push(b'.');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::clean;

    #[test]
    fn lexical_clean_preserves_unix_bytes_and_relative_parents() {
        for (input, expected) in [
            (
                b"/root/link/../home//./.symaira/bin".as_slice(),
                b"/root/home/.symaira/bin".as_slice(),
            ),
            (
                b"../../home/../raw\xff\xe2\x82/.symaira/bin",
                b"../../raw\xff\xe2\x82/.symaira/bin",
            ),
            (b"/../../home/.symaira/bin", b"/home/.symaira/bin"),
        ] {
            assert_eq!(clean(input), expected);
        }
    }
}
