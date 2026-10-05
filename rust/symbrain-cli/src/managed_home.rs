//! The managed owner boundary follows Go filepath.Join/Clean, never symlinks.
use std::ffi::OsString;
use std::path::PathBuf;

#[cfg(windows)]
const HOME_VARIABLE: &str = "USERPROFILE";
#[cfg(not(windows))]
const HOME_VARIABLE: &str = "HOME";

pub(crate) fn bin_dir() -> Option<PathBuf> {
    let home = std::env::var_os(HOME_VARIABLE).filter(|home| !home.is_empty())?;
    Some(managed_path(&home))
}

fn managed_path(home: &std::ffi::OsStr) -> PathBuf {
    let mut joined = symbrain_core::config::os_bytes(home).into_owned();
    let windows = cfg!(windows);
    let separator = if windows { b'\\' } else { b'/' };
    for part in [b".symaira".as_slice(), b"bin".as_slice()] {
        if !(joined
            .last()
            .is_some_and(|byte| is_separator(*byte, windows))
            || (windows && joined.last() == Some(&b':')))
        {
            joined.push(separator);
        }
        joined.extend_from_slice(part);
    }
    let bytes = clean(&joined, windows);
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes)
    };
    #[cfg(not(unix))]
    // Windows Go environment strings also decode UTF-16 into valid UTF-8.
    // Cleaning only removes ASCII path syntax, never splits a Unicode scalar.
    let path = OsString::from(String::from_utf8(bytes).expect("valid Windows home UTF-8"));
    PathBuf::from(path)
}

fn is_separator(byte: u8, windows: bool) -> bool {
    byte == b'/' || (windows && byte == b'\\')
}

fn prefix_fold(path: &[u8], prefix: &[u8]) -> bool {
    path.len() >= prefix.len()
        && path.iter().zip(prefix).all(|(a, b)| {
            if is_separator(*b, true) {
                is_separator(*a, true)
            } else {
                a.eq_ignore_ascii_case(b)
            }
        })
        && (path.len() == prefix.len() || is_separator(path[prefix.len()], true))
}

fn unc_len(path: &[u8], start: usize) -> usize {
    path.iter()
        .enumerate()
        .skip(start)
        .filter(|(_, byte)| is_separator(**byte, true))
        .nth(1)
        .map_or(path.len(), |(index, _)| index)
}

fn volume_len(path: &[u8]) -> usize {
    if path.get(1) == Some(&b':') {
        2
    } else if !path.first().is_some_and(|byte| is_separator(*byte, true)) {
        0
    } else if prefix_fold(path, br"\\.\UNC") {
        unc_len(path, 8)
    } else if [br"\\.".as_slice(), br"\\?".as_slice(), br"\??".as_slice()]
        .iter()
        .any(|prefix| prefix_fold(path, prefix))
    {
        if path.len() == 3 {
            3
        } else {
            path[4..]
                .iter()
                .position(|byte| is_separator(*byte, true))
                .map_or(path.len(), |index| index + 4)
        }
    } else if path.get(1).is_some_and(|byte| is_separator(*byte, true)) {
        unc_len(path, 2)
    } else {
        0
    }
}

fn clean(original: &[u8], windows: bool) -> Vec<u8> {
    let volume = if windows { volume_len(original) } else { 0 };
    let path = &original[volume..];
    let separator = if windows { b'\\' } else { b'/' };
    let rooted = path
        .first()
        .is_some_and(|byte| is_separator(*byte, windows));
    let mut parts: Vec<&[u8]> = Vec::new();
    for part in path.split(|byte| is_separator(*byte, windows)) {
        match part {
            b"" | b"." => {}
            b".." if parts.last().is_some_and(|last| *last != b"..") => {
                parts.pop();
            }
            b".." if rooted => {}
            _ => parts.push(part),
        }
    }
    let mut tail = Vec::new();
    if rooted {
        tail.push(separator);
    }
    for part in parts {
        if !tail.is_empty() && tail.last() != Some(&separator) {
            tail.push(separator);
        }
        tail.extend_from_slice(part);
    }
    if tail.is_empty() {
        // Go preserves a bare UNC volume instead of appending a dot.
        if volume > 1 && is_separator(original[0], windows) && is_separator(original[1], windows) {
            tail.clear();
        } else {
            tail.push(b'.');
        }
    }
    // Go postClean must not turn relative names into drive/device paths.
    if windows && volume == 0 && tail != path {
        if tail
            .split(|byte| is_separator(*byte, true))
            .next()
            .is_some_and(|first| first.contains(&b':'))
        {
            tail.splice(..0, [b'.', separator]);
        } else if tail.starts_with(br"\??") {
            tail.splice(..0, [separator, b'.']);
        }
    }
    let mut result = original[..volume].to_vec();
    result.extend(tail);
    if windows {
        for byte in &mut result {
            if *byte == b'/' {
                *byte = separator;
            }
        }
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
            assert_eq!(clean(input, false), expected);
        }
    }

    #[test]
    fn lexical_windows_drive_unc_and_device_roots_follow_sdk() {
        for (input, expected) in [
            (
                br"C:\link\..\home\.symaira\bin".as_slice(),
                br"C:\home\.symaira\bin".as_slice(),
            ),
            (br"C:..\home\.symaira\bin", br"C:..\home\.symaira\bin"),
            (
                br"\\host\share\..\home\.symaira\bin",
                br"\\host\share\home\.symaira\bin",
            ),
            (
                br"\\?\C:\..\home\.symaira\bin",
                br"\\?\C:\home\.symaira\bin",
            ),
            (br"a\..\c:home\.symaira\bin", br".\c:home\.symaira\bin"),
            (br"\a\..\??\c:\home", br"\.\??\c:\home"),
            (br"\\host\share", br"\\host\share"),
        ] {
            assert_eq!(clean(input, true), expected);
        }
    }

    #[test]
    #[cfg(windows)]
    fn managed_join_keeps_drive_relative_and_unc_ownership() {
        use std::ffi::OsStr;
        for (home, expected) in [
            (r"C:", r"C:.symaira\bin"),
            (r"C:\home\..\owner", r"C:\owner\.symaira\bin"),
            (r"\\host\share\..\owner", r"\\host\share\owner\.symaira\bin"),
            (r"\\?\C:\home\..\owner", r"\\?\C:\owner\.symaira\bin"),
        ] {
            assert_eq!(
                super::managed_path(OsStr::new(home)).as_os_str(),
                OsStr::new(expected)
            );
        }
    }
}
