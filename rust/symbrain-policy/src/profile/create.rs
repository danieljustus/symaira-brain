//! Profile template rendering and atomic creation for the CLI.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::profile::validate::validate_name;

/// The byte-for-byte personal profile template used by `symbrain init`.
pub const PERSONAL_TEMPLATE: &str = r#"# Example profile: full access for trusted personal use.
[profile]
name        = "personal"
description = "Full access for trusted personal use"

[servers.vault]
enabled = true
mode    = "full"

[servers.memory]
enabled = true
mode    = "read_write"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[audit]
enabled = true
"#;

/// The byte-for-byte restricted profile template used by `symbrain init`.
pub const RESTRICTED_TEMPLATE: &str = r#"# Example profile: least-privilege for untrusted or shared harnesses.
[profile]
name        = "restricted"
description = "Least-privilege profile for untrusted or shared harnesses"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[audit]
enabled = true
"#;

/// Renders a supported template with its profile name replaced.
///
/// # Errors
///
/// Returns an error when `from` is not a supported template or the template
/// does not contain the expected name field.
pub fn render_template(from: &str, name: &str) -> Result<String, String> {
    let template = match from {
        "personal" => PERSONAL_TEMPLATE,
        "restricted" => RESTRICTED_TEMPLATE,
        _ => {
            return Err(format!(
                "--from must be \"personal\" or \"restricted\", got {from:?}"
            ));
        }
    };
    let marker = match from {
        "personal" => "name        = \"personal\"",
        "restricted" => "name        = \"restricted\"",
        _ => unreachable!(),
    };
    let replacement = format!("name        = \"{name}\"");
    if !template.contains(marker) {
        return Err(format!(
            "internal error: {from:?} template has no [profile] name field to rewrite"
        ));
    }
    Ok(template.replacen(marker, &replacement, 1))
}

/// Creates a profile below `dir`, preserving the Go command's directory and
/// file modes and atomically publishing without replacing an existing file.
///
/// # Errors
///
/// Returns an I/O error if validation, directory creation, or atomic writing fails.
pub fn create_in(dir: &Path, name: &str, from: &str) -> io::Result<PathBuf> {
    validate_name(name)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err.to_string()))?;
    let path = dir.join(format!("{name}.toml"));
    if path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("profile {name:?} already exists ({})", path.display()),
        ));
    }
    let contents = render_template(from, name)
        .map_err(|message| io::Error::new(io::ErrorKind::InvalidInput, message))?;
    create_dirs_secure(dir)?;
    atomic_write_new(&path, contents.as_bytes()).map_err(|err| {
        if err.kind() == io::ErrorKind::AlreadyExists {
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("profile {name:?} already exists ({})", path.display()),
            )
        } else {
            err
        }
    })?;
    Ok(path)
}

fn create_dirs_secure(dir: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut cursor = Some(dir);
    while let Some(path) = cursor {
        if path.exists() {
            break;
        }
        missing.push(path);
        cursor = path.parent();
    }
    fs::create_dir_all(dir)?;
    for path in missing {
        #[cfg(unix)]
        set_dir_mode(path, 0o700)?;
        #[cfg(not(unix))]
        set_dir_mode(path, 0o700);
    }
    Ok(())
}

#[cfg(unix)]
fn set_dir_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_dir_mode(_path: &Path, _mode: u32) {}

fn atomic_write_new(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    // Each writer owns a fresh name. Reusing deterministic temporary slots can
    // encounter Windows delete-pending files from another concurrent writer.
    let mut file = tempfile::Builder::new()
        .prefix(".profile-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    #[cfg(unix)]
    set_mode(file.as_file(), 0o600)?;
    #[cfg(not(unix))]
    set_mode(file.as_file(), 0o600);
    file.write_all(contents)?;
    file.as_file().sync_all()?;
    // Platform no-replace publication preserves the winner's complete bytes.
    // On any earlier/publication error, the owned temporary file is dropped.
    file.persist_noclobber(path)
        .map(|_| ())
        .map_err(|error| error.error)
}

#[cfg(unix)]
fn set_mode(file: &std::fs::File, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_file: &std::fs::File, _mode: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;

    #[test]
    fn concurrent_creators_never_clobber_each_other() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        for round in 0..32 {
            let dir = temp_dir.path().join(format!("profiles-{round}"));
            let barrier = std::sync::Barrier::new(8);
            let results: Vec<_> = std::thread::scope(|scope| {
                let handles: Vec<_> = (0..8)
                    .map(|i| {
                        let dir = &dir;
                        let from = if i % 2 == 0 { "personal" } else { "restricted" };
                        let barrier = &barrier;
                        scope.spawn(move || {
                            barrier.wait();
                            create_in(dir, "race", from).map(|_| from)
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap()).collect()
            });
            let winners: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
            assert_eq!(winners.len(), 1, "{results:?}");
            for loser in results.iter().filter_map(|r| r.as_ref().err()) {
                assert_eq!(loser.kind(), io::ErrorKind::AlreadyExists);
            }
            let written = fs::read_to_string(dir.join("race.toml")).expect("profile");
            assert_eq!(written, render_template(winners[0], "race").unwrap());
            let leftovers = fs::read_dir(&dir).unwrap().count();
            assert_eq!(leftovers, 1, "temporary files must be cleaned up");
            println!(
                "PROFILE_CREATE_THREAD_RACE {}",
                serde_json::json!({
                    "round": round, "creators": 8,
                    "results": results.iter().map(|result| match result {
                        Ok(from) => format!("winner:{from}"),
                        Err(error) => format!("error:{:?}", error.kind()),
                    }).collect::<Vec<_>>(),
                    "winner_template": winners[0], "winner_bytes": written,
                    "remaining_profile_files": leftovers
                })
            );
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = fs::metadata(dir.join("race.toml"))
                    .unwrap()
                    .permissions()
                    .mode();
                assert_eq!(mode & 0o777, 0o600);
            }
        }
    }

    #[test]
    fn failed_publication_cleans_up_only_its_owned_temporary_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("winner.toml");
        fs::write(&path, b"existing winner").unwrap();
        assert_eq!(
            atomic_write_new(&path, b"loser bytes").unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read(&path).unwrap(), b"existing winner");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);

        let directory = root.path().join("directory.toml");
        fs::create_dir(&directory).unwrap();
        assert!(atomic_write_new(&directory, b"never published").is_err());
        assert!(directory.is_dir());
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn unix_set_dir_mode_sets_0700() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).expect("create_dir");

        set_dir_mode(&dir_path, 0o700).expect("set_dir_mode");

        let metadata = fs::metadata(&dir_path).expect("metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
    }

    #[cfg(unix)]
    #[test]
    fn unix_set_mode_sets_0600() {
        use std::os::unix::fs::PermissionsExt;
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let file_path = temp_dir.path().join("file.txt");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
            .expect("open");

        set_mode(&file, 0o600).expect("set_mode");

        let metadata = fs::metadata(&file_path).expect("metadata");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn unix_set_dir_mode_propagates_error() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let non_existent = temp_dir.path().join("does_not_exist");

        let err = set_dir_mode(&non_existent, 0o700).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    #[cfg(not(unix))]
    #[test]
    fn non_unix_mode_helpers_are_noops() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let dir_path = temp_dir.path().join("subdir");
        fs::create_dir(&dir_path).expect("create_dir");
        let file_path = temp_dir.path().join("file.txt");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file_path)
            .expect("open");

        set_dir_mode(&dir_path, 0o700);
        set_mode(&file, 0o600);
    }
}
