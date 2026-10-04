//! Owned argv-based source tool processes with cancellation and file capture.
#[cfg(windows)]
use process_wrap::std::{ChildWrapper, CommandWrap, JobObject};
#[cfg(windows)]
#[path = "setup_source_windows_job.rs"]
mod windows_job;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
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
    #[cfg(windows)]
    let mut names: Vec<OsString> = std::env::var("PATHEXT")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".into())
        .to_lowercase()
        .split(';')
        .filter(|ext| !ext.is_empty())
        .map(|ext| {
            format!(
                "{tool}{}{}",
                if ext.starts_with('.') { "" } else { "." },
                ext
            )
            .into()
        })
        .collect();
    #[cfg(windows)]
    if names.is_empty() {
        names.push(OsString::from(tool));
    }
    #[cfg(not(windows))]
    let names = vec![OsString::from(tool)];
    let allow_relative = std::env::var("GODEBUG")
        .unwrap_or_default()
        .split(',')
        .filter_map(|item| item.strip_prefix("execerrdot="))
        .next_back()
        == Some("0");
    let dot_error =
        || format!("exec: {tool:?}: cannot run executable found relative to current directory");
    #[cfg(windows)]
    let mut implicit = if std::env::var_os("NoDefaultCurrentDirectoryInExePath").is_none() {
        names
            .iter()
            .map(PathBuf::from)
            .find(|candidate| candidate.is_file())
    } else {
        None
    };
    #[cfg(windows)]
    if allow_relative && let Some(implicit) = &implicit {
        return Ok(implicit.clone());
    }
    // Go filepath.SplitList("") has no entries, while Rust yields one empty
    // entry. Nonempty Unix lists still permit explicit empty CWD entries.
    for directory in std::env::split_paths(&path).filter(|_| !path.is_empty()) {
        #[cfg(windows)]
        if directory.as_os_str().is_empty() {
            continue;
        }
        for name in &names {
            let candidate = directory.join(name);
            if !candidate.is_file() {
                continue;
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(&candidate).is_ok_and(|meta| meta.permissions().mode() & 0o111 == 0)
                {
                    continue;
                }
            }
            #[cfg(windows)]
            if let Some(implicit) = &implicit
                && !same_lookup_file(implicit, &candidate)
            {
                return Err(dot_error());
            }
            if !candidate.is_absolute() && !allow_relative {
                #[cfg(windows)]
                {
                    // Go remembers the first relative candidate and continues:
                    // only a later absolute name for that same file may win.
                    implicit.get_or_insert(candidate);
                    continue;
                }
                #[cfg(not(windows))]
                return Err(dot_error());
            }
            return Ok(candidate);
        }
    }
    #[cfg(windows)]
    if implicit.is_some() {
        return Err(dot_error());
    }
    #[cfg(windows)]
    let path_name = "%PATH%";
    #[cfg(not(windows))]
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
    #[cfg(windows)]
    let resolved = if executable.is_absolute() {
        None
    } else {
        cwd.map(|directory| directory.join(executable))
    };
    #[cfg(windows)]
    if let Some(resolved) = &resolved {
        // Go resolves relative executable names against Cmd.Dir on Windows;
        // CreateProcess alone would instead search the parent's directory.
        if !resolved.is_file() {
            return Err(format!(
                "exec: {}: executable file not found in %PATH%",
                symbrain_core::config::format_go_quoted(executable.as_os_str())
            )
            .into());
        }
    }
    #[cfg(windows)]
    let executable = resolved.as_deref().unwrap_or(executable);
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
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    let child = CommandWrap::from(command).wrap(JobObject).spawn();
    #[cfg(not(windows))]
    let child = command.spawn();
    let child = child.map_err(|error| {
        GoText::path(
            "fork/exec ",
            executable,
            &format!(": {}", format_io_error(&error)),
        )
    })?;
    let mut owned = Owned {
        #[cfg(unix)]
        pid: child.id(),
        child,
    };
    let started = Instant::now();
    let error = loop {
        #[cfg(windows)]
        let status = windows_job::poll_parent(owned.child.as_mut());
        #[cfg(not(windows))]
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
    #[cfg(unix)]
    pid: u32,
    #[cfg(not(windows))]
    child: Child,
    #[cfg(windows)]
    child: Box<dyn ChildWrapper>,
}
impl Drop for Owned {
    fn drop(&mut self) {
        #[cfg(unix)]
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", self.pid)])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        #[cfg(windows)]
        let _ = windows_job::terminate_and_reap(self.child.as_mut());
        #[cfg(not(windows))]
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
    #[cfg_attr(not(unix), allow(clippy::unnecessary_wraps))]
    pub(super) fn new() -> Result<Self, String> {
        let cancelled = Arc::new(AtomicBool::new(false));
        #[cfg(unix)]
        let mut guard = Self {
            cancelled: Arc::clone(&cancelled),
            ids: Vec::new(),
        };
        #[cfg(not(unix))]
        let guard = Self {
            cancelled: Arc::clone(&cancelled),
            ids: Vec::new(),
        };
        #[cfg(unix)]
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            guard.ids.push(
                signal_hook::flag::register(signal, Arc::clone(&cancelled))
                    .map_err(|error| error.to_string())?,
            );
        }
        #[cfg(not(unix))]
        let _ = cancelled;
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

#[cfg(all(test, unix))]
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

#[cfg(windows)]
fn same_lookup_file(a: &Path, b: &Path) -> bool {
    use std::os::windows::fs::OpenOptionsExt;
    fn identity(path: &Path) -> std::io::Result<same_file::Handle> {
        // Match Go's Lstat identity, including an explicit symlink/reparse file.
        let file = fs::OpenOptions::new()
            .access_mode(0x0080)
            .custom_flags(0x0020_0000 | 0x0200_0000)
            .open(path)?;
        same_file::Handle::from_file(file)
    }
    identity(a)
        .ok()
        .zip(identity(b).ok())
        .is_some_and(|(a, b)| a == b)
}
