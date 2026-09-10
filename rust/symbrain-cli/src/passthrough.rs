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
use toml_edit::{DocumentMut, Item, Value};

const VAULT_BINARY: &str = "symvault";
const VAULT_BINARY_ENV: &str = "SYMBRAIN_SERVERS_VAULT_BINARY_PATH";

#[derive(Debug)]
enum DiscoveryError {
    Missing,
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
            write_configured_error(stderr, &path, &error);
            return exit::GENERIC;
        }
    };

    let child_args = args.get(1..).unwrap_or_default();
    execute_child(&binary, child_args, stderr)
}

fn discover_vault_binary() -> Result<PathBuf, DiscoveryError> {
    let config_path = xdg::config_path();
    let configured = env::var_os(VAULT_BINARY_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| configured_binary_path(&config_path));

    if let Some(path) = configured {
        return fs::metadata(&path)
            .map(|_| path.clone())
            .map_err(|error| DiscoveryError::Configured { path, error });
    }

    if let Some(home) = xdg::home_dir() {
        let managed = home.join(".symaira").join("bin").join(VAULT_BINARY);
        if is_executable_file(&managed) {
            return Ok(managed);
        }
    }

    path_lookup(OsStr::new(VAULT_BINARY)).ok_or(DiscoveryError::Missing)
}

fn configured_binary_path(path: &Path) -> Option<PathBuf> {
    let contents = fs::read_to_string(path).ok()?;
    let document: DocumentMut = contents.parse().ok()?;
    let servers = document.get("servers")?.as_table_like()?;
    let vault = servers.get("vault")?.as_table_like()?;
    match vault.get("binary_path")? {
        Item::Value(Value::String(value)) if !value.value().is_empty() => {
            Some(PathBuf::from(value.value().as_str()))
        }
        _ => None,
    }
}

fn write_configured_error(stderr: &mut dyn Write, path: &Path, error: &std::io::Error) {
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
        "symbrain vault: broker: configured binary_path {} for \"{VAULT_BINARY}\": {operation} ",
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
    let _ = writeln!(
        stderr,
        ": {}\nHint: install {VAULT_BINARY} or run `symbrain setup`.",
        go_error(error)
    );
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
        return rendered
            .rsplit_once(" (os error ")
            .map_or(rendered.clone(), |(message, _)| message.to_string());
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
mod tests {
    use super::*;

    #[cfg(unix)]
    fn executable(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        fs::write(path, body).expect("write executable");
        let mut permissions = fs::metadata(path).expect("stat executable").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod executable");
    }

    #[cfg(not(unix))]
    fn executable(path: &Path, body: &str) {
        fs::write(path, body).expect("write executable");
    }

    #[test]
    fn config_binary_path_is_read_from_nested_table() {
        let root = tempfile::tempdir().expect("tempdir");
        let config = root.path().join("config.toml");
        fs::write(
            &config,
            "[servers.vault]\nbinary_path = \"/custom/symvault\"\n",
        )
        .expect("write config");
        assert_eq!(
            configured_binary_path(&config),
            Some(PathBuf::from("/custom/symvault"))
        );
    }

    #[test]
    fn empty_config_binary_path_is_not_an_override() {
        let root = tempfile::tempdir().expect("tempdir");
        let config = root.path().join("config.toml");
        fs::write(&config, "[servers.vault]\nbinary_path = \"\"\n").expect("write config");
        assert_eq!(configured_binary_path(&config), None);
    }

    #[cfg(unix)]
    #[test]
    fn configured_path_diagnostic_preserves_non_utf8_bytes() {
        use std::os::unix::ffi::OsStringExt;

        let path = PathBuf::from(OsString::from_vec(b"/tmp/missing_\xff".to_vec()));
        let mut stderr = Vec::new();
        write_configured_error(
            &mut stderr,
            &path,
            &std::io::Error::from(std::io::ErrorKind::NotFound),
        );
        assert!(
            stderr
                .windows(b"\"/tmp/missing_\\xff\"".len())
                .any(|bytes| { bytes == b"\"/tmp/missing_\\xff\"" })
        );
        assert!(
            stderr
                .windows(b"stat /tmp/missing_\xff:".len())
                .any(|bytes| { bytes == b"stat /tmp/missing_\xff:" })
        );
    }

    #[test]
    fn managed_binary_requires_regular_executable_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let binary = root.path().join(VAULT_BINARY);
        executable(&binary, "");
        assert!(is_executable_file(&binary));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = fs::metadata(&binary).expect("stat binary").permissions();
            permissions.set_mode(0o644);
            fs::set_permissions(&binary, permissions).expect("remove executable bit");
            assert!(!is_executable_file(&binary));
        }
    }

    #[test]
    fn path_lookup_finds_binary_in_ordered_path_entry() {
        let root = tempfile::tempdir().expect("tempdir");
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir_all(&first).expect("first dir");
        fs::create_dir_all(&second).expect("second dir");
        executable(&second.join(VAULT_BINARY), "");

        let found = path_lookup_in(OsStr::new(VAULT_BINARY), vec![first, second.clone()]);
        assert_eq!(found, Some(second.join(VAULT_BINARY)));
        assert!(is_executable_file(&second.join(VAULT_BINARY)));
    }

    #[test]
    fn windows_path_extensions_match_go_defaults() {
        assert_eq!(
            windows_executable_names("symvault", Some("")),
            [
                "symvault.com",
                "symvault.exe",
                "symvault.bat",
                "symvault.cmd"
            ]
            .map(OsString::from)
        );
        assert_eq!(
            windows_executable_names("symvault.EXE", Some(".COM;.EXE")),
            vec![OsString::from("symvault.EXE")]
        );
        assert_eq!(
            windows_executable_names("symvault", Some("COM;Exe")),
            vec![
                OsString::from("symvault.com"),
                OsString::from("symvault.exe")
            ]
        );
    }
}
