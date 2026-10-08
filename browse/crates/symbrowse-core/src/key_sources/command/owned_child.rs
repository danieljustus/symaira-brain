//! Scoped startup handles. Windows jobs survive an exited provider parent.
#[cfg(windows)]
use process_wrap::std::{ChildWrapper, CommandWrap, CreationFlags, JobObject};
#[cfg(not(windows))]
use std::process::Child;
use std::{
    io,
    process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    time::Duration,
};
#[cfg(not(windows))]
use wait_timeout::ChildExt;

pub(super) struct OwnedCommand {
    #[cfg(windows)]
    inner: CommandWrap,
    #[cfg(not(windows))]
    inner: Command,
}
impl OwnedCommand {
    pub(super) fn new(command: Command) -> Self {
        #[cfg(windows)]
        {
            let mut inner = CommandWrap::from(command);
            // JobObject temporarily suspends the new child, assigns the owned
            // job and resumes it. Descendants cannot race assignment or become
            // invisible merely because the provider parent has exited.
            let mut flags = CreationFlags(Default::default());
            flags.0.0 = 0x0000_0200; // Existing CREATE_NEW_PROCESS_GROUP policy.
            inner.wrap(flags).wrap(JobObject);
            Self { inner }
        }
        #[cfg(not(windows))]
        {
            Self { inner: command }
        }
    }
    pub(super) fn spawn(&mut self) -> io::Result<OwnedChild> {
        #[cfg(windows)]
        {
            self.inner.spawn().map(|inner| OwnedChild { inner })
        }
        #[cfg(not(windows))]
        {
            super::spawn_command(&mut self.inner).map(|inner| OwnedChild { inner })
        }
    }
}

pub(super) struct OwnedChild {
    #[cfg(windows)]
    inner: Box<dyn ChildWrapper>,
    #[cfg(not(windows))]
    inner: Child,
}
impl OwnedChild {
    pub(super) fn stdout(&mut self) -> &mut Option<ChildStdout> {
        #[cfg(windows)]
        {
            self.inner.stdout()
        }
        #[cfg(not(windows))]
        {
            &mut self.inner.stdout
        }
    }
    pub(super) fn stderr(&mut self) -> &mut Option<ChildStderr> {
        #[cfg(windows)]
        {
            self.inner.stderr()
        }
        #[cfg(not(windows))]
        {
            &mut self.inner.stderr
        }
    }
    pub(super) fn stdin(&mut self) -> &mut Option<ChildStdin> {
        #[cfg(windows)]
        {
            self.inner.stdin()
        }
        #[cfg(not(windows))]
        {
            &mut self.inner.stdin
        }
    }
    pub(super) fn wait_timeout(&mut self, timeout: Duration) -> io::Result<Option<ExitStatus>> {
        #[cfg(windows)]
        {
            // The safe wrapper's nonblocking wait preserves the direct
            // provider's status; never call its unbounded whole-job wait.
            let deadline = std::time::Instant::now() + timeout;
            loop {
                if let Some(status) = self.inner.try_wait()? {
                    return Ok(Some(status));
                }
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Ok(None);
                }
                std::thread::sleep(remaining.min(Duration::from_millis(20)));
            }
        }
        #[cfg(not(windows))]
        {
            self.inner.wait_timeout(timeout)
        }
    }
    pub(super) fn terminate(&mut self) {
        #[cfg(windows)]
        {
            let _ = self.inner.start_kill(); // TerminateJobObject, not PID enumeration.
            let _ = self.wait_timeout(Duration::from_secs(1));
        }
        #[cfg(not(windows))]
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
