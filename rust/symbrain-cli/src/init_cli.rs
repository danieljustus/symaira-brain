use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use symbrain_core::exit;
use symbrain_core::xdg;

#[cfg(windows)]
const MISSING_HOME_ERR: &str = "%userprofile% is not defined";
#[cfg(not(windows))]
const MISSING_HOME_ERR: &str = "$HOME is not defined";

const DEFAULT_CONFIG_TOML: &str = include_str!("init_templates/default_config.toml");
const PERSONAL_PROFILE_TOML: &str = include_str!("init_templates/personal.toml");
const RESTRICTED_PROFILE_TOML: &str = include_str!("init_templates/restricted.toml");
const FOREIGN_READ_ONLY_PROFILE_TOML: &str = include_str!("init_templates/foreign-read-only.toml");

/// Executes `symbrain init`: creates XDG directories, default config, and example profiles.
pub fn run(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let normalized = crate::normalize_flags(args);
    if let Some(arg) = normalized.first() {
        let arg_str = arg.to_string_lossy();
        if arg_str != "--" && arg_str != "-" && arg_str.starts_with('-') {
            let flag = arg_str.strip_prefix("--").unwrap_or(&arg_str[1..]);
            if flag.is_empty() || flag.starts_with(['-', '=']) {
                let _ = writeln!(stderr, "bad flag syntax: {arg_str}");
                let _ = writeln!(stderr, "Usage of init:");
                return exit::NO_INPUT;
            }
            let (name, _) = flag
                .split_once('=')
                .map_or((flag, None), |(k, v)| (k, Some(v)));
            if name == "h" || name == "help" {
                let _ = writeln!(stderr, "Usage of init:");
                return exit::NO_INPUT;
            }
            let _ = writeln!(stderr, "flag provided but not defined: -{name}");
            let _ = writeln!(stderr, "Usage of init:");
            return exit::NO_INPUT;
        }
    }

    let Some(data_dir) = xdg::data_dir() else {
        let _ = writeln!(stderr, "symbrain init: {MISSING_HOME_ERR}");
        return exit::GENERIC;
    };
    let Some(audit_dir) = xdg::audit_dir() else {
        let _ = writeln!(stderr, "symbrain init: {MISSING_HOME_ERR}");
        return exit::GENERIC;
    };
    let Some(cache_dir) = xdg::cache_dir() else {
        let _ = writeln!(stderr, "symbrain init: {MISSING_HOME_ERR}");
        return exit::GENERIC;
    };

    let dirs = [
        xdg::config_dir(),
        xdg::profiles_dir(),
        data_dir,
        audit_dir,
        cache_dir,
    ]
    .map(native_path);
    for dir in &dirs {
        if let Err(err) = mkdir_all(dir) {
            let _ = writeln!(stderr, "symbrain init: create {}: {err}", dir.display());
            return exit::GENERIC;
        }
    }

    let files = [
        (xdg::config_path(), DEFAULT_CONFIG_TOML),
        (
            xdg::profiles_dir().join("personal.toml"),
            PERSONAL_PROFILE_TOML,
        ),
        (
            xdg::profiles_dir().join("restricted.toml"),
            RESTRICTED_PROFILE_TOML,
        ),
        (
            xdg::profiles_dir().join("foreign-read-only.toml"),
            FOREIGN_READ_ONLY_PROFILE_TOML,
        ),
    ];

    for (path, contents) in files {
        let path = native_path(path);
        match write_if_missing(&path, contents.as_bytes()) {
            Ok(true) => {
                let _ = writeln!(stdout, "created {}", path.display());
            }
            Ok(false) => {
                let _ = writeln!(stdout, "skipped {} (already exists)", path.display());
            }
            Err(err) => {
                let _ = writeln!(stderr, "symbrain init: {}: {err}", path.display());
                return exit::GENERIC;
            }
        }
    }

    exit::OK
}

// PathBuf::join retains separators inside an XDG value, unlike Go's
// filepath.Join. Rebuild Windows components without resolving symlinks.
fn native_path(path: PathBuf) -> PathBuf {
    #[cfg(windows)]
    {
        path.components().collect()
    }
    #[cfg(not(windows))]
    {
        path
    }
}

/// Atomically writes `contents` to `path` unless a file/directory/symlink-target already
/// exists there. Matches Go's `writeIfMissing`.
fn write_if_missing(path: &Path, contents: &[u8]) -> Result<bool, String> {
    match fs::metadata(path) {
        Ok(_) => Ok(false),
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            symbrain_core::config::atomic_write(path, contents).map_err(|err| go_io_error(&err))?;
            Ok(true)
        }
        Err(err) => {
            // Go's Windows stat fallback opens reparse points with CreateFile.
            let operation = if cfg!(windows) && err.raw_os_error() == Some(1921) {
                "CreateFile"
            } else {
                "stat"
            };
            Err(format!(
                "{operation} {}: {}",
                path.display(),
                go_io_error(&err)
            ))
        }
    }
}

// Go's MkdirAll reports the first failing ancestor, not necessarily the
// requested leaf. Keep creation-time permissions and its stat/mkdir fallback.
fn mkdir_all(path: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::metadata(path) {
        return if metadata.is_dir() {
            Ok(())
        } else {
            let reason = if cfg!(windows) {
                go_io_error(&io::Error::from_raw_os_error(3))
            } else {
                "not a directory".to_owned()
            };
            Err(format!("mkdir {}: {reason}", path.display()))
        };
    }
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        mkdir_all(parent)?;
    }
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder
        .create(path)
        .or_else(|error| {
            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
                Ok(())
            } else {
                Err(error)
            }
        })
        .map_err(|error| format!("mkdir {}: {}", path.display(), go_io_error(&error)))
}

fn go_io_error(error: &io::Error) -> String {
    let rendered = error.to_string();
    if error.raw_os_error().is_none() {
        return rendered;
    }
    let message = rendered
        .rsplit_once(" (os error ")
        .map_or(rendered.as_str(), |(text, _)| text);
    #[cfg(windows)]
    {
        message.to_owned()
    }
    #[cfg(not(windows))]
    {
        message.to_ascii_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_flag_preserves_go_diagnostic() {
        for (input, normalized) in [("--=x", "-=x"), ("---=x", "--=x"), ("----x", "---x")] {
            let mut stdout = Vec::new();
            let mut stderr = Vec::new();
            assert_eq!(
                run(&[OsString::from(input)], &mut stdout, &mut stderr),
                exit::NO_INPUT
            );
            assert!(stdout.is_empty());
            assert_eq!(
                stderr,
                format!("bad flag syntax: {normalized}\nUsage of init:\n").as_bytes()
            );
        }
    }

    #[test]
    fn mkdir_collision_reports_first_failing_ancestor() {
        let root = tempfile::tempdir().unwrap();
        let collision = root.path().join("file");
        fs::write(&collision, b"preserve").unwrap();
        let reason = if cfg!(windows) {
            "The system cannot find the path specified."
        } else {
            "not a directory"
        };
        assert_eq!(
            mkdir_all(&collision.join("nested/leaf")),
            Err(format!("mkdir {}: {reason}", collision.display()))
        );
        assert_eq!(fs::read(collision).unwrap(), b"preserve");
    }

    #[cfg(unix)]
    #[test]
    fn symlink_loop_preserves_stat_context() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("loop");
        std::os::unix::fs::symlink("loop", &path).unwrap();
        assert_eq!(
            write_if_missing(&path, b"unchanged"),
            Err(format!(
                "stat {}: too many levels of symbolic links",
                path.display()
            ))
        );
        assert_eq!(fs::read_link(path).unwrap(), Path::new("loop"));
    }

    #[test]
    fn help_flag_outputs_usage() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(&[OsString::from("--help")], &mut stdout, &mut stderr);
        assert_eq!(code, exit::NO_INPUT);
        assert!(stdout.is_empty());
        assert_eq!(stderr, b"Usage of init:\n");

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(&[OsString::from("-h")], &mut stdout, &mut stderr);
        assert_eq!(code, exit::NO_INPUT);
        assert_eq!(stderr, b"Usage of init:\n");
    }

    #[test]
    fn unknown_flag_fails_with_usage() {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let code = run(&[OsString::from("--bogus")], &mut stdout, &mut stderr);
        assert_eq!(code, exit::NO_INPUT);
        assert!(stdout.is_empty());
        assert_eq!(
            stderr,
            b"flag provided but not defined: -bogus\nUsage of init:\n"
        );
    }
}
