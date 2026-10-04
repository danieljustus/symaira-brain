//! Go ProcessState diagnostics for the additive raw Memory reference owner.
use std::process::ExitStatus;

pub(super) fn go(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        #[cfg(windows)]
        {
            let code = u32::from_ne_bytes(code.to_ne_bytes());
            if code >= 1 << 16 { return format!("exit status 0x{code:x}"); }
        }
        return format!("exit status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let signal = status.signal().unwrap_or(0);
        let name = signals().iter().find(|(number, _)| *number == signal)
            .map_or_else(|| format!("signal {signal}"), |(_, name)| (*name).to_owned());
        let suffix = if status.core_dumped() { " (core dumped)" } else { "" };
        return format!("signal: {name}{suffix}");
    }
    #[cfg(not(unix))]
    "exit status -1".to_owned()
}

#[cfg(target_os = "linux")]
fn signals() -> &'static [(i32, &'static str)] {
    &[
    (1, "hangup"),
    (2, "interrupt"),
    (3, "quit"),
    (4, "illegal instruction"),
    (5, "trace/breakpoint trap"),
    (6, "aborted"),
    (7, "bus error"),
    (8, "floating point exception"),
    (9, "killed"),
    (10, "user defined signal 1"),
    (11, "segmentation fault"),
    (12, "user defined signal 2"),
    (13, "broken pipe"),
    (14, "alarm clock"),
    (15, "terminated"),
    (16, "stack fault"),
    (17, "child exited"),
    (18, "continued"),
    (19, "stopped (signal)"),
    (20, "stopped"),
    (21, "stopped (tty input)"),
    (22, "stopped (tty output)"),
    (23, "urgent I/O condition"),
    (24, "CPU time limit exceeded"),
    (25, "file size limit exceeded"),
    (26, "virtual timer expired"),
    (27, "profiling timer expired"),
    (28, "window changed"),
    (29, "I/O possible"),
    (30, "power failure"),
    (31, "bad system call"),
    ]
}

#[cfg(target_os = "macos")]
fn signals() -> &'static [(i32, &'static str)] {
    &[
    (1, "hangup"),
    (2, "interrupt"),
    (3, "quit"),
    (4, "illegal instruction"),
    (5, "trace/BPT trap"),
    (6, "abort trap"),
    (7, "EMT trap"),
    (8, "floating point exception"),
    (9, "killed"),
    (10, "bus error"),
    (11, "segmentation fault"),
    (12, "bad system call"),
    (13, "broken pipe"),
    (14, "alarm clock"),
    (15, "terminated"),
    (16, "urgent I/O condition"),
    (17, "suspended (signal)"),
    (18, "suspended"),
    (19, "continued"),
    (20, "child exited"),
    (21, "stopped (tty input)"),
    (22, "stopped (tty output)"),
    (23, "I/O possible"),
    (24, "cputime limit exceeded"),
    (25, "filesize limit exceeded"),
    (26, "virtual timer expired"),
    (27, "profiling timer expired"),
    (28, "window size changes"),
    (29, "information request"),
    (30, "user defined signal 1"),
    (31, "user defined signal 2"),
    ]
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn signals() -> &'static [(i32, &'static str)] { &[] }

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::process::ExitStatusExt;
    #[test]
    fn preserves_distinct_signals_exit_and_core_bit() {
        for (raw, expected) in [(15, "signal: terminated"), (13, "signal: broken pipe"), (9, "signal: killed"), (7 << 8, "exit status 7"), (9 | 128, "signal: killed (core dumped)")] {
            assert_eq!(super::go(std::process::ExitStatus::from_raw(raw)), expected);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use std::os::windows::process::ExitStatusExt;
    #[test]
    fn preserves_unsigned_hex_windows_status() {
        for (raw, expected) in [(7, "exit status 7"), (65_536, "exit status 0x10000"), (0xc000_0005, "exit status 0xc0000005")] {
            assert_eq!(super::go(std::process::ExitStatus::from_raw(raw)), expected);
        }
    }
}
