//! Actual process termination diagnostics used by the frozen Go CLI.
use std::process::ExitStatus;

/// Formats a native process exit using Go's platform status diagnostics.
#[must_use]
pub fn format(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit status {code}");
    }
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
}
