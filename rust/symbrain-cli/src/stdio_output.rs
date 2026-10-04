//! Go-compatible actual stdout failures, shared by Setup and Memory completion.

use std::fmt;
use std::io::{self, Write};

pub(crate) fn pipe_boundary(error: &io::Error, process_stdout: bool) {
    #[cfg(unix)]
    if process_stdout && error.raw_os_error().is_some() && error.kind() == io::ErrorKind::BrokenPipe
    {
        // Go os.File invokes epipecheck at the failing write, rather than at
        // startup or command completion. Already committed work remains owned.
        let _ = signal_hook::low_level::emulate_default_handler(signal_hook::consts::SIGPIPE);
    }
    #[cfg(not(unix))]
    let _ = (error, process_stdout);
}

pub(crate) fn io_cause(error: &io::Error) -> String {
    let Some(code) = error.raw_os_error() else {
        return error.to_string();
    };
    let literal = error.to_string();
    let suffix = format!(" (os error {code})");
    let message = literal.strip_suffix(&suffix).unwrap_or(&literal);
    #[cfg(unix)]
    let message = message.to_lowercase();
    message.to_string()
}

pub(crate) fn go_write_error(error: &io::Error) -> String {
    if error.raw_os_error().is_some() {
        format!("write /dev/stdout: {}", io_cause(error))
    } else {
        error.to_string()
    }
}

#[derive(Debug)]
struct StdoutError(io::Error);

impl fmt::Display for StdoutError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&go_write_error(&self.0))
    }
}

impl std::error::Error for StdoutError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

pub(crate) struct Output<'a> {
    writer: &'a mut dyn Write,
    process_stdout: bool,
}

impl<'a> Output<'a> {
    pub(crate) fn new(writer: &'a mut dyn Write, process_stdout: bool) -> Self {
        Self {
            writer,
            process_stdout,
        }
    }

    fn checked<T>(&self, result: io::Result<T>) -> io::Result<T> {
        result.map_err(|error| {
            pipe_boundary(&error, self.process_stdout);
            if self.process_stdout && error.raw_os_error().is_some() {
                io::Error::new(error.kind(), StdoutError(error))
            } else {
                error
            }
        })
    }
}

impl Write for Output<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = self.writer.write(bytes);
        self.checked(result)
    }

    fn flush(&mut self) -> io::Result<()> {
        let result = self.writer.flush();
        self.checked(result)
    }
}

#[cfg(test)]
#[path = "stdio_output_tests.rs"]
mod tests;
