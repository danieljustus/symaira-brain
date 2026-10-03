//! Actual process termination diagnostics used by the frozen Go CLI.
use std::process::ExitStatus;

pub(super) fn format(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        #[cfg(windows)]
        {
            let unsigned = u32::from_ne_bytes(code.to_ne_bytes());
            if unsigned >= 1 << 16 {
                return format!("exit status 0x{unsigned:x}");
            }
        }
        return format!("exit status {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        let signal = status.signal().unwrap_or_default();
        let name = usize::try_from(signal)
            .ok()
            .and_then(|index| SIGNAL_NAMES.get(index))
            .filter(|name| !name.is_empty())
            .map_or_else(|| format!("signal {signal}"), |name| (*name).into());
        let core_dump = if status.core_dumped() {
            " (core dumped)"
        } else {
            ""
        };
        format!("signal: {name}{core_dump}")
    }
    #[cfg(not(unix))]
    {
        status.to_string()
    }
}

// Platform signal names follow Go's Linux/Darwin process-status contract.
#[cfg(all(unix, not(target_os = "macos")))]
const SIGNAL_NAMES: &[&str] = &[
    "",
    "hangup",
    "interrupt",
    "quit",
    "illegal instruction",
    "trace/breakpoint trap",
    "aborted",
    "bus error",
    "floating point exception",
    "killed",
    "user defined signal 1",
    "segmentation fault",
    "user defined signal 2",
    "broken pipe",
    "alarm clock",
    "terminated",
    "stack fault",
    "child exited",
    "continued",
    "stopped (signal)",
    "stopped",
    "stopped (tty input)",
    "stopped (tty output)",
    "urgent I/O condition",
    "CPU time limit exceeded",
    "file size limit exceeded",
    "virtual timer expired",
    "profiling timer expired",
    "window changed",
    "I/O possible",
    "power failure",
    "bad system call",
];

#[cfg(target_os = "macos")]
const SIGNAL_NAMES: &[&str] = &[
    "",
    "hangup",
    "interrupt",
    "quit",
    "illegal instruction",
    "trace/BPT trap",
    "abort trap",
    "EMT trap",
    "floating point exception",
    "killed",
    "bus error",
    "segmentation fault",
    "bad system call",
    "broken pipe",
    "alarm clock",
    "terminated",
    "urgent I/O condition",
    "suspended (signal)",
    "suspended",
    "continued",
    "child exited",
    "stopped (tty input)",
    "stopped (tty output)",
    "I/O possible",
    "cputime limit exceeded",
    "filesize limit exceeded",
    "virtual timer expired",
    "profiling timer expired",
    "window size changes",
    "information request",
    "user defined signal 1",
    "user defined signal 2",
];

#[cfg(test)]
mod tests {
    use super::format;
    #[cfg(unix)]
    #[test]
    fn unix_termination_keeps_the_actual_signal_and_core_flag() {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            format(std::process::ExitStatus::from_raw(15)),
            "signal: terminated"
        );
        assert_eq!(
            format(std::process::ExitStatus::from_raw(2)),
            "signal: interrupt"
        );
        assert_eq!(
            format(std::process::ExitStatus::from_raw(0x0b | 0x80)),
            "signal: segmentation fault (core dumped)"
        );
        assert_eq!(
            format(std::process::ExitStatus::from_raw(42 << 8)),
            "exit status 42"
        );
    }
    #[cfg(windows)]
    #[test]
    fn windows_large_status_codes_preserve_unsigned_hex() {
        use std::os::windows::process::ExitStatusExt;
        for (code, expected) in [
            (42, "exit status 42"),
            (65535, "exit status 65535"),
            (65536, "exit status 0x10000"),
            (0xc000_0005, "exit status 0xc0000005"),
        ] {
            assert_eq!(format(std::process::ExitStatus::from_raw(code)), expected);
        }
    }
}
