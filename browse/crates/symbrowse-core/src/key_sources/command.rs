//! Existing bounded subprocess execution; cancellation is opt-in for startup owners.
use crate::key_resolver::{MissingReason, ProbeError};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
use wait_timeout::ChildExt;
const MAX_OUTPUT_BYTES: usize = 1 << 20;
pub(super) enum Ownership {
    Supervisor,
    Provider(Arc<AtomicBool>),
}
impl Ownership {
    fn cancelled(&self) -> bool {
        matches!(self, Self::Provider(cancelled) if cancelled.load(Ordering::Acquire))
    }
}
#[derive(Debug)]
pub(super) struct CommandOutput {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
}
#[derive(Debug)]
pub(super) struct OwnedOutput {
    pub(super) status: ExitStatus,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}

mod owned_child;
mod standalone;
use owned_child::{OwnedChild, OwnedCommand};
pub(super) use standalone::run_command;

pub(super) fn run_owned_command(
    program: &Path,
    args: &[OsString],
    input: Option<&[u8]>,
    timeout: Duration,
    ownership: Ownership,
) -> Result<OwnedOutput, ProbeError> {
    // Discovery belongs only to the explicit startup provider owner. The
    // supervisor executable and standalone/public runner keep their contract.
    let resolved = if matches!(ownership, Ownership::Provider(_)) {
        Some(
            super::startup_discovery::executable(program)?
                .ok_or(ProbeError::Missing(MissingReason::Unavailable))?,
        )
    } else {
        None
    };
    #[cfg(windows)]
    if let Some(path) = &resolved
        && let Some(error) = super::startup_discovery::batch_error(program, &path.spelling)
    {
        return Err(ProbeError::Failed(error));
    }
    #[cfg(windows)]
    let launch = resolved
        .as_ref()
        .map(|path| super::startup_discovery::launch_path(program, path));
    #[cfg(not(windows))]
    let launch = resolved.as_ref().map(|path| path.owner.clone());
    let mut command = Command::new(launch.as_deref().unwrap_or(program));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.arg0(program);
    }
    command
        .args(args)
        .stdin(
            if input.is_some() || matches!(ownership, Ownership::Supervisor) {
                Stdio::piped()
            } else {
                Stdio::null()
            },
        )
        .stdout(Stdio::piped())
        .stderr(if matches!(ownership, Ownership::Supervisor) {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    configure_process_tree(&mut command);
    let mut command = OwnedCommand::new(command);
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
            match command.spawn() {
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
        .stdout()
        .take()
        .ok_or_else(|| ProbeError::Failed("capture child stdout".to_owned()))?;
    let (output_sender, output_receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = output_sender.send(read_bounded(stdout));
    });

    let stderr_receiver = child.stderr().take().map(|stderr| {
        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let _ = sender.send(read_bounded(stderr));
        });
        receiver
    });
    // This writer remains owned by this stack frame. Rust's process API does
    // not inherit it into the supervisor or its provider children.
    let mut lifetime_writer = if matches!(ownership, Ownership::Supervisor) {
        child.stdin().take()
    } else {
        None
    };
    let deadline = Instant::now() + timeout;
    if let Some(input) = input
        && let Some(mut stdin) = child.stdin().take()
        && let Err(error) = stdin.write_all(input)
    {
        child.terminate();
        return Err(ProbeError::Failed(error.to_string()));
    }

    let status = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if ownership.cancelled() || remaining.is_zero() {
            stop_owned(&mut child, &ownership, &mut lifetime_writer);
            return Err(ProbeError::Failed(format!(
                "command timed out after {}ms",
                timeout.as_millis()
            )));
        }
        let wait = if matches!(ownership, Ownership::Provider(_)) {
            remaining.min(Duration::from_millis(20))
        } else {
            remaining
        };
        match child.wait_timeout(wait) {
            Ok(Some(status)) => break status,
            Ok(None) => continue,
            Err(error) => {
                stop_owned(&mut child, &ownership, &mut lifetime_writer);
                return Err(ProbeError::Failed(error.to_string()));
            }
        }
    };

    let stdout = receive_output(&output_receiver, deadline, &ownership).inspect_err(|_| {
        stop_owned(&mut child, &ownership, &mut lifetime_writer);
    })?;
    let stderr = match stderr_receiver {
        Some(receiver) => receive_output(&receiver, deadline, &ownership).inspect_err(|_| {
            stop_owned(&mut child, &ownership, &mut lifetime_writer);
        })?,
        None => Vec::new(),
    };
    Ok(OwnedOutput {
        status,
        stdout,
        stderr,
    })
}

#[cfg(test)]
thread_local! {
    pub(super) static INJECT_ETXTBSY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn spawn_command(command: &mut Command) -> std::io::Result<Child> {
    #[cfg(test)]
    if INJECT_ETXTBSY.with(|remaining| {
        let count = remaining.get();
        remaining.set(count.saturating_sub(1));
        count != 0
    }) {
        return Err(std::io::Error::from_raw_os_error(26));
    }
    command.spawn()
}

#[cfg(unix)]
fn configure_process_tree(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_process_tree(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_process_tree(_command: &mut Command) {}

#[cfg(unix)]
fn terminate_process_tree(child: &mut std::process::Child) {
    use rustix::process::{Pid, Signal, kill_process_group};

    if let Some(pid) = Pid::from_raw(child.id() as i32) {
        let _ = kill_process_group(pid, Signal::KILL);
    }
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(windows)]
fn terminate_process_tree(child: &mut std::process::Child) {
    let pid = child.id().to_string();
    let _ = Command::new("taskkill.exe")
        .args(["/PID", &pid, "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(not(any(unix, windows)))]
fn terminate_process_tree(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn read_bounded(mut reader: impl Read) -> Result<Vec<u8>, ProbeError> {
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut oversized = false;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| ProbeError::Failed(error.to_string()))?;
        if read == 0 {
            break;
        }
        if output.len() < MAX_OUTPUT_BYTES {
            let keep = read.min(MAX_OUTPUT_BYTES - output.len());
            output.extend_from_slice(&buffer[..keep]);
            oversized |= keep != read;
        } else {
            oversized = true;
        }
    }
    if oversized {
        return Err(ProbeError::Failed(format!(
            "command output exceeds {MAX_OUTPUT_BYTES} bytes"
        )));
    }
    Ok(output)
}

fn receive_output(
    receiver: &mpsc::Receiver<Result<Vec<u8>, ProbeError>>,
    deadline: Instant,
    ownership: &Ownership,
) -> Result<Vec<u8>, ProbeError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if ownership.cancelled() || remaining.is_zero() {
            return Err(ProbeError::Failed(
                "stdout pipe remained open after command exit".into(),
            ));
        }
        let wait = if matches!(ownership, Ownership::Provider(_)) {
            remaining.min(Duration::from_millis(20))
        } else {
            remaining
        };
        match receiver.recv_timeout(wait) {
            Ok(output) => return output,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ProbeError::Failed("capture child output".into()));
            }
        }
    }
}

fn stop_owned(
    child: &mut OwnedChild,
    ownership: &Ownership,
    writer: &mut Option<std::process::ChildStdin>,
) {
    if matches!(ownership, Ownership::Supervisor) {
        // EOF first lets the still-live supervisor reap its separate provider
        // group even when the daemon's own process group has been killed.
        drop(writer.take());
        if matches!(child.wait_timeout(Duration::from_secs(2)), Ok(Some(_))) {
            return;
        }
    }
    child.terminate();
}
