//! Go-shaped error and path text shared by the profile, discovery and VCS tools.

use std::fmt::Write as _;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Go's `syscall.Errno` text: libc `strerror` with a lower-case first letter,
/// without Rust's ` (os error N)` suffix.
#[must_use]
pub fn io_text(error: &io::Error) -> String {
    let text = error.to_string();
    let text = text
        .rsplit_once(" (os error ")
        .map_or(text.as_str(), |(head, _)| head);
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_lowercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

/// Go's `*fs.PathError` text: `op path: reason`.
#[must_use]
pub fn path_error(operation: &str, path: &Path, error: &io::Error) -> String {
    format!("{operation} {}: {}", path.display(), io_text(error))
}

/// Lexical `filepath.Clean` for unix-style paths (no symlink resolution).
#[must_use]
pub fn clean(path: &Path) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => out.push(component),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        return PathBuf::from(".");
    }
    out.iter().collect()
}

/// `filepath.Join(base, elem)`: empty elements are skipped, the rest are
/// joined with a separator and lexically cleaned. Unlike [`Path::join`], an
/// absolute `elem` is **not** a new root (`Join("/lib", "/abs")` is
/// `/lib/abs`), so a caller-supplied name can never leave `base` except by
/// relative `..` components, exactly as in Go.
#[must_use]
pub fn join(base: impl AsRef<Path>, elem: impl AsRef<Path>) -> PathBuf {
    let (base, elem) = (base.as_ref(), elem.as_ref());
    match (base.as_os_str().is_empty(), elem.as_os_str().is_empty()) {
        (true, true) => PathBuf::new(),
        (true, false) => clean(elem),
        (false, true) => clean(base),
        (false, false) => {
            let mut joined = base.as_os_str().to_os_string();
            joined.push("/");
            joined.push(elem.as_os_str());
            clean(Path::new(&joined))
        }
    }
}

/// `filepath.Abs`: join with the working directory, then clean.
#[must_use]
pub fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        clean(path)
    } else {
        clean(&std::env::current_dir().unwrap_or_default().join(path))
    }
}

/// Go's `%q` for a string: double-quoted with Go escapes for control characters.
#[must_use]
pub fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{b}' => out.push_str("\\v"),
            ch if (ch as u32) < 0x20 || ch as u32 == 0x7f => {
                let _ = write!(out, "\\x{:02x}", ch as u32);
            }
            ch if ch.is_control() => {
                if (ch as u32) < 0x10000 {
                    let _ = write!(out, "\\u{:04x}", ch as u32);
                } else {
                    let _ = write!(out, "\\U{:08x}", ch as u32);
                }
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_matches_go_percent_q_for_controls_and_quotes() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
        assert_eq!(quote("tab\tnl\n"), "\"tab\\tnl\\n\"");
        assert_eq!(quote("\u{1}\u{7f}"), "\"\\x01\\x7f\"");
        assert_eq!(quote("größe 日本"), "\"größe 日本\"");
    }

    #[test]
    fn clean_is_lexical_like_filepath_clean() {
        assert_eq!(clean(Path::new("/a/b/../c/./d/")), PathBuf::from("/a/c/d"));
        assert_eq!(clean(Path::new("/a/../..")), PathBuf::from("/"));
        assert_eq!(clean(Path::new("a/..")), PathBuf::from("."));
        assert_eq!(clean(Path::new("../a")), PathBuf::from("../a"));
    }

    #[test]
    fn join_is_go_filepath_join_not_path_join() {
        // An absolute second element is re-rooted under the base, never a new root.
        assert_eq!(join("/lib", "/etc/x"), PathBuf::from("/lib/etc/x"));
        assert_eq!(join("/lib", "/"), PathBuf::from("/lib"));
        assert_eq!(join("/lib", "a/../b"), PathBuf::from("/lib/b"));
        assert_eq!(join("/lib", "../x"), PathBuf::from("/x"));
        assert_eq!(join("/lib/", "./a//b/"), PathBuf::from("/lib/a/b"));
        assert_eq!(join("", "x"), PathBuf::from("x"));
        assert_eq!(join("x", ""), PathBuf::from("x"));
        assert_eq!(join("", ""), PathBuf::new());
        assert_eq!(join(".", ".."), PathBuf::from(".."));
        assert_eq!(join("a", ".."), PathBuf::from("."));
    }

    #[cfg(unix)]
    #[test]
    fn io_text_is_go_errno_text() {
        let error = io::Error::from_raw_os_error(2);
        assert_eq!(io_text(&error), "no such file or directory");
        assert_eq!(
            path_error("lstat", Path::new("/x"), &error),
            "lstat /x: no such file or directory"
        );
    }
}
