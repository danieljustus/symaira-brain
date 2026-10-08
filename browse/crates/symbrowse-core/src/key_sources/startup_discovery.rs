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

/// The path handed to `Command` for a resolved owner. Rust std launches any
/// path not ending in `.exe` as `<path>.exe` when that file exists (an
/// extensionless `symvault`, or `symvault.com` beside `symvault.com.exe`);
/// Go launches exactly the file LookPath chose. A trailing period is the Win32
/// "no extension" spelling: std's probe of the absent `<path>..exe` fails, and
/// CreateProcessW's path normalization drops the period, so the chosen file
/// runs. Explicit paths are unchanged. Verbatim `\\?\` paths skip Win32
/// normalization, so the period would name a different file; they are passed
/// unchanged and keep std's `.exe` probe (a known, documented limitation).
#[cfg(windows)]
pub(super) fn launch_path(program: &Path, resolved: &Executable) -> PathBuf {
    let owner = resolved.owner.as_os_str().as_encoded_bytes();
    let exe = owner.len() >= 4 && owner[owner.len() - 4..].eq_ignore_ascii_case(b".exe");
    if explicit(program) || exe || owner.starts_with(br"\\?\") {
        return resolved.owner.clone();
    }
    let mut spelled = resolved.owner.as_os_str().to_owned();
    spelled.push(".");
    spelled.into()
}

/// The file name as Win32 opens it: trailing periods and spaces are removed by
/// path normalization, so `symvault.bat.` and `symvault.bat ` are batch files.
#[cfg(windows)]
fn win32_extension(path: &Path) -> Option<&[u8]> {
    let name = path.file_name()?.as_encoded_bytes();
    let end = name.iter().rposition(|byte| !matches!(byte, b'.' | b' '))? + 1;
    let name = &name[..end];
    name.iter()
        .rposition(|byte| *byte == b'.')
        .map(|dot| &name[dot + 1..])
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
    let extension = win32_extension(resolved)?;
    if !extension.eq_ignore_ascii_case(b"bat") && !extension.eq_ignore_ascii_case(b"cmd") {
        return None;
    }
    // Intended divergence from Go (docs/adr/daemon-provider-discovery-and-
    // owner-772.md): Go's CreateProcess runs a discovered .bat/.cmd provider
    // through cmd.exe (pinned SDK oracle: invoke_error "", script executed).
    // A key provider must never gain an implicit cmd.exe owner whose argument
    // parsing is the BatBadBut injection class, so the scoped lookup refuses
    // it with the Windows ERROR_BAD_EXE_FORMAT (193) text.
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
