// Native provider commands compatibility implementation.

/// Why a bounded child command did not yield usable stdout.
enum CommandFailure {
    NotFound,
    TimedOut,
    ExitFailed { code: Option<i32>, stderr: Vec<u8> },
    Other(String),
}

/// Runs one command with a hard deadline and bounded stdout/stderr capture,
/// never through a shell. Each stream goes to a temporary file so a child
/// holding a pipe open cannot outlive cancellation. This is the single
/// subprocess runner behind both the legacy `Option` API and the
/// Go-parity secret-reference path.
fn run_command_capture(
    command: &str,
    args: &[&str],
    timeout: Duration,
    cap: u64,
) -> Result<Vec<u8>, CommandFailure> {
    let file =
        tempfile::NamedTempFile::new().map_err(|error| CommandFailure::Other(error.to_string()))?;
    let stderr_file =
        tempfile::NamedTempFile::new().map_err(|error| CommandFailure::Other(error.to_string()))?;
    let handle = file
        .as_file()
        .try_clone()
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    let stderr_handle = stderr_file
        .as_file()
        .try_clone()
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    let mut child_command = Command::new(command);
    child_command
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::from(stderr_handle))
        .stdout(Stdio::from(handle));
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut child_command, 0);
    let mut child = match child_command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(CommandFailure::NotFound);
        }
        Err(error) => return Err(CommandFailure::Other(error.to_string())),
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    let stderr = read_capture(stderr_file.as_file(), cap);
                    return Err(CommandFailure::ExitFailed {
                        code: status.code(),
                        stderr,
                    });
                }
                break;
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Ok(None) => {
                terminate_child(&mut child);
                return Err(CommandFailure::TimedOut);
            }
            Err(error) => {
                terminate_child(&mut child);
                return Err(CommandFailure::Other(error.to_string()));
            }
        }
    }
    terminate_child(&mut child);
    let mut file = file
        .as_file()
        .try_clone()
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    let mut output = Vec::new();
    file.take(cap + 1)
        .read_to_end(&mut output)
        .map_err(|error| CommandFailure::Other(error.to_string()))?;
    if output.len() as u64 > cap {
        return Err(CommandFailure::Other(
            "command output exceeds the bounded read limit".into(),
        ));
    }
    Ok(output)
}

fn read_capture(file: &std::fs::File, cap: u64) -> Vec<u8> {
    let Ok(mut file) = file.try_clone() else {
        return Vec::new();
    };
    if file.seek(SeekFrom::Start(0)).is_err() {
        return Vec::new();
    }
    let mut buffer = Vec::new();
    if file.take(cap + 1).read_to_end(&mut buffer).is_err() {
        return Vec::new();
    }
    if buffer.len() as u64 > cap {
        buffer.truncate(usize::try_from(cap).unwrap_or(usize::MAX));
    }
    buffer
}

/// Legacy surface: probes and the usage credential path keep their
/// `Option<Vec<u8>>` contract over the shared runner.
#[cfg(target_os = "macos")]
fn bounded_command_stdout(
    command: &str,
    args: &[&str],
    timeout: Duration,
    cap: u64,
) -> Option<Vec<u8>> {
    run_command_capture(command, args, timeout, cap).ok()
}

fn terminate_child(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = rustix::process::kill_process_group(
            rustix::process::Pid::from_child(child),
            rustix::process::Signal::KILL,
        );
    }
    let _ = child.kill();
    let _ = child.wait();
}
