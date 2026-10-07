//! Unchanged standalone vault/provisioning subprocess lifecycle.
use super::*;

pub(in crate::key_sources) fn run_command(
    program: &Path,
    args: &[&str],
    input: Option<&[u8]>,
    timeout: Duration,
) -> Result<CommandOutput, ProbeError> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    configure_process_tree(&mut command);
    let deadline = std::time::Instant::now() + timeout;
    let mut child = {
        let mut last_error = None;
        let mut spawned = None;
        for _ in 0..3 {
            if deadline <= std::time::Instant::now() {
                return Err(ProbeError::Failed(
                    "command timed out before spawn".to_owned(),
                ));
            }
            match spawn_command(&mut command) {
                Ok(child) => {
                    spawned = Some(child);
                    break;
                }
                Err(error) if error.raw_os_error() == Some(26) => {
                    // ETXTBSY is a transient Unix race when a freshly-created
                    // fixture executable is still being released by the filesystem.
                    last_error = Some(error);
                    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                    if remaining.is_zero() {
                        return Err(ProbeError::Failed(
                            "command timed out before spawn retry".to_owned(),
                        ));
                    }
                    thread::sleep(Duration::from_millis(2).min(remaining));
                }
                Err(error) => {
                    last_error = Some(error);
                    break;
                }
            }
        }
        match spawned {
            Some(child) => child,
            None => {
                let error = last_error.expect("spawn error");
                return Err(if error.kind() == std::io::ErrorKind::NotFound {
                    ProbeError::Missing(MissingReason::Unavailable)
                } else {
                    ProbeError::Failed(error.to_string())
                });
            }
        }
    };

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProbeError::Failed("capture child stdout".to_owned()))?;
    let (output_sender, output_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = output_sender.send(read_bounded(stdout));
    });

    let deadline = std::time::Instant::now() + timeout;
    if let Some(input) = input
        && let Some(mut stdin) = child.stdin.take()
        && let Err(error) = stdin.write_all(input)
    {
        terminate_process_tree(&mut child);
        return Err(ProbeError::Failed(error.to_string()));
    }

    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    let status = match child.wait_timeout(remaining) {
        Ok(Some(status)) => status,
        Ok(None) => {
            terminate_process_tree(&mut child);
            return Err(ProbeError::Failed(format!(
                "command timed out after {}ms",
                timeout.as_millis()
            )));
        }
        Err(error) => {
            terminate_process_tree(&mut child);
            return Err(ProbeError::Failed(error.to_string()));
        }
    };

    let remaining = deadline.saturating_duration_since(std::time::Instant::now());
    let stdout = if remaining.is_zero() {
        terminate_process_tree(&mut child);
        return Err(ProbeError::Failed("command timed out".to_owned()));
    } else {
        match output_receiver.recv_timeout(remaining) {
            Ok(Ok(output)) => output,
            Ok(Err(error)) => {
                terminate_process_tree(&mut child);
                return Err(error);
            }
            Err(_) => {
                terminate_process_tree(&mut child);
                return Err(ProbeError::Failed(
                    "stdout pipe remained open after command exit".to_owned(),
                ));
            }
        }
    };
    Ok(CommandOutput { status, stdout })
}
