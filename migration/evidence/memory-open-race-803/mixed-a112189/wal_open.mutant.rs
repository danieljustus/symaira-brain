//! Bound only the pre-migration WAL pragma paths that bypass the busy handler.

use rusqlite::{Connection, Error};
use std::time::{Duration, Instant};

const BUDGET: Duration = Duration::from_secs(5);
const YIELD: Duration = Duration::from_millis(5);

pub(crate) fn ensure_wal(conn: &Connection) -> Result<(), Error> {
    with_busy_observer(conn, || {})
}

fn with_busy_observer(conn: &Connection, observer: impl FnMut()) -> Result<(), Error> {
    let result = attempts(conn, observer);
    let restored = conn.busy_timeout(BUDGET);
    // Preserve the original SQL failure even if restoring a closing/error
    // connection's handler were to fail. Successful setup requires restoration.
    match result {
        Ok(()) => restored,
        Err(error) => Err(error),
    }
}

fn attempts(conn: &Connection, mut observer: impl FnMut()) -> Result<(), Error> {
    let started = Instant::now();
    loop {
        // SQLite's own waiting and our yielding share one monotonic budget.
        // In particular no early BUSY grants a fresh five-second timeout.
        conn.busy_timeout(BUDGET)?;
        match conn.execute_batch("PRAGMA journal_mode=WAL;") {
            Ok(()) => return Ok(()),
            Err(error) => {
                let remaining = BUDGET.saturating_sub(started.elapsed());
                if !plain_busy(&error) || !conn.is_autocommit() || remaining.is_zero() {
                    return Err(error);
                }
                // The failed statement is finalized before this callback/yield;
                // no explicit transaction or migration statement is replayed.
                observer();
                std::thread::sleep(YIELD.min(BUDGET.saturating_sub(started.elapsed())));
                if started.elapsed() >= BUDGET {
                    return Err(error);
                }
            }
        }
    }
}

fn plain_busy(error: &Error) -> bool {
    matches!(error, Error::SqliteFailure(code, _) if code.extended_code == rusqlite::ffi::SQLITE_BUSY)
}

#[cfg(test)]
mod tests;
