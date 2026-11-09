//! Bounded managed-version probes with Go-compatible Windows path resolution.
use crate::ManagedError;
use crate::process_status;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
const VERSION_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Probes `<binary> version --json`, returning an empty string when absent.
///
/// # Errors
/// Returns an error when the process cannot start, times out, exits non-zero,
/// or emits malformed JSON.
pub fn installed_version(bin_dir: &Path, binary_name: &str) -> Result<String, ManagedError> {
    let path = bin_dir.join(binary_name);
    let stat_error = match fs::metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        result => result.err(),
    };

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
    let mut child = command.spawn().map_err(|error| {
        // Go reports execve's errno. macOS 15 posix_spawn (used by std on
        // Apple) reports ENOENT for a non-directory path component where
        // execve and stat report ENOTDIR, so prefer the stat path error.
        let error = match stat_error {
            Some(stat_error) if error.kind() == std::io::ErrorKind::NotFound => stat_error,
            _ => error,
        };
        ManagedError::RawContext(crate::GoText::path(
            &format!("probe {binary_name}: fork/exec "),
            &path,
            &format!(": {}", crate::provenance::go_io_error(&error)),
        ))
    })?;
    let process_group = child.id();
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                terminate_probe_descendants(process_group);
                let bytes = fs::read(output.path()).map_err(ManagedError::Io)?;
                if !status.success() {
                    let status = process_status::format(status);
                    return Err(ManagedError::Context(format!(
                        "probe {binary_name}: {status}"
                    )));
                }
                return crate::json_record::version(&bytes).map_err(|error| {
                    ManagedError::Context(format!("parse {binary_name} version: {error}"))
                });
            }
            Ok(None) if started.elapsed() < VERSION_PROBE_TIMEOUT => {
                thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                terminate_probe(&mut child, process_group);
                let status = "signal: killed";
                return Err(ManagedError::Context(format!(
                    "probe {binary_name}: {status}"
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

fn configure_probe_process(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

fn terminate_probe(child: &mut std::process::Child, process_group: u32) {
    {
        // Go CommandContext kills the child immediately at its deadline.
        // Also kill the owned process group so descendants cannot retain a
        // capture descriptor or survive the bounded probe.
        terminate_probe_descendants(process_group);
        let _ = child.wait();
    }
}

fn terminate_probe_descendants(process_group: u32) {
    signal_probe_group(process_group, "-KILL");
}

fn signal_probe_group(process_group: u32, signal: &str) {
    let _ = Command::new("/bin/kill")
        .arg(signal)
        .arg("--")
        .arg(format!("-{process_group}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(test)]
mod tests {
    #[test]
    fn file_bin_dir_reports_execve_not_a_directory() {
        let root = tempfile::tempdir().expect("fixture root");
        let bin = root.path().join("bin");
        std::fs::write(&bin, b"owned obstruction").expect("obstruction");
        let error = super::installed_version(&bin, "symvault").expect_err("probe fails");
        assert!(error.to_string().ends_with(": not a directory"), "{error}");
    }
}
