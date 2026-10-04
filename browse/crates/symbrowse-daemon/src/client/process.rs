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

fn detach_command(command: &mut Command) {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP | DETACHED_PROCESS);
    }
}
