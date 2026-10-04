//! Preserve owned JobObject completion notifications until whole-tree cleanup.
use process_wrap::std::ChildWrapper;
use std::io;
use std::process::ExitStatus;

pub(super) fn poll_parent(child: &mut dyn ChildWrapper) -> io::Result<Option<ExitStatus>> {
    // process-wrap 10.0.1 JobObject try_wait drains ACTIVE_PROCESS_ZERO but
    // does not populate the wrapper's wait cache. Polling its inner std Child
    // preserves those notifications for the eventual whole-job wait.
    child.inner_mut().try_wait()
}

pub(super) fn terminate_and_reap(child: &mut dyn ChildWrapper) -> io::Result<ExitStatus> {
    // Terminate the entire owned job, then await all of it through the wrapper.
    // Waiting still happens after a termination error, matching Owned's Drop.
    let terminated = child.start_kill();
    let waited = child.wait();
    terminated.and(waited)
}
