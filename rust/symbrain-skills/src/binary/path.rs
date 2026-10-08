// Copyright 2009 The Go Authors.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//    * Redistributions of source code must retain the above copyright
// notice, this list of conditions and the following disclaimer.
//    * Redistributions in binary form must reproduce the above
// copyright notice, this list of conditions and the following disclaimer
// in the documentation and/or other materials provided with the
// distribution.
//    * Neither the name of Google LLC nor the names of its
// contributors may be used to endorse or promote products derived from
// this software without specific prior written permission.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
// "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
// LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
// OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
// SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT
// LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE,
// DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY
// THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
// (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Pinned lexical Go filepath.Join/Clean for discovered candidates only.
//! Adapted from Go Authors filepath (2010/2024) and filepathlite (2024).
//! Native units are preserved; never canonicalize or traverse symlinks here.
use std::path::{Path, PathBuf};

pub(crate) fn join(directory: &Path, program: &Path) -> PathBuf {
    let mut path = units(directory);
    let next = units(program);
    #[cfg(windows)]
    let mut next = next;
    if !path.is_empty() && !next.is_empty() {
        #[cfg(unix)]
        path.push(47);
        #[cfg(windows)]
        match path.last().copied() {
            Some(last) if separator(last) => {
                let start = next
                    .iter()
                    .position(|unit| !separator(*unit))
                    .unwrap_or(next.len());
                drop(next.drain(..start));
                if path.len() == 1
                    && next.starts_with(&[63, 63])
                    && (next.len() == 2 || separator(next[2]))
                {
                    path.extend_from_slice(&[46, 92]);
                }
            }
            Some(58) => {}
            _ => path.push(92),
        }
    }
    path.extend(next);
    #[cfg(unix)]
    {
        native(clean(&path))
    }
    #[cfg(windows)]
    {
        native(&clean(&path))
    }
}

pub(super) fn is_absolute(path: &Path) -> bool {
    let input = units(path);
    #[cfg(unix)]
    return input.first() == Some(&47);
    #[cfg(windows)]
    {
        let volume = volume_len(&input);
        volume > 0
            && (input.len() > 1 && separator(input[0]) && separator(input[1])
                || input.get(volume).is_some_and(|unit| separator(*unit)))
    }
}

#[cfg(unix)]
fn units(path: &Path) -> Vec<u16> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|byte| u16::from(*byte))
        .collect()
}
#[cfg(windows)]
fn units(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str().encode_wide().collect()
}
#[cfg(unix)]
fn native(input: Vec<u16>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(
        input
            .into_iter()
            .map(|unit| unit.to_le_bytes()[0])
            .collect(),
    )
    .into()
}
#[cfg(windows)]
fn native(input: &[u16]) -> PathBuf {
    use std::os::windows::ffi::OsStringExt;
    std::ffi::OsString::from_wide(input).into()
}
fn separator(unit: u16) -> bool {
    #[cfg(unix)]
    return unit == 47;
    #[cfg(windows)]
    return matches!(unit, 47 | 92);
}
fn clean(input: &[u16]) -> Vec<u16> {
    #[cfg(unix)]
    let volume = 0;
    #[cfg(windows)]
    let volume = volume_len(input);
    let path = &input[volume..];
    if path.is_empty() {
        let mut result = input.to_owned();
        if !(volume > 1 && separator(input[0]) && separator(input[1])) {
            result.push(46);
        }
        return from_slash(result);
    }
    let rooted = separator(path[0]);
    let mut parts: Vec<&[u16]> = Vec::new();
    for part in path.split(|unit| separator(*unit)) {
        if part.is_empty() || part == [46] {
            continue;
        }
        if part == [46, 46] {
            if parts.last().is_some_and(|previous| *previous != [46, 46]) {
                let _ = parts.pop();
            } else if !rooted {
                parts.push(part);
            }
        } else {
            parts.push(part);
        }
    }
    #[cfg(unix)]
    let slash = 47;
    #[cfg(windows)]
    let slash = 92;
    let mut result = input[..volume].to_owned();
    if rooted {
        result.push(slash);
    }
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            result.push(slash);
        }
        result.extend_from_slice(part);
    }
    if result.len() == volume {
        result.push(46);
    }
    #[cfg(windows)]
    if volume == 0 {
        // Same postClean protection against new drive/device-root meanings.
        let first = result
            .split(|unit| separator(*unit))
            .next()
            .unwrap_or_default();
        if first.contains(&58) {
            drop(result.splice(..0, [46, 92]));
        } else if result.starts_with(&[92, 63, 63]) {
            drop(result.splice(..0, [92, 46]));
        }
    }
    from_slash(result)
}
fn from_slash(input: Vec<u16>) -> Vec<u16> {
    #[cfg(windows)]
    let mut input = input;
    #[cfg(windows)]
    for unit in &mut input {
        if *unit == 47 {
            *unit = 92;
        }
    }
    input
}

#[cfg(windows)]
fn volume_len(input: &[u16]) -> usize {
    // Go detects a drive at byte1 of WTF-8. A multi-byte first unit is not
    // a drive letter, although the native units themselves remain untouched.
    if input.len() >= 2 && input[0] < 128 && input[1] == 58 {
        return 2;
    }
    if input.is_empty() || !separator(input[0]) {
        return 0;
    }
    if prefix(input, b"\\\\.\\UNC") {
        return unc_len(input, 8);
    }
    if prefix(input, b"\\\\.") || prefix(input, b"\\\\?") || prefix(input, b"\\??") {
        if input.len() == 3 {
            return 3;
        }
        return input
            .iter()
            .enumerate()
            .skip(4)
            .find(|(_, unit)| separator(**unit))
            .map_or(input.len(), |(index, _)| index);
    }
    if input.len() >= 2 && separator(input[1]) {
        return unc_len(input, 2);
    }
    0
}
#[cfg(windows)]
fn prefix(input: &[u16], prefix: &[u8]) -> bool {
    input.len() >= prefix.len()
        && input.iter().zip(prefix).all(|(unit, byte)| {
            if separator(u16::from(*byte)) {
                separator(*unit)
            } else {
                u8::try_from(*unit).is_ok_and(|candidate| {
                    candidate.is_ascii() && candidate.eq_ignore_ascii_case(byte)
                })
            }
        })
        && (input.len() == prefix.len() || separator(input[prefix.len()]))
}
#[cfg(windows)]
fn unc_len(input: &[u16], start: usize) -> usize {
    input
        .iter()
        .enumerate()
        .skip(start)
        .filter(|(_, unit)| separator(**unit))
        .nth(1)
        .map_or(input.len(), |(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    use std::os::windows::ffi::OsStringExt;
    #[cfg(unix)]
    #[test]
    fn unix_candidates_clean_lexically_without_losing_bytes() {
        use std::os::unix::ffi::OsStrExt;
        for (directory, expected) in [
            (
                b"/owned/missing/../bin".as_slice(),
                b"/owned/bin/symvault".as_slice(),
            ),
            (b"/owned/link/../bin", b"/owned/bin/symvault"),
            (b"/owned//./bin/", b"/owned/bin/symvault"),
            (b"../../bin/..", b"../../symvault"),
            (b"/../../bin", b"/bin/symvault"),
            (b"/raw-\xff/../bin-\xfe", b"/bin-\xfe/symvault"),
            (b"", b"symvault"),
        ] {
            let path = join(
                Path::new(std::ffi::OsStr::from_bytes(directory)),
                Path::new("symvault"),
            );
            assert_eq!(path.as_os_str().as_bytes(), expected);
        }
    }
    #[cfg(windows)]
    #[test]
    fn windows_candidates_keep_drive_unc_device_and_dotdot_rules() {
        for (directory, expected) in [
            (r"C:\owned\missing\..\bin", r"C:\owned\bin\symvault"),
            (r"C:\owned\link\..\bin", r"C:\owned\bin\symvault"),
            (r"C:", r"C:symvault"),
            (r"C:bin\..", r"C:symvault"),
            (r"\bin\..", r"\symvault"),
            (r"..\..\bin\..", r"..\..\symvault"),
            (r"\\host\share\bin\..", r"\\host\share\symvault"),
            (r"\\?\C:\bin\..", r"\\?\C:\symvault"),
            (
                r"\\.\UNC\host\share\..\bin",
                r"\\.\UNC\host\share\bin\symvault",
            ),
            // Go Join never cleans elements first: `a\..\c:` ends in ':' so no
            // separator is added, Clean gives `c:symvault`, and postClean
            // prefixes `.\` because ':' precedes the first separator.
            (r"a\..\c:", r".\c:symvault"),
            (r"\a\..\??\x", r"\.\??\x\symvault"),
            ("C:/owned//./bin/", r"C:\owned\bin\symvault"),
        ] {
            assert_eq!(
                join(Path::new(directory), Path::new("symvault")),
                PathBuf::from(expected),
                "{directory}"
            );
        }
        assert!(prefix(
            &[92, 92, 46, 92, 117, 110, 99, 92],
            &[92, 92, 46, 92, 85, 78, 67]
        ));
        assert!(!prefix(
            &[92, 92, 46, 92, 0x0130, 78, 67, 92],
            &[92, 92, 46, 92, 85, 78, 67]
        ));
        let raw = std::ffi::OsString::from_wide(&[67, 58, 92, 0xd800, 92, 46, 46, 92, 0xd801]);
        assert_eq!(
            units(&join(Path::new(&raw), Path::new("symvault"))),
            [
                67, 58, 92, 0xd801, 92, 115, 121, 109, 118, 97, 117, 108, 116
            ]
        );
    }
}
