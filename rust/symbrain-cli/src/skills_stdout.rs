//! Only the executable's Skills fd1 boundary owns Go stdout OS semantics.
use std::io::{self, Write};
#[cfg(unix)]
use std::os::fd::AsFd;
#[cfg(windows)]
use std::os::windows::io::AsHandle;

pub(super) struct SkillsStdout(io::Stdout);
impl SkillsStdout {
    pub(super) fn new(stdout: io::Stdout) -> Self {
        Self(stdout)
    }
}
impl Write for SkillsStdout {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // Stdout suppresses bad-descriptor errors. Clone its existing handle
        // only when writing so fd flags and commands with no output survive.
        #[cfg(unix)]
        let owned = self.0.as_fd().try_clone_to_owned();
        #[cfg(windows)]
        let owned = self.0.as_handle().try_clone_to_owned();
        #[cfg(any(unix, windows))]
        {
            std::fs::File::from(owned.map_err(stdout_error)?)
                .write(bytes)
                .map_err(stdout_error)
        }
        #[cfg(not(any(unix, windows)))]
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
