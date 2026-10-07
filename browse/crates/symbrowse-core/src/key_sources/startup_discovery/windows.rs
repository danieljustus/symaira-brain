//! Native Windows Go LookPath: extensions, implicit cwd and Lstat identity.
use super::{Executable, godebug, path};
use crate::key_resolver::ProbeError;
use std::{
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

#[path = "windows_lower.rs"]
mod windows_lower;

pub(super) fn lookup(program: &Path, path: &OsStr) -> Result<Option<Executable>, ProbeError> {
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
    extensions_from(std::env::var_os("PATHEXT"))
}

fn extensions_from(value: Option<OsString>) -> Vec<OsString> {
    let text = value
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
    fn empty_pathext_list_launches_extensionless_owner_not_exe_sibling() {
        // Go: PATHEXT=";;" parses to no extensions, so LookPath selects the
        // extensionless file and CreateProcess runs exactly that image.
        assert!(extensions_from(Some(";;".into())).is_empty());
        assert_eq!(extensions_from(None).len(), 4);
        let root = Owned::new();
        let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap()).join("System32");
        let owner = root.0.join("tool");
        fs::copy(system.join("whoami.exe"), &owner).unwrap();
        fs::copy(system.join("hostname.exe"), root.0.join("tool.exe")).unwrap();
        assert_eq!(find(&owner, &[]), Some(owner.clone()));
        let resolved = Executable {
            owner: owner.clone(),
            spelling: owner.clone(),
        };
        let launch = super::super::launch_path(Path::new("tool"), &resolved);
        assert_eq!(launch.as_os_str(), root.0.join("tool.").as_os_str());
        let run = |program: &Path| std::process::Command::new(program).output().unwrap().stdout;
        assert_eq!(run(&launch), run(&system.join("whoami.exe")));
        // Without the spelling, std would have run the `.exe` sibling.
        assert_eq!(run(&owner), run(&system.join("hostname.exe")));
        // Extension-bearing and explicit owners keep their exact path.
        let exe = Executable {
            owner: root.0.join("tool.exe"),
            spelling: root.0.join("tool.exe"),
        };
        assert_eq!(
            super::super::launch_path(Path::new("tool"), &exe),
            root.0.join("tool.exe")
        );
        assert_eq!(super::super::launch_path(&owner, &resolved), owner);
        // Any non-.exe owner: std would run `tool.com.exe` instead of `tool.com`.
        let com = root.0.join("tool.com");
        fs::copy(system.join("whoami.exe"), &com).unwrap();
        fs::copy(system.join("hostname.exe"), root.0.join("tool.com.exe")).unwrap();
        let com_owner = Executable {
            owner: com.clone(),
            spelling: com.clone(),
        };
        let launch = super::super::launch_path(Path::new("tool"), &com_owner);
        assert_eq!(launch.as_os_str(), root.0.join("tool.com.").as_os_str());
        assert_eq!(run(&launch), run(&system.join("whoami.exe")));
        assert_eq!(run(&com), run(&system.join("hostname.exe")));
        // Verbatim paths are not normalized; they are passed through unchanged.
        let verbatim = PathBuf::from(r"\\?\C:\owned\tool");
        let verbatim_owner = Executable {
            owner: verbatim.clone(),
            spelling: verbatim.clone(),
        };
        assert_eq!(
            super::super::launch_path(Path::new("tool"), &verbatim_owner),
            verbatim
        );
    }
    #[test]
    fn batch_refusal_uses_win32_trailing_dot_and_space_normalization() {
        // PATHEXT ".BAT." or ".CMD " selects `symvault.bat.`/`symvault.cmd `,
        // which Win32 opens as the batch file and CreateProcess runs via cmd.
        for name in [
            "symvault.bat",
            "symvault.bat.",
            "symvault.BAT..",
            "symvault.cmd ",
            "symvault.Cmd. .",
        ] {
            let resolved = PathBuf::from(r"C:\owned").join(name);
            assert!(
                super::super::batch_error(Path::new("symvault"), &resolved).is_some(),
                "{name}"
            );
        }
        for name in [
            "symvault.exe",
            "symvault.exe.",
            "symvault.batx",
            "symvault",
            "symvault.",
        ] {
            let resolved = PathBuf::from(r"C:\owned").join(name);
            assert!(
                super::super::batch_error(Path::new("symvault"), &resolved).is_none(),
                "{name}"
            );
        }
        // Explicit paths keep their existing behaviour.
        let explicit = PathBuf::from(r"C:\owned\symvault.bat.");
        assert!(super::super::batch_error(&explicit, &explicit).is_none());
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
