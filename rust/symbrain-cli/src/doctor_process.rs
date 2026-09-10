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
    use std::path::Path;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    fn shell_fixture(contents: &str) -> (&'static Path, tempfile::TempDir, String) {
        // Execute fixture text through the stable system shell. The fixture
        // pathname is never execve'd, so Linux cannot race a writer against
        // execve with ETXTBSY while tests run in parallel.
        let dir = tempfile::tempdir().expect("tempdir");
        (Path::new("/bin/sh"), dir, contents.to_string())
    }

    #[test]
    fn drains_large_stdout_and_stderr_without_deadlock() {
        let (shell, _dir, script) = shell_fixture(
            "#!/bin/sh\ni=0\nwhile [ \"$i\" -lt 5000 ]; do\n  printf '0123456789abcdef0123456789abcdef\\n'\n  printf 'fedcba9876543210fedcba9876543210\\n' >&2\n  i=$((i + 1))\ndone\n",
        );
        let (status, stdout, stderr) =
            run_process(shell, &["-c", &script], Duration::from_secs(5)).expect("probe");
        assert!(status.success());
        assert!(stdout.len() > 64 * 1024);
        assert!(stderr.len() > 64 * 1024);
    }

    #[test]
    fn terminates_timed_out_probe() {
        let (shell, _dir, script) = shell_fixture("#!/bin/sh\nsleep 30\n");
        let started = Instant::now();
        let error =
            run_process(shell, &["-c", &script], Duration::from_millis(25)).expect_err("timeout");
        assert_eq!(error, "probe timed out");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn retained_descendant_pipes_cannot_extend_timeout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let pid_file = dir.path().join("descendant.pid");
        let script = "sleep 30 & printf '%s' \"$!\" > \"$1\"; exit 0";
        let pid_path = pid_file.to_str().expect("UTF-8 pid path");
        let started = Instant::now();
        let error = run_process(
            Path::new("/bin/sh"),
            &["-c", script, "probe", pid_path],
            Duration::from_millis(25),
        )
        .expect_err("retained pipe timeout");
        let elapsed = started.elapsed();
        assert_eq!(error, "probe timed out");
        assert!(
            elapsed >= Duration::from_millis(25),
            "deadline not reached: {elapsed:?}"
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "cleanup exceeded bound: {elapsed:?}"
        );
        let raw_pid = fs::read_to_string(&pid_file).expect("descendant launched");
        let pid = parse_positive_pid(&raw_pid).expect("descendant pid must be positive integer");
        let status = Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("probe descendant status");
        assert!(
            !status.success(),
            "descendant retained after timeout cleanup"
        );
    }

    #[test]
    fn rejects_malformed_pid_before_kill_check() {
        assert!(parse_positive_pid(r#"  "83995" "#).is_err());
        assert!(parse_positive_pid("0").is_err());
        assert_eq!(parse_positive_pid(" 83995\n"), Ok(83995));
    }

    #[test]
    fn pid_checker_detects_live_child_before_dead() {
        let mut child = Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("spawn lifecycle child");
        let pid = child.id();
        assert!(process_alive(pid), "checker missed live child");
        child.kill().expect("kill lifecycle child");
        child.wait().expect("reap lifecycle child");
        assert!(!process_alive(pid), "checker reported dead child alive");
    }

    fn parse_positive_pid(raw: &str) -> Result<u32, String> {
        let pid = raw
            .trim()
            .parse::<u32>()
            .map_err(|error| format!("invalid pid: {error}"))?;
        (pid > 0)
            .then_some(pid)
            .ok_or_else(|| "pid must be positive".to_string())
    }

    fn process_alive(pid: u32) -> bool {
        Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}
