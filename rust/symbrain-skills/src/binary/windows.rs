//! Native Windows Go `LookPath`: extensions, implicit cwd and `Lstat` identity.
#[cfg(windows)]
use super::{Executable, godebug, path};
#[cfg(windows)]
use crate::SkillError;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::{ffi::OsStr, fs};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

#[cfg(windows)]
#[path = "windows_lower.rs"]
mod windows_lower;

#[cfg(windows)]
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

#[cfg(windows)]
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

#[cfg(windows)]
fn extensions() -> Vec<OsString> {
    let text = std::env::var_os("PATHEXT")
        .filter(|value| !value.is_empty())
        .map_or_else(
            || ".com;.exe;.bat;.cmd".to_owned(),
            |value| go_lower(value.as_encoded_bytes()),
        );
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
#[cfg(windows)]
fn go_lower(bytes: &[u8]) -> String {
    go_text(bytes).chars().map(windows_lower::lower).collect()
}

#[cfg(windows)]
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

#[cfg(windows)]
fn find(program: &Path, extensions: &[OsString]) -> Option<PathBuf> {
    // Go's hasExt counts any dot in the final component, including a leading dot.
    let has_extension =
        !extensions.is_empty() && program.file_name()?.encode_wide().any(|unit| unit == 46);
    find_with(program, extensions, has_extension, |candidate| {
        fs::metadata(candidate).is_ok_and(|metadata| !metadata.is_dir())
    })
}

fn find_with(
    program: &Path,
    extensions: &[OsString],
    has_extension: bool,
    mut present: impl FnMut(&Path) -> bool,
) -> Option<PathBuf> {
    if extensions.is_empty() {
        return present(program).then(|| program.to_owned());
    }
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

#[cfg(windows)]
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
    #[cfg(windows)]
    use std::sync::atomic::{AtomicU64, Ordering};
    #[cfg(windows)]
    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[cfg(windows)]
    struct Owned(PathBuf);
    #[cfg(windows)]
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
    #[cfg(windows)]
    impl Drop for Owned {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn find_uses_only_go_pathext_candidates() {
        use std::fs as test_fs;

        let root = tempfile::tempdir().expect("fixture directory");
        let extensionless = root.path().join("tool");
        let exe = root.path().join("tool.exe");
        let cmd = root.path().join("tool.cmd");
        let dotted = root.path().join("tool.bat");
        let dotted_exe = root.path().join("tool.bat.exe");
        let extensions = [OsString::from(".exe"), OsString::from(".cmd")];
        test_fs::write(&extensionless, b"not a PATHEXT candidate").expect("extensionless fixture");

        let present = |candidate: &Path| {
            test_fs::metadata(candidate).is_ok_and(|metadata| !metadata.is_dir())
        };
        assert_eq!(
            find_with(&extensionless, &extensions, false, present),
            None,
            "PATHEXT lookup must not probe an exact extensionless file"
        );
        test_fs::write(&exe, b"exe").expect("exe fixture");
        test_fs::write(&cmd, b"cmd").expect("cmd fixture");
        assert_eq!(
            find_with(&extensionless, &extensions, false, present),
            Some(exe.clone()),
            "PATHEXT candidates must be searched in declared order"
        );

        test_fs::write(&dotted_exe, b"bat executable").expect("dotted executable fixture");
        assert_eq!(
            find_with(&dotted, &extensions, true, present),
            Some(dotted_exe),
            "a missing dotted basename must still try appended extensions"
        );
        test_fs::write(&dotted, b"exact dotted file").expect("dotted basename fixture");
        assert_eq!(
            find_with(&dotted, &extensions, true, present),
            Some(dotted),
            "an existing dotted basename takes precedence over appended extensions"
        );
        assert_eq!(
            find_with(&extensionless, &[], false, present),
            Some(extensionless),
            "an empty PATHEXT list probes the exact path"
        );
    }

    #[cfg(windows)]
    #[test]
    fn pathext_uses_go_simple_lowercase_and_one_replacement_per_invalid_byte() {
        assert_eq!(go_lower(b".EXE;.CMD"), ".exe;.cmd");
        assert_eq!(go_lower(".İ;Ä;ǅǄǆ".as_bytes()), ".i;ä;ǆǆǆ");
        assert_eq!(go_lower(b".E\xed\xa0\x80"), ".e\u{fffd}\u{fffd}\u{fffd}");
        assert_eq!(go_lower(b"\xe2\x82"), "\u{fffd}\u{fffd}");
        // Unicode16 added this casing; pinned Go Unicode15 leaves it alone.
        assert_eq!(go_lower("\u{1c89}".as_bytes()), "\u{1c89}");
    }

    #[cfg(windows)]
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

    #[cfg(windows)]
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
