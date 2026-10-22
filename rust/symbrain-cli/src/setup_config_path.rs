//! The configuration path exactly as the Go loader prints it.
//!
//! corekit's `configkit.DefaultPath` returns `filepath.Join(...)`, whose last
//! step is `filepath.Clean`, and the message embeds that cleaned text. On
//! Windows `Clean` ends in `FromSlash`: every `/` (including those inside
//! `XDG_CONFIG_HOME` or the home directory) is printed as `\`, empty and `.`
//! elements vanish and `..` resolves lexically. Rust's `Path::join` keeps the
//! base text untouched (`C:/cfg` joined with `symbrain` is `C:/cfg\symbrain`),
//! so the diagnostic prints this port of `internal/filepathlite.Clean` (Go
//! 1.26, Windows variant) while the file system keeps using the real path.
use std::path::Path;

use symbrain_core::config::os_bytes;

/// Bytes Go prints for `path`: Windows applies `filepath.Clean`; every other
/// platform prints the path unchanged.
pub(super) fn printed(path: &Path) -> Vec<u8> {
    let bytes = os_bytes(path.as_os_str());
    if cfg!(windows) {
        clean_windows(&bytes)
    } else {
        bytes.into_owned()
    }
}

const SEPARATOR: u8 = b'\\';

const fn is_separator(byte: u8) -> bool {
    byte == b'\\' || byte == b'/'
}

fn from_slash(path: &[u8]) -> Vec<u8> {
    path.iter()
        .map(|&byte| if byte == b'/' { SEPARATOR } else { byte })
        .collect()
}

/// Go's `lazybuf`: output is only materialised once it diverges from the
/// input, and `post_clean` inspects the whole materialised buffer (stale bytes
/// past `written` included) exactly as Go does.
struct LazyBuf<'a> {
    path: &'a [u8],
    buf: Option<Vec<u8>>,
    written: usize,
}

impl LazyBuf<'_> {
    fn index(&self, position: usize) -> u8 {
        self.buf
            .as_ref()
            .map_or_else(|| self.path[position], |buf| buf[position])
    }

    fn append(&mut self, byte: u8) {
        if self.buf.is_none() {
            if self.path.get(self.written) == Some(&byte) {
                self.written += 1;
                return;
            }
            let mut fresh = vec![0; self.path.len()];
            fresh[..self.written].copy_from_slice(&self.path[..self.written]);
            self.buf = Some(fresh);
        }
        if let Some(buf) = self.buf.as_mut() {
            if let Some(slot) = buf.get_mut(self.written) {
                *slot = byte;
            } else {
                buf.push(byte);
            }
            self.written += 1;
        }
    }

    fn prepend(&mut self, prefix: &[u8]) {
        if let Some(buf) = self.buf.as_mut() {
            buf.splice(0..0, prefix.iter().copied());
            self.written += prefix.len();
        }
    }

    fn finish(&self, volume: &[u8]) -> Vec<u8> {
        let mut text = volume.to_vec();
        match &self.buf {
            Some(buf) => text.extend_from_slice(&buf[..self.written]),
            None => text.extend_from_slice(&self.path[..self.written]),
        }
        text
    }
}

/// Go `filepath.Clean` with Windows semantics, byte for byte.
pub(super) fn clean_windows(original: &[u8]) -> Vec<u8> {
    let volume_len = volume_name_len(original);
    let path = &original[volume_len..];
    if path.is_empty() {
        if volume_len > 1 && is_separator(original[0]) && is_separator(original[1]) {
            return from_slash(original); // UNC root
        }
        let mut text = original.to_vec();
        text.push(b'.');
        return text;
    }
    let rooted = is_separator(path[0]);
    let mut out = LazyBuf {
        path,
        buf: None,
        written: 0,
    };
    let (mut read, mut dotdot) = (0, 0);
    if rooted {
        out.append(SEPARATOR);
        (read, dotdot) = (1, 1);
    }
    while read < path.len() {
        let next = path.get(read + 1).copied();
        let after = path.get(read + 2).copied();
        if is_separator(path[read]) || (path[read] == b'.' && next.is_none_or(is_separator)) {
            read += 1; // empty element or `.`
        } else if path[read] == b'.' && next == Some(b'.') && after.is_none_or(is_separator) {
            read += 2;
            if out.written > dotdot {
                out.written -= 1;
                while out.written > dotdot && !is_separator(out.index(out.written)) {
                    out.written -= 1;
                }
            } else if !rooted {
                if out.written > 0 {
                    out.append(SEPARATOR);
                }
                out.append(b'.');
                out.append(b'.');
                dotdot = out.written;
            }
        } else {
            if (rooted && out.written != 1) || (!rooted && out.written != 0) {
                out.append(SEPARATOR);
            }
            while read < path.len() && !is_separator(path[read]) {
                out.append(path[read]);
                read += 1;
            }
        }
    }
    if out.written == 0 {
        out.append(b'.');
    }
    post_clean(&mut out, volume_len);
    from_slash(&out.finish(&original[..volume_len]))
}

/// Keeps a relative path from turning into a drive-relative or device path.
fn post_clean(out: &mut LazyBuf<'_>, volume_len: usize) {
    if volume_len != 0 {
        return;
    }
    let Some(buf) = out.buf.as_ref() else {
        return;
    };
    let colon_first = buf.iter().find(|&&byte| is_separator(byte) || byte == b':') == Some(&b':');
    let device_prefix = buf.len() >= 3 && is_separator(buf[0]) && buf[1] == b'?' && buf[2] == b'?';
    if colon_first {
        out.prepend(&[b'.', SEPARATOR]);
    } else if device_prefix {
        out.prepend(&[SEPARATOR, b'.']);
    }
}

/// Go `volumeNameLen` (Windows): drive letter, UNC share or device prefix.
fn volume_name_len(path: &[u8]) -> usize {
    if path.len() >= 2 && path[1] == b':' {
        return 2;
    }
    if path.first().is_none_or(|&byte| !is_separator(byte)) {
        return 0;
    }
    if has_prefix_fold(path, br"\\.\UNC") {
        return unc_len(path, br"\\.\UNC\".len());
    }
    if ["\\\\.", "\\\\?", "\\??"]
        .iter()
        .any(|prefix| has_prefix_fold(path, prefix.as_bytes()))
    {
        if path.len() == 3 {
            return 3;
        }
        return path[4..]
            .iter()
            .position(|&byte| is_separator(byte))
            .map_or(path.len(), |offset| 4 + offset);
    }
    if path.len() >= 2 && is_separator(path[1]) {
        return unc_len(path, 2);
    }
    0
}

/// Case-insensitive prefix test where every separator matches any separator;
/// a longer `path` must continue with a separator.
fn has_prefix_fold(path: &[u8], prefix: &[u8]) -> bool {
    if path.len() < prefix.len() {
        return false;
    }
    let same = prefix.iter().zip(path).all(|(&wanted, &actual)| {
        if is_separator(wanted) {
            is_separator(actual)
        } else {
            wanted.eq_ignore_ascii_case(&actual)
        }
    });
    same && path
        .get(prefix.len())
        .is_none_or(|&byte| is_separator(byte))
}

/// Length of a UNC volume (`\\host\share`) given the prefix before the host.
fn unc_len(path: &[u8], prefix_len: usize) -> usize {
    let mut separators = 0;
    for (offset, &byte) in path.iter().enumerate().skip(prefix_len) {
        if is_separator(byte) {
            separators += 1;
            if separators == 2 {
                return offset;
            }
        }
    }
    path.len()
}

#[cfg(test)]
#[path = "setup_config_path_tests.rs"]
mod tests;
