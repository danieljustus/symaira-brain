//! Native Windows Go LookPath: extensions, implicit cwd and Lstat identity.
use super::{Executable, godebug, path};
use crate::SkillError;
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

#[path = "windows_lower.rs"]
mod windows_lower;

pub(super) fn lookup(program: &Path, path: &OsStr) -> Result<Option<Executable>, SkillError> {
    let extensions = extensions();
    let mut implicit = if std::env::var_os("NoDefaultCurrentDirectoryInExePath").is_none() {
        find(&path::join(Path::new("."), program), &extensions)
    } else {
        None
    };
    if let Some(found) = &implicit
        && godebug::allow_relative()?
    {
        return Ok(absolute(found.clone()));
    }
    for directory in std::env::split_paths(path).filter(|part| !part.as_os_str().is_empty()) {
        let Some(candidate) = find(&path::join(&directory, program), &extensions) else {
            continue;
        };
        if let Some(dot) = &implicit
            && !same_file(dot, &candidate)
        {
            return Ok(None);
        }
        if !path::is_absolute(&candidate) && !godebug::allow_relative()? {
            implicit.get_or_insert(candidate);
            continue;
        }
        return Ok(absolute(candidate));
    }
    Ok(None)
}

fn absolute(candidate: PathBuf) -> Option<Executable> {
    let owner = if path::is_absolute(&candidate) {
        Some(candidate.clone())
    } else {
        // Native GetFullPathName resolves drive-relative and rooted paths;
        // it does not traverse symlinks or canonicalize the selected owner.
        std::path::absolute(&candidate).ok()
    }?;
    Some(Executable {
        owner,
        spelling: candidate,
    })
}

fn extensions() -> Vec<OsString> {
    let text = std::env::var_os("PATHEXT")
        .filter(|value| !value.is_empty())
        .map(|value| go_lower(value.as_encoded_bytes()))
        .unwrap_or_else(|| ".com;.exe;.bat;.cmd".into());
    text.split(';')
        .filter(|part| !part.is_empty())
        .map(|part| {
            OsString::from(if part.starts_with('.') {
                part.to_owned()
            } else {
                format!(".{part}")
            })
        })
        .collect()
}

// Go UTF16ToString keeps WTF-8 in env values; strings.ToLower then decodes
// invalid UTF-8 one byte at a time. This conversion is ONLY for PATHEXT.
// PATH and provider OsString values retain their native units unchanged.
fn go_lower(bytes: &[u8]) -> String {
    go_text(bytes).chars().map(windows_lower::lower).collect()
}

pub(super) fn go_text(mut bytes: &[u8]) -> String {
    let mut output = String::new();
    while !bytes.is_empty() {
        let (valid, rest) = match std::str::from_utf8(bytes) {
            Ok(valid) => (valid, &[][..]),
            Err(error) => {
                let (prefix, rest) = bytes.split_at(error.valid_up_to());
                (
                    std::str::from_utf8(prefix).expect("validated UTF-8 prefix"),
                    rest,
                )
            }
        };
        output.push_str(valid);
        if rest.is_empty() {
            break;
        }
        output.push('\u{fffd}');
        bytes = &rest[1..];
    }
    output
}

fn find(program: &Path, extensions: &[OsString]) -> Option<PathBuf> {
    fn present(path: &Path) -> bool {
        fs::metadata(path).is_ok_and(|metadata| !metadata.is_dir())
    }
    if extensions.is_empty() {
        return present(program).then(|| program.to_owned());
    }
    // The provider is a basename here. A dot starts an extension, including
    // a leading dot; don't inspect or normalize its native encoding.
    use std::os::windows::ffi::OsStrExt;
    let has_extension = program.file_name()?.encode_wide().any(|unit| unit == 46);
    if has_extension && present(program) {
        return Some(program.to_owned());
    }
    for extension in extensions {
        let mut candidate = program.as_os_str().to_owned();
        candidate.push(extension);
        let candidate = PathBuf::from(candidate);
        if present(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn same_file(first: &Path, second: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    fn identity(path: &Path) -> std::io::Result<same_file::Handle> {
        // Match Go Lstat: open the reparse entry itself, including directory
        // handles, with read-attributes rather than broad data access.
        let file = fs::OpenOptions::new()
            .access_mode(0x0080)
            .custom_flags(0x0020_0000 | 0x0200_0000)
            .open(path)?;
        same_file::Handle::from_file(file)
    }
    identity(first)
        .ok()
        .zip(identity(second).ok())
        .is_some_and(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Owned(PathBuf);
    impl Owned {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "browse-lstat-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn pathext_uses_go_simple_lowercase_and_one_replacement_per_invalid_byte() {
        assert_eq!(go_lower(b".EXE;.CMD"), ".exe;.cmd");
        assert_eq!(go_lower(".İ;Ä;ǅǄǆ".as_bytes()), ".i;ä;ǆǆǆ");
        assert_eq!(go_lower(b".E\xed\xa0\x80"), ".e\u{fffd}\u{fffd}\u{fffd}");
        assert_eq!(go_lower(b"\xe2\x82"), "\u{fffd}\u{fffd}");
        // Unicode16 added this casing; pinned Go Unicode15 leaves it alone.
        assert_eq!(go_lower("\u{1c89}".as_bytes()), "\u{1c89}");
    }
    #[test]
    fn lstat_identity_accepts_hardlink_but_not_distinct_file() {
        let root = Owned::new();
        let first = root.0.join("a");
        let second = root.0.join("b");
        let other = root.0.join("c");
        fs::write(&first, b"owned same bytes").unwrap();
        fs::hard_link(&first, &second).unwrap();
        fs::write(&other, b"owned same bytes").unwrap();
        assert!(same_file(&first, &second));
        assert!(!same_file(&first, &other));
    }
    #[test]
    fn lstat_identity_keeps_symlink_entry_distinct_from_its_target() {
        use std::os::windows::fs::symlink_file;
        let root = Owned::new();
        let target = root.0.join("target");
        let link = root.0.join("link");
        fs::write(&target, b"owned").unwrap();
        symlink_file(&target, &link).unwrap();
        assert!(same_file(&link, &link));
        assert!(!same_file(&target, &link));
    }
}
