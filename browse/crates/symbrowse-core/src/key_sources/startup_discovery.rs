//! Go-compatible bare-name discovery for the opt-in daemon startup owner.
//! Explicit native paths and the ordinary standalone runner are unchanged.
#[cfg(unix)]
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

mod godebug;
mod path;
#[cfg(windows)]
mod windows;
use crate::key_resolver::ProbeError;

pub(super) struct Executable {
    pub(super) owner: PathBuf,
    #[cfg(windows)]
    pub(super) spelling: PathBuf,
}

pub(super) fn executable(program: &Path) -> Result<Option<Executable>, ProbeError> {
    if explicit(program) {
        return Ok(Some(Executable {
            owner: program.to_owned(),
            #[cfg(windows)]
            spelling: program.to_owned(),
        }));
    }
    let path = std::env::var_os("PATH").unwrap_or_default();
    #[cfg(unix)]
    return unix_lookup(program, &path);
    #[cfg(windows)]
    return windows::lookup(program, &path);
}

fn explicit(program: &Path) -> bool {
    #[cfg(unix)]
    return program.as_os_str().as_encoded_bytes().contains(&b'/');
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        program
            .as_os_str()
            .encode_wide()
            .any(|unit| matches!(unit, 58 | 47 | 92))
    }
}

#[cfg(unix)]
fn unix_lookup(program: &Path, path: &OsStr) -> Result<Option<Executable>, ProbeError> {
    // Go SplitList("") has zero entries, unlike Rust's split_paths.
    for directory in std::env::split_paths(path).filter(|_| !path.is_empty()) {
        let candidate = self::path::join(&directory, program);
        if !unix_executable(&candidate) {
            continue;
        }
        // Refuse the FIRST executable relative candidate; do not skip to a
        // later absolute program. Go LookupVault treats ErrDot as absence.
        if !self::path::is_absolute(&candidate) && !godebug::allow_relative()? {
            return Ok(None);
        }
        let owner = if self::path::is_absolute(&candidate) {
            Some(candidate)
        } else {
            std::env::current_dir().ok().map(|cwd| cwd.join(candidate))
        };
        return Ok(owner.map(|owner| Executable { owner }));
    }
    Ok(None)
}

#[cfg(unix)]
fn unix_executable(candidate: &Path) -> bool {
    use rustix::fs::{Access, AtFlags, CWD, accessat};
    use std::os::unix::fs::PermissionsExt;
    let Ok(metadata) = std::fs::metadata(candidate) else {
        return false;
    };
    if metadata.is_dir() {
        return false;
    }
    match accessat(CWD, candidate, Access::EXEC_OK, AtFlags::EACCESS) {
        Ok(()) => true,
        // Same effective-access fallback as pinned Go findExecutable.
        Err(error) if error == rustix::io::Errno::NOSYS || error == rustix::io::Errno::PERM => {
            metadata.permissions().mode() & 0o111 != 0
        }
        Err(_) => false,
    }
}

#[cfg(windows)]
pub(super) fn batch_error(program: &Path, resolved: &Path) -> Option<String> {
    if explicit(program) {
        return None;
    }
    let extension = resolved.extension()?.as_encoded_bytes();
    if !extension.eq_ignore_ascii_case(b"bat") && !extension.eq_ignore_ascii_case(b"cmd") {
        return None;
    }
    // Go's CreateProcess path fails with ERROR_BAD_EXE_FORMAT. Rust Command
    // would instead run cmd.exe; never add that implicit script owner here.
    let error = std::io::Error::from_raw_os_error(193).to_string();
    let message = error.strip_suffix(" (os error 193)").unwrap_or(&error);
    Some(format!(
        "fork/exec {}: {message}",
        windows::go_text(resolved.as_os_str().as_encoded_bytes())
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn explicit_paths_preserve_raw_bytes_and_relative_separators() {
        use std::os::unix::ffi::OsStrExt;
        for value in [
            b"./symvault".as_slice(),
            b"../raw-\xff/symvault",
            b"/raw-\xff/symvault",
        ] {
            let path = Path::new(OsStr::from_bytes(value));
            assert!(explicit(path));
            assert_eq!(
                executable(path)
                    .unwrap()
                    .unwrap()
                    .owner
                    .as_os_str()
                    .as_bytes(),
                value
            );
        }
        assert!(!explicit(Path::new("symvault")));
    }
}
