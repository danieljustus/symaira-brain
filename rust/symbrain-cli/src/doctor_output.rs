//! Doctor's ignored fmt writes preserve one submission per Go operation.
use std::fmt;
use std::io::{self, Write};

pub(super) struct GoWriter<'a>(&'a mut dyn Write);

impl<'a> GoWriter<'a> {
    pub(super) fn new(writer: &'a mut dyn Write) -> Self {
        Self(writer)
    }
}

impl Write for GoWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        // A failing fmt call never retries its unwritten suffix. The next fmt
        // call still runs, including with recovering embedded writers. The
        // underlying process-only adapter handles real SIGPIPE first.
        let _ = self.0.write(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = self.0.flush();
        Ok(())
    }

    fn write_fmt(&mut self, arguments: fmt::Arguments<'_>) -> io::Result<()> {
        let bytes = fmt::format(arguments);
        self.write(bytes.as_bytes()).map(|_| ())
    }
}

#[cfg(test)]
#[path = "doctor_output_tests.rs"]
mod tests;
