//! Actual process stdout for `symbrain skills` reports.
//!
//! `std::io::Stdout` treats `EBADF` as success, so a readonly or closed
//! descriptor 1 swallows the report and the command exits 0. Go's `os.Stdout`
//! surfaces `write /dev/stdout: bad file descriptor` instead. Writing through a
//! safe duplicate of the existing descriptor (created per write, never by
//! reopening a path) reports the real OS error and leaves the sink untouched.

use std::io::{self, Write};

/// Unbuffered writer over a duplicate of the process's stdout descriptor.
pub(crate) struct ProcessStdout;

impl Write for ProcessStdout {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        duplicate()?.write(bytes)
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        duplicate()?.write_all(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Nothing is buffered here; every write goes straight to the kernel.
        Ok(())
    }
}

fn duplicate() -> io::Result<std::fs::File> {
    use std::os::fd::AsFd;
    io::stdout().as_fd().try_clone_to_owned().map(Into::into)
}
