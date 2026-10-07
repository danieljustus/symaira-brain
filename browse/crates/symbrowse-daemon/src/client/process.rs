use super::*;
use std::fs::{self, OpenOptions};

impl Client {
    pub(super) fn start_daemon(&self) -> Result<Child, ClientError> {
        let start = self.options.start.clone().unwrap_or_else(|| StartOptions {
            executable: current_executable(),
            log_path: default_log_path(),
            args: vec![
                "daemon".into(),
                "--session".into(),
                self.options.session.clone(),
            ],
        });
        let launch = || -> io::Result<Child> {
            if start.executable.as_os_str().is_empty() {
                return Err(io::Error::other("daemon executable path is required"));
            }
            if start.args.first().map(String::as_str) != Some("daemon") {
                return Err(io::Error::other(
                    "daemon start arguments must begin with 'daemon'",
                ));
            }
            if let Some(parent) = start
                .log_path
                .parent()
                .filter(|path| !path.as_os_str().is_empty())
            {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    let mut builder = fs::DirBuilder::new();
                    builder.recursive(true).mode(0o700).create(parent)?;
                }
                #[cfg(not(unix))]
                fs::create_dir_all(parent)?;
            }
            let mut log_options = OpenOptions::new();
            log_options.create(true).append(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                log_options.mode(0o600);
            }
            let log = log_options.open(&start.log_path)?;
            let stderr = log.try_clone()?;
            let mut command = Command::new(&start.executable);
            command
                .args(&start.args)
                .stdin(Stdio::null())
                .stdout(Stdio::from(log))
                .stderr(Stdio::from(stderr));
            detach_command(&mut command);
            command.spawn()
        };
        launch().map_err(|error| {
            lifecycle_error(
                &self.options,
                format!(
                    "failed to start daemon for session {:?}: {}",
                    self.options.session,
                    redact_str(&error.to_string())
                ),
            )
        })
    }
}

const CHILD_REAP_TIMEOUT: Duration = Duration::from_secs(1);

pub(super) fn terminate_child(child: &mut Child) {
    #[cfg(unix)]
    {
        if let Ok(pid) = i32::try_from(child.id()) {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pid),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        let _ = child.kill();
    }
    #[cfg(windows)]
    {
        let _ = child.kill();
    }
    let deadline = Instant::now() + CHILD_REAP_TIMEOUT;
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
        }
    }
    // Do not fall back to Child::wait: a platform process can remain
    // unkillable, and cleanup must never turn a bounded request into an
    // unbounded join. The final try_wait preserves best-effort reaping.
    let _ = child.try_wait();
}

fn current_executable() -> PathBuf {
    std::env::current_exe().unwrap_or_else(|_| PathBuf::from("symbrowse"))
}

/// Detach a daemon launch from the caller's process group and stdio.
pub fn detach_command(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
        disinherit_caller_stdio();
        for (key, value) in std::env::vars_os() {
            let (go_key, go_value) = (go_env_block_text(&key), go_env_block_text(&value));
            if go_key != key || go_value != value {
                command.env_remove(&key).env(go_key, go_value);
            }
        }
    }
}

/// Go autostarts its daemon through os/exec, whose syscall.createEnvBlock
/// ranges over each WTF-8 environment string as UTF-8: the three bytes of an
/// unpaired UTF-16 surrogate become three U+FFFD units in the daemon's block
/// (a PATH entry `raw-<D800>` arrives as `raw-<FFFD><FFFD><FFFD>`). Only the
/// launch hand-off is lossy in Go; a daemon started directly keeps raw units.
#[cfg(windows)]
pub(crate) fn go_env_block_text(text: &std::ffi::OsStr) -> std::ffi::OsString {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let mut units = Vec::new();
    for decoded in char::decode_utf16(text.encode_wide()) {
        match decoded {
            Ok(character) => units.extend(character.encode_utf16(&mut [0; 2]).iter()),
            Err(_) => units.extend([0xfffd; 3]),
        }
    }
    std::ffi::OsString::from_wide(&units)
}

/// Rust std spawns with `bInheritHandles = TRUE` and no handle list, unlike
/// Go's `PROC_THREAD_ATTRIBUTE_HANDLE_LIST`. Without this, a detached daemon
/// keeps the caller's stdio pipes open, so whoever reads this CLI's output
/// (a shell pipeline, an MCP client, a test harness) waits for daemon exit.
/// `Stdio::inherit` stays intact: std duplicates an inheritable copy per spawn.
#[cfg(windows)]
#[allow(unsafe_code)]
fn disinherit_caller_stdio() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    for handle in [
        io::stdin().as_raw_handle(),
        io::stdout().as_raw_handle(),
        io::stderr().as_raw_handle(),
    ] {
        if !handle.is_null() {
            // SAFETY: a live process std handle; this clears only its inherit
            // flag and neither closes nor takes ownership of it. Failure (for
            // example a console pseudo-handle) leaves nothing to leak.
            unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) };
        }
    }
}

#[cfg(all(test, windows))]
mod go_env_block_tests {
    use super::go_env_block_text;
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    fn wide(text: &std::ffi::OsStr) -> Vec<u16> {
        text.encode_wide().collect()
    }

    #[test]
    fn unpaired_surrogates_become_three_replacements_like_go_create_env_block() {
        // provider-discovery raw-wide-path: PATH=<root>\raw-<D800>.
        let raw = OsString::from_wide(&[0x72, 0x61, 0x77, 0x2d, 0xd800]);
        assert_eq!(
            wide(&go_env_block_text(&raw)),
            [0x72, 0x61, 0x77, 0x2d, 0xfffd, 0xfffd, 0xfffd]
        );
        // raw-wide-pathext: PATHEXT=.E<D800>, and a trailing low surrogate.
        let ext = OsString::from_wide(&[0x2e, 0x45, 0xd800, 0x3b, 0xdc00]);
        assert_eq!(
            wide(&go_env_block_text(&ext)),
            [
                0x2e, 0x45, 0xfffd, 0xfffd, 0xfffd, 0x3b, 0xfffd, 0xfffd, 0xfffd
            ]
        );
        // Well-formed text, including a surrogate pair, is passed unchanged.
        let valid = OsString::from_wide(&[0x43, 0x3a, 0xd83d, 0xde00, 0xfffd]);
        assert_eq!(go_env_block_text(&valid), valid);
    }
}
