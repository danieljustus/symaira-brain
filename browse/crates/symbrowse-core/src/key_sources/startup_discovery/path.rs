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

pub(super) fn join(directory: &Path, program: &Path) -> PathBuf {
    let mut path = units(directory);
    let next = units(program);
    if !path.is_empty() && !next.is_empty() {
        path.push(47);
    }
    path.extend(next);
    native(clean(&path))
}

pub(super) fn is_absolute(path: &Path) -> bool {
    let input = units(path);
    input.first() == Some(&47)
}

fn units(path: &Path) -> Vec<u16> {
    use std::os::unix::ffi::OsStrExt;
    path.as_os_str()
        .as_bytes()
        .iter()
        .map(|byte| u16::from(*byte))
        .collect()
}
fn native(input: Vec<u16>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    std::ffi::OsString::from_vec(input.into_iter().map(|unit| unit as u8).collect()).into()
}
fn separator(unit: u16) -> bool {
    unit == 47
}
fn clean(input: &[u16]) -> Vec<u16> {
    let volume = 0;
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
    let slash = 47;
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
    from_slash(result)
}
fn from_slash(input: Vec<u16>) -> Vec<u16> {
    input
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
