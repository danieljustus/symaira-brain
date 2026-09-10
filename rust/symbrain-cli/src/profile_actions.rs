use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use symbrain_core::exit;
use symbrain_core::xdg;
use symbrain_harness::{self, AtomicFile};
use symbrain_policy::profile;

use crate::profile_args::{
    normalize_flags, parse_add_args, parse_remove_args, reorder_flags_first,
};

pub(crate) fn run_add(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let args = reorder_flags_first(args, &["from"]);
    let args = normalize_flags(&args);
    let Ok((name, from)) = parse_add_args(&args, stderr) else {
        return exit::USAGE;
    };
    let name = name.to_string_lossy();
    let from = from.to_string_lossy();

    if let Err(error) = profile::validate_name(&name) {
        let _ = writeln!(
            stderr,
            "symbrain profile add: {}",
            validation_message(error)
        );
        return exit::NO_INPUT;
    }
    let path = profile::path(&name);
    if profile::exists(&name) {
        let _ = writeln!(
            stderr,
            "symbrain profile add: profile {:?} already exists ({})",
            name,
            path.display()
        );
        return exit::NO_INPUT;
    }
    if let Err(message) = symbrain_policy::profile::create::render_template(&from, &name) {
        let _ = writeln!(stderr, "symbrain profile add: {message}");
        return exit::NO_INPUT;
    }
    if let Err(error) =
        symbrain_policy::profile::create::create_in(&xdg::profiles_dir(), &name, &from)
    {
        let _ = writeln!(stderr, "symbrain profile add: {error}");
        return exit::GENERIC;
    }
    let _ = writeln!(stdout, "created {} (from {})", path.display(), from);
    exit::OK
}

#[allow(clippy::too_many_lines)]
pub(crate) fn run_remove(args: &[OsString], stdout: &mut dyn Write, stderr: &mut dyn Write) -> u8 {
    let args = reorder_flags_first(args, &["project"]);
    let args = normalize_flags(&args);
    let Ok(parsed) = parse_remove_args(&args, stderr) else {
        return exit::USAGE;
    };
    let name = parsed.name.to_string_lossy().into_owned();
    if let Err(error) = profile::validate_name(&name) {
        let _ = writeln!(
            stderr,
            "symbrain profile remove: {}",
            validation_message(error)
        );
        return exit::NO_INPUT;
    }

    let profiles_dir = xdg::profiles_dir();
    let path = profiles_dir.join(format!("{name}.toml"));
    let file_name = format!("{name}.toml");
    let capability =
        match AtomicFile::open(&profiles_dir, Path::new(&file_name), path.clone(), false) {
            Ok(capability) => capability,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return report_missing_profile(&name, stderr);
            }
            Err(error) => {
                let detail = profile_remove_open_error(&profiles_dir, &error);
                let _ = writeln!(
                    stderr,
                    "symbrain profile remove: remove {}: {detail}",
                    path.display()
                );
                return exit::GENERIC;
            }
        };
    // The existence probe is intentionally no-follow: a symlink or special
    // file is still a removable directory entry, and the final deletion is
    // performed through the retained parent capability.
    match capability.exists_no_follow() {
        Ok(true) => {}
        Ok(false) | Err(_) => return report_missing_profile(&name, stderr),
    }

    let project = parsed
        .project
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .map(|path| clean_project_path(&path));
    let bindings = symbrain_harness::profile_bindings(&name, project.as_deref());
    if !parsed.force && (!bindings.errors.is_empty() || !bindings.bindings.is_empty()) {
        if !bindings.errors.is_empty() {
            let _ = writeln!(
                stderr,
                "symbrain profile remove: unable to safely inspect harness bindings:"
            );
            for error in &bindings.errors {
                let _ = writeln!(
                    stderr,
                    "  - {} ({}): {}",
                    error.harness, error.path, error.error
                );
            }
        }
        if !bindings.bindings.is_empty() {
            let _ = writeln!(
                stderr,
                "symbrain profile remove: profile {name:?} is still bound to harnesses:"
            );
            for binding in &bindings.bindings {
                let _ = writeln!(stderr, "  - {} ({})", binding.harness, binding.path);
            }
        }
        let _ = writeln!(stderr, "Refusing to remove. Use --force to override.");
        return exit::GENERIC;
    }

    if !parsed.force {
        if write!(
            stdout,
            "Remove profile {name:?} ({})? [y/N]: ",
            path.display()
        )
        .and_then(|()| stdout.flush())
        .is_err()
        {
            return exit::GENERIC;
        }
        let mut stdin = io::stdin().lock();
        let answer = match read_confirmation(&mut stdin) {
            Ok(answer) => String::from_utf8_lossy(&answer).trim().to_ascii_lowercase(),
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                let _ = writeln!(
                    stderr,
                    "symbrain profile remove: confirmation input exceeds maximum of {MAX_CONFIRMATION_BYTES} bytes"
                );
                return exit::GENERIC;
            }
            Err(_) => {
                let _ = writeln!(
                    stderr,
                    "symbrain profile remove: unable to read confirmation input"
                );
                return exit::GENERIC;
            }
        };
        if answer != "y" && answer != "yes" {
            let _ = writeln!(stdout, "aborted");
            return exit::OK;
        }
    }

    match capability.remove_no_follow() {
        Ok(true) => {
            let _ = writeln!(stdout, "removed {}", path.display());
            exit::OK
        }
        Ok(false) => {
            let _ = writeln!(
                stderr,
                "symbrain profile remove: remove {}: no such file or directory",
                path.display()
            );
            exit::GENERIC
        }
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain profile remove: remove {}: {error}",
                path.display()
            );
            exit::GENERIC
        }
    }
}

const MAX_CONFIRMATION_BYTES: usize = 1024;

fn read_confirmation(reader: &mut impl Read) -> io::Result<Vec<u8>> {
    let mut input = Vec::with_capacity(MAX_CONFIRMATION_BYTES.min(64));
    for _ in 0..=MAX_CONFIRMATION_BYTES {
        let mut byte = [0_u8; 1];
        let count = reader.read(&mut byte)?;
        if count == 0 {
            return Ok(input);
        }
        if byte[0] == b'\n' {
            return Ok(input);
        }
        input.push(byte[0]);
        if input.len() > MAX_CONFIRMATION_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "confirmation input exceeds maximum",
            ));
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "confirmation input exceeds maximum",
    ))
}

fn clean_project_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => match result.components().next_back() {
                Some(std::path::Component::Normal(_)) => {
                    result.pop();
                }
                Some(std::path::Component::ParentDir) | None => result.push(".."),
                _ => {}
            },
            component => result.push(component.as_os_str()),
        }
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

fn profile_remove_open_error(profiles_dir: &Path, error: &io::Error) -> String {
    if error.kind() != io::ErrorKind::NotADirectory {
        return error.to_string();
    }
    let mut current = Path::new(if profiles_dir.is_absolute() { "/" } else { "." }).to_path_buf();
    for component in profiles_dir.components() {
        let std::path::Component::Normal(name) = component else {
            continue;
        };
        current.push(name);
        match std::fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => {
            }
            Ok(_) => {
                return format!(
                    "open profile directory component {:?}: not a directory",
                    name.to_string_lossy()
                );
            }
            Err(_) => break,
        }
    }
    error.to_string()
}

fn report_missing_profile(name: &str, stderr: &mut dyn Write) -> u8 {
    let _ = writeln!(
        stderr,
        "symbrain profile remove: profile {name:?} does not exist"
    );
    exit::NO_INPUT
}

fn validation_message(error: symbrain_policy::ProfileError) -> String {
    match error {
        symbrain_policy::ProfileError::InvalidName { message, .. } => message,
        other => other.to_string(),
    }
}
