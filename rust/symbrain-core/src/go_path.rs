// Copyright 2024 The Go Authors. All rights reserved.
// Derived SDK rules use the retained BSD-3-Clause license referenced below.
//! Byte-preserving Go filepath ownership; lexical cleaning never follows links.
//! Derived from Go Authors BSD-3-Clause filepathlite rules; license retained in
//! migration/fixtures/brain-config13/go-sdk-LICENSE.txt.
use std::ffi::OsStr;
use std::path::PathBuf;

#[path = "go_path_codec.rs"]
mod codec;
pub use codec::{from_bytes, os_bytes};

/// Go os.UserHomeDir uses exactly one platform variable.
#[must_use]
pub const fn home_variable() -> &'static str {
    if cfg!(windows) { "USERPROFILE" } else { "HOME" }
}

/// Joins path bytes using the current Go platform's Join/Clean rules.
#[must_use]
pub fn join(parts: &[&OsStr]) -> PathBuf {
    let bytes: Vec<_> = parts.iter().map(|part| os_bytes(part)).collect();
    let refs: Vec<_> = bytes.iter().map(Vec::as_slice).collect();
    PathBuf::from(from_bytes(&join_bytes(&refs, cfg!(windows))))
}

/// Implements SDK filepath.Join, including drive-relative and UNC admission.
#[must_use]
pub fn join_bytes(parts: &[&[u8]], windows: bool) -> Vec<u8> {
    let mut joined = Vec::new();
    for part in parts {
        let mut part = *part;
        if let Some(last) = joined.last().copied() {
            if windows && is_separator(last, true) {
                part = part.trim_ascii_start_matches_separator();
                if joined.len() == 1
                    && part.starts_with(b"??")
                    && (part.len() == 2 || is_separator(part[2], true))
                {
                    joined.extend_from_slice(br".\");
                }
            } else if !windows || last != b':' {
                joined.push(if windows { b'\\' } else { b'/' });
            }
        }
        joined.extend_from_slice(part);
    }
    if joined.is_empty() {
        joined
    } else {
        clean_bytes(&joined, windows)
    }
}

/// Go Windows `IsAbs` needs a volume and separator; a root-only slash isn't absolute.
#[must_use]
pub fn is_absolute(bytes: &[u8], windows: bool) -> bool {
    if !windows {
        return bytes.starts_with(b"/");
    }
    let volume = volume_len(bytes);
    volume != 0
        && ((bytes.len() >= 2 && is_separator(bytes[0], true) && is_separator(bytes[1], true))
            || bytes.get(volume).is_some_and(|b| is_separator(*b, true)))
}

trait LeadingSeparators {
    fn trim_ascii_start_matches_separator(&self) -> &Self;
}
impl LeadingSeparators for [u8] {
    fn trim_ascii_start_matches_separator(&self) -> &Self {
        &self[self
            .iter()
            .position(|b| !is_separator(*b, true))
            .unwrap_or(self.len())..]
    }
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

/// Mirrors the SDK lazy buffer, including stale allocated bytes used by postClean.
#[must_use]
pub fn clean_bytes(original: &[u8], windows: bool) -> Vec<u8> {
    let volume = if windows { volume_len(original) } else { 0 };
    let path = &original[volume..];
    let separator = if windows { b'\\' } else { b'/' };
    if path.is_empty() {
        let mut result = original.to_vec();
        if !(volume > 1 && is_separator(original[0], windows) && is_separator(original[1], windows))
        {
            result.push(b'.');
        }
        return from_slash(result, windows);
    }
    let rooted = is_separator(path[0], windows);
    let mut output = Vec::new();
    let mut slots: Option<Vec<u8>> = None;
    let (mut read, mut boundary) = (0, 0);
    if rooted {
        append(&mut output, separator, path, &mut slots);
        read = 1;
        boundary = 1;
    }
    while read < path.len() {
        if is_separator(path[read], windows)
            || (path[read] == b'.'
                && (read + 1 == path.len() || is_separator(path[read + 1], windows)))
        {
            read += 1;
        } else if path[read] == b'.'
            && path.get(read + 1) == Some(&b'.')
            && (read + 2 == path.len() || is_separator(path[read + 2], windows))
        {
            read += 2;
            if output.len() > boundary {
                output.pop();
                while output.len() > boundary
                    && !is_separator(index(&output, path, slots.as_deref()), windows)
                {
                    output.pop();
                }
            } else if !rooted {
                if !output.is_empty() {
                    append(&mut output, separator, path, &mut slots);
                }
                append(&mut output, b'.', path, &mut slots);
                append(&mut output, b'.', path, &mut slots);
                boundary = output.len();
            }
        } else {
            if (rooted && output.len() != 1) || (!rooted && !output.is_empty()) {
                append(&mut output, separator, path, &mut slots);
            }
            while read < path.len() && !is_separator(path[read], windows) {
                append(&mut output, path[read], path, &mut slots);
                read += 1;
            }
        }
    }
    if output.is_empty() {
        append(&mut output, b'.', path, &mut slots);
    }
    if windows
        && volume == 0
        && let Some(slots) = &slots
    {
        // The SDK scans the entire allocation, not just the visible output.
        if slots
            .split(|byte| is_separator(*byte, true))
            .next()
            .is_some_and(|first| first.contains(&b':'))
        {
            output.splice(..0, [b'.', separator]);
        } else if slots.starts_with(br"\??") {
            output.splice(..0, [separator, b'.']);
        }
    }
    let mut result = original[..volume].to_vec();
    result.extend(output);
    from_slash(result, windows)
}
fn index(output: &[u8], original: &[u8], slots: Option<&[u8]>) -> u8 {
    // Go backtracking reads the backing byte at w, after decrementing w.
    slots.unwrap_or(original)[output.len()]
}
fn append(output: &mut Vec<u8>, byte: u8, original: &[u8], slots: &mut Option<Vec<u8>>) {
    if slots.is_none() && original.get(output.len()) != Some(&byte) {
        let mut buffer = vec![0; original.len()];
        buffer[..output.len()].copy_from_slice(output);
        *slots = Some(buffer);
    }
    if let Some(buffer) = slots {
        buffer[output.len()] = byte;
    }
    output.push(byte);
}
fn from_slash(mut bytes: Vec<u8>, windows: bool) -> Vec<u8> {
    if windows {
        for byte in &mut bytes {
            if *byte == b'/' {
                *byte = b'\\';
            }
        }
    }
    bytes
}
