//! Atomic owned schema/data/FTS transitions, with truthful ordered bookkeeping.

mod backfill;
mod catalog;
mod facts;
mod fts;
mod initial;
#[cfg(unix)]
mod permissions;
mod runner;
mod sql;

#[cfg(unix)]
pub(crate) use permissions::secure_files;
#[cfg(test)]
mod backfill_tests;
#[cfg(test)]
mod fts_tests;
#[cfg(test)]
mod historical_tests;
#[cfg(test)]
mod rollback_tests;
#[cfg(test)]
mod sql_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

use crate::{Store, StoreError};
use rusqlite::{Connection, TransactionBehavior};
use std::time::Duration;

pub(crate) fn configure(mut conn: Connection) -> Result<Store, StoreError> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA secure_delete=ON;",
    )?;
    // Reserve before inspection: busy_timeout cannot retry a deferred snapshot
    // upgrade after another opener commits (the former BUSY_SNAPSHOT517 bug).
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    runner::apply(&tx)?;
    tx.commit()?;
    Ok(Store {
        conn: std::sync::Mutex::new(conn),
    })
}
