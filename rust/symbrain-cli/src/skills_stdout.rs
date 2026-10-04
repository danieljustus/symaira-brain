//! Only the executable's Skills fd1 boundary owns Go stdout OS semantics.
use std::io::{self, Write};

pub(super) struct SkillsStdout(io::Stdout);
impl SkillsStdout {
    pub(super) fn new(stdout: io::Stdout) -> Self {
        Self(stdout)
    }
}
impl Write for SkillsStdout {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes).map_err(stdout_error)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush().map_err(stdout_error)
    }
}
fn stdout_error(error: io::Error) -> io::Error {
    #[cfg(unix)]
    if error.kind() == io::ErrorKind::BrokenPipe && error.raw_os_error().is_some() {
        // A completed sync is retained. Only an actual fd1 OS write failure
        // emulates Go runtime.sigpipe; no standalone writer or signal reset.
        let _ = signal_hook::low_level::emulate_default_handler(signal_hook::consts::SIGPIPE);
    }
    let Some(code) = error.raw_os_error() else {
        return error;
    };
    let literal = error.to_string();
    let suffix = format!(" (os error {code})");
    let text = literal.strip_suffix(&suffix).unwrap_or(&literal);
    #[cfg(unix)]
    let text = text.to_lowercase();
    io::Error::new(error.kind(), format!("write /dev/stdout: {text}"))
}
