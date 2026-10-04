//! Private lifetime-pipe supervisor, used only by explicit CLI daemon startup.
use super::command::{CommandOutput, Ownership, run_owned_command};
use crate::key_resolver::{MissingReason, ProbeError};
use std::{
    ffi::OsString,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::ExitStatus,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub const STARTUP_PROVIDER_ARGUMENT: &str = "--internal-startup-key-provider";

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "result", deny_unknown_fields)]
enum Outcome {
    Exited { status: u32 },
    Unavailable,
    Failed { message: String },
}

pub(super) fn lookup(
    owner: &Path,
    program: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<CommandOutput, ProbeError> {
    let mut arguments = vec![
        OsString::from(STARTUP_PROVIDER_ARGUMENT),
        program.as_os_str().to_owned(),
        OsString::from(timeout.as_millis().to_string()),
    ];
    arguments.extend(args.iter().map(OsString::from));
    // Provider names and references are public and bounded; key bytes travel
    // only through the captured stdout pipe, never through argv or diagnostics.
    let output = run_owned_command(owner, &arguments, None, timeout, Ownership::Supervisor)
        .map_err(|error| match error {
            // Only the explicitly running supervisor may report a missing
            // provider. A missing owner is a failed ownership boundary.
            ProbeError::Missing(_) => {
                ProbeError::Failed("startup provider supervisor unavailable".into())
            }
            error => error,
        })?;
    if !output.status.success() {
        return Err(ProbeError::Failed(
            "startup provider supervisor failed".into(),
        ));
    }
    match serde_json::from_slice::<Outcome>(&output.stderr)
        .map_err(|_| ProbeError::Failed("invalid startup provider result".into()))?
    {
        Outcome::Exited { status } => Ok(CommandOutput {
            status: from_raw_status(status),
            stdout: output.stdout,
        }),
        Outcome::Unavailable => Err(ProbeError::Missing(MissingReason::Unavailable)),
        Outcome::Failed { message } => Err(ProbeError::Failed(message)),
    }
}

/// Internal executable route. A caller must launch this process in a private
/// group with a piped stdin. EOF means its explicit startup owner disappeared.
/// It exposes no IPC endpoint and cannot resolve or provision arbitrary keys.
pub fn run_startup_provider(args: &[OsString]) -> u8 {
    let Some((program, timeout, provider_args)) = validate(args) else {
        return 1;
    };
    #[cfg(unix)]
    if rustix::process::getpgrp() != rustix::process::getpid() {
        return 1;
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let watch = Arc::clone(&cancelled);
    std::thread::spawn(move || {
        // No command is sent over this channel. Any byte, EOF or read failure
        // closes the lease; inherited handles cannot prolong daemon ownership.
        let mut byte = [0_u8];
        let _ = std::io::stdin().read(&mut byte);
        watch.store(true, Ordering::Release);
    });
    let arguments: Vec<_> = provider_args.iter().map(OsString::from).collect();
    // The owner's deadline is authoritative. This fallback is later so its
    // normal deadline diagnostic cannot race the parent's timeout adaptation.
    let outcome = match run_owned_command(
        &program,
        &arguments,
        None,
        timeout + Duration::from_secs(1),
        Ownership::Provider(cancelled),
    ) {
        Ok(output) => {
            if std::io::stdout().write_all(&output.stdout).is_err() {
                return 1;
            }
            Outcome::Exited {
                status: raw_status(output.status),
            }
        }
        Err(ProbeError::Missing(MissingReason::Unavailable)) => Outcome::Unavailable,
        Err(error) => Outcome::Failed {
            message: match error {
                ProbeError::Failed(message) => message,
                ProbeError::Missing(_) => "startup provider unavailable".into(),
            },
        },
    };
    if serde_json::to_writer(std::io::stderr().lock(), &outcome).is_err() {
        return 1;
    }
    0
}

fn validate(args: &[OsString]) -> Option<(PathBuf, Duration, Vec<String>)> {
    let [program, timeout, remaining @ ..] = args else {
        return None;
    };
    let timeout = timeout.to_str()?.parse::<u64>().ok()?;
    if timeout == 0 || timeout > 60_000 {
        return None;
    }
    let program = PathBuf::from(program);
    let name = program.file_stem()?.to_str()?;
    let values = remaining
        .iter()
        .map(|arg| arg.to_str().map(str::to_owned))
        .collect::<Option<Vec<_>>>()?;
    let refs: Vec<_> = values.iter().map(String::as_str).collect();
    let valid = match name {
        "symvault" => refs == ["get", crate::key_resolver::VAULT_ENTRY_NAME],
        "security" => {
            refs == [
                "find-generic-password",
                "-s",
                crate::key_resolver::KEYCHAIN_SERVICE,
                "-a",
                crate::key_resolver::KEYCHAIN_ACCOUNT,
                "-w",
            ]
        }
        _ => false,
    };
    valid.then_some((program, Duration::from_millis(timeout), values))
}

#[cfg(unix)]
fn raw_status(status: ExitStatus) -> u32 {
    use std::os::unix::process::ExitStatusExt;
    status.into_raw() as u32
}
#[cfg(unix)]
fn from_raw_status(status: u32) -> ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    ExitStatus::from_raw(status as i32)
}
#[cfg(windows)]
fn raw_status(status: ExitStatus) -> u32 {
    // Windows ExitStatus always carries the process DWORD, represented as i32.
    status.code().unwrap_or(1) as u32
}
#[cfg(windows)]
fn from_raw_status(status: u32) -> ExitStatus {
    use std::os::windows::process::ExitStatusExt;
    ExitStatus::from_raw(status)
}
