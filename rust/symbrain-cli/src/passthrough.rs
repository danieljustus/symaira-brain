//! Native passthroughs for external Symaira core binaries.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use symbrain_core::config::format_go_quoted;
use symbrain_core::exit;
use symbrain_core::xdg;

const VAULT_BINARY: &str = "symvault";
const VAULT_BINARY_ENV: &str = "SYMBRAIN_SERVERS_VAULT_BINARY_PATH";

#[derive(Debug)]
pub(super) enum DiscoveryError {
    Missing,
    Unusable(PathBuf),
    Configured {
        path: PathBuf,
        error: std::io::Error,
    },
}

/// Executes `symbrain vault` without parsing or rewriting any child argument.
///
/// On Unix and Windows the child keeps inherited stdio and the returned status
/// mirrors the Go oracle, including its 255 mapping for a signaled child.
pub fn run(args: &[OsString], stderr: &mut dyn Write) -> u8 {
    let binary = match discover_vault_binary() {
        Ok(path) => path,
        Err(DiscoveryError::Missing) => {
            let _ = writeln!(
                stderr,
                "symbrain vault: broker: \"{VAULT_BINARY}\" not found on PATH or in managed directory: exec: \"{VAULT_BINARY}\": {}\nHint: install {VAULT_BINARY} or run `symbrain setup`.",
                path_not_found_message()
            );
            return exit::GENERIC;
        }
        Err(DiscoveryError::Configured { path, error }) => {
            write_configured_error(stderr, "symbrain vault", &path, &error);
            let _ = writeln!(
                stderr,
                "Hint: install {VAULT_BINARY} or run `symbrain setup`."
            );
            return exit::GENERIC;
        }
        Err(DiscoveryError::Unusable(path)) => {
            write_unusable(stderr, "symbrain vault", &path);
            let _ = writeln!(
                stderr,
                "Hint: install {VAULT_BINARY} or run `symbrain setup`."
            );
            return exit::GENERIC;
        }
    };

    let child_args = args.get(1..).unwrap_or_default();
    execute_child(&binary, child_args, stderr)
}

pub(super) fn admin_binary(action: &str, stderr: &mut dyn Write) -> Option<PathBuf> {
    match discover_vault_binary() {
        Ok(path) => Some(path),
        Err(DiscoveryError::Missing) => {
            let _ = writeln!(
                stderr,
                "symbrain vault {action}: broker: \"{VAULT_BINARY}\" not found on PATH or in managed directory: exec: \"{VAULT_BINARY}\": {}",
                path_not_found_message()
            );
            None
        }
        Err(DiscoveryError::Configured { path, error }) => {
            write_configured_error(stderr, &format!("symbrain vault {action}"), &path, &error);
            None
        }
        Err(DiscoveryError::Unusable(path)) => {
            write_unusable(stderr, &format!("symbrain vault {action}"), &path);
            None
        }
    }
}

pub(super) fn discover_vault_binary() -> Result<PathBuf, DiscoveryError> {
    let config_path = xdg::config_path();
    let configured = crate::vault_config::configured_override(&config_path, VAULT_BINARY_ENV);

    if let Some(path) = configured {
        return match fs::metadata(&path) {
            Err(error) => Err(DiscoveryError::Configured { path, error }),
            Ok(_) if !is_executable_file(&path) => Err(DiscoveryError::Unusable(path)),
            Ok(_) => Ok(path),
        };
    }

    if let Some(home) = xdg::home_dir() {
        let directory = home.join(".symaira").join("bin");
        let managed = directory.join(VAULT_BINARY);
        if is_executable_file(&managed) {
            return Ok(managed);
        }
        if cfg!(windows)
            && let Some(path) = path_lookup_in(OsStr::new(VAULT_BINARY), [directory])
        {
            return Ok(path);
        }
    }

    path_lookup(OsStr::new(VAULT_BINARY)).ok_or(DiscoveryError::Missing)
}

fn write_unusable(stderr: &mut dyn Write, context: &str, path: &Path) {
    let _ = writeln!(
        stderr,
        "{context}: broker: configured binary_path {} for \"{VAULT_BINARY}\" is not an executable regular file",
        format_go_quoted(path.as_os_str())
    );
}

fn write_configured_error(
    stderr: &mut dyn Write,
    context: &str,
    path: &Path,
    error: &std::io::Error,
) {
    #[cfg(windows)]
    let operation = if error.kind() == std::io::ErrorKind::NotFound {
        "GetFileAttributesEx"
    } else {
        "CreateFile"
    };
    #[cfg(not(windows))]
    let operation = "stat";
    let _ = write!(
        stderr,
        "{context}: broker: configured binary_path {} for \"{VAULT_BINARY}\": {operation} ",
        format_go_quoted(path.as_os_str()),
    );
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let _ = stderr.write_all(path.as_os_str().as_bytes());
    }
    #[cfg(not(unix))]
    {
        let _ = write!(stderr, "{}", path.display());
    }
    let _ = writeln!(stderr, ": {}", go_error(error));
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn path_lookup(binary: &OsStr) -> Option<PathBuf> {
    let paths = env::var_os("PATH")?;
    path_lookup_in(binary, env::split_paths(&paths))
}

fn path_lookup_in<I>(binary: &OsStr, directories: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    let names = executable_names(binary);
    for directory in directories {
        for name in &names {
            let candidate = directory.join(name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn executable_names(binary: &OsStr) -> Vec<OsString> {
    #[cfg(windows)]
    {
        windows_executable_names(
            &binary.to_string_lossy(),
            env::var("PATHEXT").ok().as_deref(),
        )
    }
    #[cfg(not(windows))]
    {
        vec![binary.to_os_string()]
    }
}

#[cfg(any(windows, test))]
fn windows_executable_names(binary: &str, path_ext: Option<&str>) -> Vec<OsString> {
    let extensions = path_ext
        .filter(|value| !value.is_empty())
        .unwrap_or(".com;.exe;.bat;.cmd")
        .split(';')
        .filter(|extension| !extension.is_empty())
        .map(|extension| {
            let extension = extension.to_ascii_lowercase();
            if extension.starts_with('.') {
                extension
            } else {
                format!(".{extension}")
            }
        })
        .collect::<Vec<_>>();
    if extensions.iter().any(|extension| {
        binary
            .to_ascii_lowercase()
            .ends_with(&extension.to_ascii_lowercase())
    }) {
        vec![OsString::from(binary)]
    } else {
        extensions
            .iter()
            .map(|extension| OsString::from(format!("{binary}{extension}")))
            .collect()
    }
}

const fn path_not_found_message() -> &'static str {
    if cfg!(windows) {
        "executable file not found in %PATH%"
    } else {
        "executable file not found in $PATH"
    }
}

fn child_exit_code(status: std::process::ExitStatus) -> u8 {
    status.code().map_or(u8::MAX, |code| {
        u8::try_from(code.rem_euclid(256)).unwrap_or(exit::GENERIC)
    })
}

#[cfg(unix)]
fn execute_child(path: &Path, args: &[OsString], stderr: &mut dyn Write) -> u8 {
    match Command::new(path)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
    {
        // Go's exec.ExitError.ExitCode() returns -1 for a signaled process;
        // converting that to exitcodes.ExitCode yields 255.
        Ok(status) => child_exit_code(status),
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain vault: fork/exec {}: {}",
                path.display(),
                go_error(&error)
            );
            exit::GENERIC
        }
    }
}

#[cfg(not(unix))]
fn execute_child(path: &Path, args: &[OsString], stderr: &mut dyn Write) -> u8 {
    match Command::new(path)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
    {
        Ok(status) => child_exit_code(status),
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain vault: failed to start {}: {}",
                path.display(),
                go_error(&error)
            );
            exit::GENERIC
        }
    }
}

fn go_error(error: &std::io::Error) -> String {
    #[cfg(windows)]
    {
        let rendered = error.to_string();
        rendered
            .rsplit_once(" (os error ")
            .map_or(rendered.clone(), |(message, _)| message.to_string())
    }
    #[cfg(not(windows))]
    match error.kind() {
        std::io::ErrorKind::NotFound => "no such file or directory".to_string(),
        std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
        std::io::ErrorKind::AlreadyExists => "file exists".to_string(),
        _ => error.to_string(),
    }
}

#[cfg(test)]
#[path = "passthrough_tests.rs"]
mod tests;
