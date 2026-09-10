//! Profile template rendering and atomic creation for the CLI.

use std::fs::{self, OpenOptions};
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
/// file modes and using a same-directory atomic rename for the final file.
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
    atomic_write_new(&path, contents.as_bytes())?;
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
        set_dir_mode(path, 0o700)?;
    }
    Ok(())
}

fn set_dir_mode(path: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    let _ = (path, mode);
    Ok(())
}

fn atomic_write_new(path: &Path, contents: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("profile.toml");
    let mut temp_path = None;
    let mut temp_file = None;
    for attempt in 0..100u32 {
        let candidate = parent.join(format!(".{file_name}.{attempt}.tmp"));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => {
                temp_path = Some(candidate);
                temp_file = Some(file);
                break;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(err),
        }
    }
    let temp_path = temp_path.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::AlreadyExists,
            "could not allocate temporary profile file",
        )
    })?;
    let mut file = temp_file.expect("temporary path and file are created together");
    set_mode(&file, 0o600)?;
    file.write_all(contents)?;
    file.sync_all()?;
    drop(file);
    if let Err(err) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(err);
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(file: &std::fs::File, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(fs::Permissions::from_mode(mode))
}

#[cfg(not(unix))]
fn set_mode(_file: &std::fs::File, _mode: u32) -> io::Result<()> {
    Ok(())
}
