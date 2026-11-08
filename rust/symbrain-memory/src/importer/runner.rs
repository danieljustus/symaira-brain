//! Injectable external-command runner for importers that shell out to a
//! user-installed CLI (`himalaya`, `gh`, calendar tools). Production code uses
//! [`SystemRunner`]; tests inject a fake so no mailbox or network is touched.

use std::process::{Command, Stdio};

/// Runs `program args...` and returns its stdout.
pub trait Runner {
    /// # Errors
    /// Go-style text: `exec: "<program>": executable file not found in $PATH`
    /// or `exit status N`.
    fn run(&self, program: &str, args: &[String]) -> Result<Vec<u8>, String>;
}

/// Spawns the real binary (stderr is discarded, like `Output()` without a
/// captured stderr sink in the tests; Go captured it only in the error).
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<Vec<u8>, String> {
        let output = Command::new(program)
            .args(args)
            .stderr(Stdio::null())
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    format!("exec: \"{program}\": executable file not found in $PATH")
                } else {
                    e.to_string()
                }
            })?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(output.status.code().map_or_else(
                || "signal: killed".to_owned(),
                |c| format!("exit status {c}"),
            ))
        }
    }
}

/// Truncates `text` to at most `max` bytes on a character boundary.
#[must_use]
pub fn truncate_bytes(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
pub(crate) mod fake {
    use super::Runner;
    use std::cell::RefCell;

    /// Replies are matched by the first argument(s) joined with spaces.
    pub(crate) struct Fake {
        pub replies: Vec<(&'static str, Result<&'static str, &'static str>)>,
        pub calls: RefCell<Vec<String>>,
    }

    impl Fake {
        pub(crate) fn new(
            replies: Vec<(&'static str, Result<&'static str, &'static str>)>,
        ) -> Self {
            Self {
                replies,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl Runner for &Fake {
        fn run(&self, program: &str, args: &[String]) -> Result<Vec<u8>, String> {
            let line = format!("{program} {}", args.join(" "));
            self.calls.borrow_mut().push(line.clone());
            self.replies
                .iter()
                .find(|(needle, _)| line.contains(needle))
                .map_or_else(
                    || Err(format!("unexpected command: {line}")),
                    |(_, reply)| reply.map(|s| s.as_bytes().to_vec()).map_err(str::to_owned),
                )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_char_boundaries() {
        for (text, max, want) in [
            ("hello", 10, "hello"),
            ("hello", 3, "hel"),
            ("aé", 2, "a"),
            ("aé", 3, "aé"),
            ("", 0, ""),
        ] {
            assert_eq!(truncate_bytes(text, max), want, "{text:?}/{max}");
        }
    }

    #[test]
    fn system_runner_reports_go_style_errors() {
        let missing = SystemRunner
            .run("symbrain-no-such-binary-xyz", &[])
            .unwrap_err();
        assert_eq!(
            missing,
            "exec: \"symbrain-no-such-binary-xyz\": executable file not found in $PATH"
        );
        let failed = SystemRunner
            .run("sh", &["-c".into(), "exit 3".into()])
            .unwrap_err();
        assert_eq!(failed, "exit status 3");
    }
}
