//! Bounded managed-version probes with Go-compatible Windows path resolution.
use crate::ManagedError;
use serde::Deserialize;
use std::fs;
use std::path::Path;
#[cfg(any(windows, test))]
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Deserialize)]
struct VersionPayload {
    version: String,
}

/// Probes `<binary> version --json`, returning an empty string when absent.
///
/// # Errors
/// Returns an error when the process cannot start, times out, exits non-zero,
/// or emits malformed JSON.
pub fn installed_version(bin_dir: &Path, binary_name: &str) -> Result<String, ManagedError> {
    let path = bin_dir.join(binary_name);
    if !path.exists() {
        return Ok(String::new());
    }

    #[cfg(windows)]
    let path =
        windows_probe_path(&path, std::env::var("PATHEXT").ok().as_deref()).ok_or_else(|| {
            ManagedError::Process(format!(
                "probe {binary_name}: executable file not found in %PATH%"
            ))
        })?;

    let output = tempfile::NamedTempFile::new()
        .map_err(|error| ManagedError::IoContext("create version output".to_string(), error))?;
    let output_writer = output
        .reopen()
        .map_err(|error| ManagedError::IoContext("open version output".to_string(), error))?;
    let mut command = Command::new(&path);
    command
        .arg("version")
        .arg("--json")
        .stdin(Stdio::null())
        .stdout(Stdio::from(output_writer))
        .stderr(Stdio::null());
    configure_probe_process(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| ManagedError::Process(format!("probe {binary_name}: {error}")))?;
    let process_group = child.id();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                terminate_probe_descendants(process_group);
                let bytes = fs::read(output.path()).map_err(ManagedError::Io)?;
                if !status.success() {
                    return Err(ManagedError::Process(format!(
                        "probe {binary_name}: process exited with {status}"
                    )));
                }
                let payload: VersionPayload = serde_json::from_slice(&bytes).map_err(|error| {
                    ManagedError::Process(format!("parse {binary_name} version: {error}"))
                })?;
                return Ok(payload.version);
            }
            Ok(None) if started.elapsed() < VERSION_PROBE_TIMEOUT => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                terminate_probe(&mut child, process_group);
                return Err(ManagedError::Process(format!(
                    "probe {binary_name}: timed out"
                )));
            }
            Err(error) => {
                terminate_probe(&mut child, process_group);
                return Err(ManagedError::Process(format!(
                    "probe {binary_name}: {error}"
                )));
            }
        }
    }
}

#[cfg(unix)]
fn configure_probe_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_probe_process(_command: &mut Command) {}

fn terminate_probe(child: &mut std::process::Child, process_group: u32) {
    #[cfg(unix)]
    {
        signal_probe_group(process_group, "-TERM");
        let deadline = Instant::now() + Duration::from_millis(250);
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                terminate_probe_descendants(process_group);
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        terminate_probe_descendants(process_group);
        let _ = child.wait();
    }
    #[cfg(not(unix))]
    {
        let _ = process_group;
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(unix)]
fn terminate_probe_descendants(process_group: u32) {
    signal_probe_group(process_group, "-KILL");
}

#[cfg(not(unix))]
fn terminate_probe_descendants(_process_group: u32) {}

#[cfg(unix)]
fn signal_probe_group(process_group: u32, signal: &str) {
    let _ = Command::new("/bin/kill")
        .arg(signal)
        .arg("--")
        .arg(format!("-{process_group}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// Go checks the original file before exec.Command resolves PATHEXT. Windows
// CreateProcess accepts an extensionless PE directly; Go's default lookup does
// not. Preserve that distinction, including the explicit all-empty PATHEXT case.
#[cfg(any(windows, test))]
fn windows_probe_path(path: &Path, raw_extensions: Option<&str>) -> Option<PathBuf> {
    let extensions: Vec<String> = raw_extensions.filter(|raw| !raw.is_empty()).map_or_else(
        || vec![".com".into(), ".exe".into(), ".bat".into(), ".cmd".into()],
        |raw| {
            raw.to_lowercase()
                .split(';')
                .filter(|ext| !ext.is_empty())
                .map(|ext| {
                    if ext.starts_with('.') {
                        ext.into()
                    } else {
                        format!(".{ext}")
                    }
                })
                .collect()
        },
    );
    let filename = path.file_name()?.to_string_lossy();
    if let Some((_, suffix)) = filename.rsplit_once('.')
        && extensions
            .iter()
            .any(|ext| ext.eq_ignore_ascii_case(&format!(".{suffix}")))
    {
        // Go assumes known executable extensions have already been resolved.
        return Some(path.into());
    }
    if (extensions.is_empty() || filename.contains('.')) && path.is_file() {
        return Some(path.into());
    }
    extensions.into_iter().find_map(|extension| {
        let mut candidate = path.as_os_str().to_os_string();
        candidate.push(extension);
        let candidate = PathBuf::from(candidate);
        candidate.is_file().then_some(candidate)
    })
}

#[cfg(test)]
mod tests {
    use super::windows_probe_path;
    #[test]
    fn extensionless_pe_is_not_executed_without_a_pathext_candidate() {
        let root = tempfile::tempdir().expect("fixture root");
        let raw = root.path().join("probe");
        std::fs::write(&raw, b"extensionless fixture").expect("raw file");
        assert_eq!(windows_probe_path(&raw, None), None);
        assert_eq!(windows_probe_path(&raw, Some("")), None);
        assert_eq!(windows_probe_path(&raw, Some(";;")), Some(raw.clone()));
        let exe = root.path().join("probe.exe");
        std::fs::write(&exe, b"exe fixture").expect("exe file");
        assert_eq!(windows_probe_path(&raw, None), Some(exe.clone()));
        assert_eq!(windows_probe_path(&raw, Some("EXE;CMD")), Some(exe.clone()));
        assert_eq!(windows_probe_path(&exe, None), Some(exe));
        let com = root.path().join("probe.com");
        std::fs::write(&com, b"com fixture").expect("com file");
        assert_eq!(windows_probe_path(&raw, None), Some(com));
        let odd = root.path().join("probe.other");
        std::fs::write(&odd, b"odd extension").expect("odd file");
        assert_eq!(windows_probe_path(&odd, None), Some(odd));
    }
}
