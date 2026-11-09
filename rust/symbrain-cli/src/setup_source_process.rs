//! Owned argv-based source tool processes with cancellation and file capture.
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Child;
use std::process::{Command, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use symbrain_managed::{GoText, format_io_error, format_process_exit_status};

pub(super) struct Output {
    pub bytes: Vec<u8>,
    pub error: Option<String>,
}

pub(super) fn lookup(tool: &str) -> Result<PathBuf, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let names = vec![OsString::from(tool)];
    let allow_relative = std::env::var("GODEBUG")
        .unwrap_or_default()
        .split(',')
        .filter_map(|item| item.strip_prefix("execerrdot="))
        .next_back()
        == Some("0");
    let dot_error =
        || format!("exec: {tool:?}: cannot run executable found relative to current directory");
    // Go filepath.SplitList("") has no entries, while Rust yields one empty
    // entry. Nonempty Unix lists still permit explicit empty CWD entries.
    for directory in std::env::split_paths(&path).filter(|_| !path.is_empty()) {
        for name in &names {
            let candidate = directory.join(name);
            if !candidate.is_file() {
                continue;
            }
            {
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(&candidate).is_ok_and(|meta| meta.permissions().mode() & 0o111 == 0)
                {
                    continue;
                }
            }
            if !candidate.is_absolute() && !allow_relative {
                return Err(dot_error());
            }
            return Ok(candidate);
        }
    }
    let path_name = "$PATH";
    Err(format!(
        "exec: {tool:?}: executable file not found in {path_name}"
    ))
}

#[allow(clippy::too_many_arguments)] // Independent argv, cwd, environment, capture, budget and cancellation contracts.
pub(super) fn run(
    executable: &Path,
    args: &[&OsStr],
    cwd: Option<&Path>,
    environment: &[(OsString, OsString)],
    combined: bool,
    budget: Duration,
    context: &Context,
) -> Result<Output, GoText> {
    if context.cancelled.load(Ordering::Relaxed) {
        return Err("source build cancelled".into());
    }
    if let Some(cwd) = cwd {
        fs::metadata(cwd).map_err(|error| {
            GoText::path("chdir ", cwd, &format!(": {}", format_io_error(&error)))
        })?;
    }
    let capture = super::temp::capture()?;
    let writer = capture
        .as_file()
        .try_clone()
        .map_err(|error| format!("open build capture: {error}"))?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .envs(environment.iter().cloned())
        .stdin(Stdio::null())
        .stdout(writer);
    if combined {
        // Clones share the file offset, preserving the tool's combined write order.
        command.stderr(
            capture
                .as_file()
                .try_clone()
                .map_err(|error| error.to_string())?,
        );
    } else {
        command.stderr(Stdio::null());
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let child = command.spawn();
    let child = child.map_err(|error| {
        GoText::path(
            "fork/exec ",
            executable,
            &format!(": {}", format_io_error(&error)),
        )
    })?;
    let mut owned = Owned {
        pid: child.id(),
        child,
    };
    let started = Instant::now();
    let error = loop {
        let status = owned.child.try_wait();
        match status {
            Ok(Some(status)) => {
                break (!status.success()).then(|| format_process_exit_status(status));
            }
            Ok(None) if context.cancelled.load(Ordering::Relaxed) => {
                break Some("source build cancelled".into());
            }
            Ok(None) if started.elapsed() >= budget => break Some("source build timed out".into()),
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => break Some(format_io_error(&error)),
        }
    };
    drop(owned);
    let bytes = fs::read(capture.path()).map_err(|error| error.to_string())?;
    Ok(Output { bytes, error })
}

struct Owned {
    pid: u32,
    child: Child,
}
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", self.pid)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

pub(super) struct Context {
    cancelled: Arc<AtomicBool>,
    ids: Vec<signal_hook::SigId>,
}
impl Context {
    // One fallible interface: Unix signal registration can fail; Windows keeps
    // the same caller contract even though it currently has no registration.
    pub(super) fn new() -> Result<Self, String> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut guard = Self {
            cancelled: Arc::clone(&cancelled),
            ids: Vec::new(),
        };
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            guard.ids.push(
                signal_hook::flag::register(signal, Arc::clone(&cancelled))
                    .map_err(|error| error.to_string())?,
            );
        }
        Ok(guard)
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_capture_preserves_sequential_stdout_and_stderr_writes() {
        let context = Context::new().unwrap();
        let out = run(
            Path::new("/bin/sh"),
            &[
                OsStr::new("-c"),
                OsStr::new("printf first; printf second >&2"),
            ],
            None,
            &[],
            true,
            Duration::from_secs(3),
            &context,
        )
        .unwrap();
        assert!(out.error.is_none());
        assert_eq!(out.bytes, b"firstsecond");
    }

    #[test]
    fn deadline_kills_and_reaps_the_owned_tool_process() {
        let context = Context::new().unwrap();
        let out = run(
            Path::new("/bin/sh"),
            &[OsStr::new("-c"), OsStr::new("echo $$; exec /bin/sleep 60")],
            None,
            &[],
            false,
            Duration::from_millis(250),
            &context,
        )
        .unwrap();
        assert_eq!(out.error.as_deref(), Some("source build timed out"));
        let pid = std::str::from_utf8(&out.bytes).unwrap().trim();
        assert!(pid.parse::<u32>().is_ok());
        assert!(
            !Command::new("/bin/kill")
                .args(["-0", pid])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
            "owned child survived or was not reaped"
        );
    }
}
