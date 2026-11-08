//! Bounded, background execution of the first-run backup for entrypoints.
//!
//! A binary starts the backup with [`start_for_binary`], waits at most
//! [`START_GRACE`] in [`Backup::settle`], runs its command, and gives the
//! worker at most [`EXIT_GRACE`] in [`Backup::finish`]. A backup that is still
//! running after the grace period keeps going next to the command (`symbrain
//! mcp` answers `initialize` immediately) and reports on stderr when done;
//! stdout is never touched.

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::{DISABLE_ENV, Homes, Outcome, Root, disabled, ensure_cancellable};

/// How long an entrypoint waits for the backup before it starts the command.
/// A typical state tree is copied within this time, so the command still
/// starts after a finished backup and a failed one still stops it.
pub const START_GRACE: Duration = Duration::from_secs(3);
/// How long [`Backup::finish`] lets a running backup continue after the
/// command returned before the copy is cancelled and retried on the next run.
pub const EXIT_GRACE: Duration = Duration::from_secs(5);
/// Time a cancelled worker gets to remove its partial copy.
const CANCEL_GRACE: Duration = Duration::from_secs(2);

/// Handle of a backup running on a worker thread.
///
/// Dropping it cancels a still-running copy, so no orphaned partial tree is
/// left behind by a normal exit.
pub struct Backup {
    app: String,
    done: Option<Receiver<bool>>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    ok: bool,
}

/// Starts the backup of `app` on a worker thread and returns immediately.
///
/// Nothing runs when [`disabled`] or without a usable `$HOME`.
pub fn start_for_binary(app: &str, roots: impl FnOnce(&Homes) -> Vec<Root>) -> Backup {
    if disabled() {
        return Backup::idle(app, true);
    }
    let Some(homes) = Homes::from_env() else {
        return Backup::idle(app, true);
    };
    Backup::spawn(app, roots(&homes), homes.store())
}

/// Runs the backup to completion and reports on stderr (blocking variant).
///
/// Returns `false` when the backup failed; the caller must then stop before
/// touching state. Success, skip and disabled states return `true`. Prefer
/// [`start_for_binary`], which never blocks longer than [`START_GRACE`].
pub fn run_for_binary(app: &str, roots: impl FnOnce(&Homes) -> Vec<Root>) -> bool {
    start_for_binary(app, roots).block()
}

impl Backup {
    fn idle(app: &str, ok: bool) -> Self {
        Self {
            app: app.to_string(),
            done: None,
            cancel: Arc::new(AtomicBool::new(false)),
            worker: None,
            ok,
        }
    }

    pub(crate) fn spawn(app: &str, roots: Vec<Root>, store: PathBuf) -> Self {
        let mut backup = Self::idle(app, true);
        let (sender, receiver) = mpsc::channel();
        let (name, cancel) = (app.to_string(), Arc::clone(&backup.cancel));
        let spawned = std::thread::Builder::new()
            .name("cutover-backup".into())
            .spawn(move || {
                let result = ensure_cancellable(&name, &roots, &store, &cancel);
                let _ = sender.send(report(&name, result));
            });
        match spawned {
            Ok(worker) => {
                backup.worker = Some(worker);
                backup.done = Some(receiver);
            }
            Err(error) => backup.ok = report(app, Err(error)),
        }
        backup
    }

    /// Waits up to `budget` for the backup. Returns `false` only when it
    /// finished and failed; a backup that is still running yields `true`
    /// after a stderr notice, and the command proceeds next to it.
    pub fn settle(&mut self, budget: Duration) -> bool {
        if let Some(ok) = self.wait(budget) {
            return ok;
        }
        notice(&format!(
            "{}: backing up existing state in the background; continuing without waiting (the backup is verified before it counts)",
            self.app
        ));
        true
    }

    /// Waits for the backup without a time limit.
    pub fn block(&mut self) -> bool {
        let Some(done) = &self.done else {
            return self.ok;
        };
        self.ok = done.recv().unwrap_or_else(|_| lost(&self.app));
        self.done = None;
        self.ok
    }

    /// Lets a running backup continue for at most `budget`, then cancels it
    /// (its partial copy is removed; the next run starts over).
    pub fn finish(mut self, budget: Duration) {
        let _ = self.wait(budget);
    }

    /// `None` while the worker is still running after `budget`.
    fn wait(&mut self, budget: Duration) -> Option<bool> {
        let Some(done) = &self.done else {
            return Some(self.ok);
        };
        match done.recv_timeout(budget) {
            Ok(ok) => self.ok = ok,
            Err(RecvTimeoutError::Timeout) => return None,
            Err(RecvTimeoutError::Disconnected) => self.ok = lost(&self.app),
        }
        self.done = None;
        Some(self.ok)
    }
}

impl Drop for Backup {
    fn drop(&mut self) {
        let Some(worker) = self.worker.take() else {
            return;
        };
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(done) = self.done.take() {
            let _ = done.recv_timeout(CANCEL_GRACE);
        }
        // A worker still verifying a huge tree is abandoned: it dies with the
        // process and the next run removes its partial copy.
        if worker.is_finished() {
            let _ = worker.join();
        }
    }
}

fn notice(message: &str) {
    let _ = writeln!(io::stderr(), "{message}");
}

fn lost(app: &str) -> bool {
    notice(&format!(
        "{app}: automatic state backup before the first Rust run failed: the backup worker stopped unexpectedly\n\
         {app}: no backup marker was written; the backup is retried on the next run"
    ));
    false
}

/// Prints the outcome of one backup; `false` marks a failed backup.
fn report(app: &str, result: io::Result<Outcome>) -> bool {
    match result {
        Ok(Outcome::Created { dir, files, .. }) => {
            notice(&format!(
                "{app}: backed up existing state ({files} files) to {} before the first Rust run",
                dir.display()
            ));
            true
        }
        Ok(Outcome::InProgress) => {
            notice(&format!(
                "{app}: another process is already backing up existing state; continuing"
            ));
            true
        }
        Ok(_) => true,
        Err(error) if error.kind() == io::ErrorKind::Interrupted => {
            notice(&format!(
                "{app}: state backup paused before it finished; it restarts on the next run"
            ));
            true
        }
        Err(error) => {
            notice(&format!(
                "{app}: automatic state backup before the first Rust run failed: {error}\n\
                 {app}: free space or fix permissions and retry, or set {DISABLE_ENV}=off to skip the backup"
            ));
            false
        }
    }
}
