//! Scoped startup handles. Windows jobs survive an exited provider parent.
use std::process::Child;
use std::{
    io,
    process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    time::Duration,
};
use wait_timeout::ChildExt;

pub(super) struct OwnedCommand {
    inner: Command,
}
impl OwnedCommand {
    pub(super) fn new(command: Command) -> Self {
        Self { inner: command }
    }
    pub(super) fn spawn(&mut self) -> io::Result<OwnedChild> {
        super::spawn_command(&mut self.inner).map(|inner| OwnedChild { inner })
    }
}

pub(super) struct OwnedChild {
    inner: Child,
}
impl OwnedChild {
    pub(super) fn stdout(&mut self) -> &mut Option<ChildStdout> {
        &mut self.inner.stdout
    }
    pub(super) fn stderr(&mut self) -> &mut Option<ChildStderr> {
        &mut self.inner.stderr
    }
    pub(super) fn stdin(&mut self) -> &mut Option<ChildStdin> {
        &mut self.inner.stdin
    }
    pub(super) fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        self.inner.wait_timeout(timeout)
    }
    pub(super) fn terminate(&mut self) {
        {
            super::terminate_process_tree(&mut self.inner);
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Also close descendants which kept running after a successful parent
        // exit without holding stdout. This owns only the explicitly created job.
        self.terminate();
    }
}
