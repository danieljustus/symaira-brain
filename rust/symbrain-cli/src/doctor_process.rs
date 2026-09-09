use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

pub(super) fn which(binary: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    env::split_paths(&path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file() && is_executable(candidate))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}
#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

pub(super) fn run_process(
    path: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>), String> {
    let mut command = Command::new(path);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "capture child stdout".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "capture child stderr".to_string())?;
    let stdout_reader = thread::spawn(move || read_all(stdout));
    let stderr_reader = thread::spawn(move || read_all(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                terminate(&mut child);
                return Err("probe timed out".to_string());
            }
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                terminate(&mut child);
                return Err(error.to_string());
            }
        }
    };
    while !stdout_reader.is_finished() || !stderr_reader.is_finished() {
        if started.elapsed() >= timeout {
            terminate(&mut child);
            return Err("probe timed out".to_string());
        }
        thread::sleep(Duration::from_millis(10));
    }
    let out = join_reader(stdout_reader, "stdout")?;
    let err = join_reader(stderr_reader, "stderr")?;
    Ok((status, out, err))
}

fn terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn read_all(mut reader: impl Read) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn join_reader(
    handle: thread::JoinHandle<Result<Vec<u8>, String>>,
    stream: &str,
) -> Result<Vec<u8>, String> {
    handle
        .join()
        .map_err(|_| format!("capture child {stream}: reader panicked"))?
        .map_err(|error| format!("capture child {stream}: {error}"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::run_process;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::{Duration, Instant};

    fn script(contents: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        // Publish the executable with an atomic rename. Linux rejects execve
        // with ETXTBSY while the target is still open for writing; writing the
        // final pathname directly made this fixture race cargo's parallel
        // test execution on the hosted runner.
        let path = dir.path().join("probe");
        let staging = dir.path().join("probe.staging");
        fs::write(&staging, contents).expect("write probe");
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o700)).expect("chmod probe");
        fs::rename(&staging, &path).expect("publish probe");
        dir
    }

    #[test]
    fn drains_large_stdout_and_stderr_without_deadlock() {
        let dir = script(
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do\n  printf '0123456789abcdef0123456789abcdef\\n'\n  printf 'fedcba9876543210fedcba9876543210\\n' >&2\n  i=$((i + 1))\ndone\n",
        );
        let (status, stdout, stderr) =
            run_process(&dir.path().join("probe"), &[], Duration::from_secs(5)).expect("probe");
        assert!(status.success());
        assert!(stdout.len() > 64 * 1024);
        assert!(stderr.len() > 64 * 1024);
    }

    #[test]
    fn terminates_timed_out_probe() {
        let dir = script("#!/bin/sh\nsleep 30\n");
        let started = Instant::now();
        let error = run_process(&dir.path().join("probe"), &[], Duration::from_millis(25))
            .expect_err("timeout");
        assert_eq!(error, "probe timed out");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn retained_descendant_pipes_cannot_extend_timeout() {
        let dir = script("#!/bin/sh\nsleep 30 &\nexit 0\n");
        let started = Instant::now();
        let error = run_process(&dir.path().join("probe"), &[], Duration::from_millis(25))
            .expect_err("retained pipe timeout");
        assert_eq!(error, "probe timed out");
        assert!(started.elapsed() < Duration::from_secs(2));
    }
}
