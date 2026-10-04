//! Real rollback-reader contention and typed retry boundaries, never fake rows.

use super::*;
use std::cell::Cell;
use std::{path::PathBuf, sync::mpsc};

thread_local! {
    static BUSY_CALLBACKS: Cell<usize> = const { Cell::new(0) };
}

fn count_busy_callback(_attempt: i32) -> bool {
    BUSY_CALLBACKS.with(|calls| calls.set(calls.get() + 1));
    false
}

struct OwnedDatabase(PathBuf);

impl OwnedDatabase {
    fn new() -> Self {
        let mut entropy = [0_u8; 16];
        getrandom::fill(&mut entropy).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "symbrain-wal-open-{:032x}",
            u128::from_le_bytes(entropy)
        ));
        std::fs::create_dir(&directory).unwrap();
        Self(directory.join("memory.db"))
    }
}

impl Drop for OwnedDatabase {
    fn drop(&mut self) {
        std::fs::remove_dir_all(self.0.parent().unwrap()).unwrap();
    }
}

fn retained_reader(database: &OwnedDatabase) -> Connection {
    let conn = Connection::open(&database.0).unwrap();
    conn.execute_batch("PRAGMA journal_mode=DELETE; CREATE TABLE retained(id TEXT PRIMARY KEY, value TEXT NOT NULL); INSERT INTO retained VALUES ('id','unchanged'); BEGIN;").unwrap();
    assert_eq!(
        conn.query_row("SELECT value FROM retained", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "unchanged"
    );
    conn
}

fn retained_writer(database: &OwnedDatabase) -> Connection {
    let conn = retained_reader(database);
    // A reader permits RESERVED acquisition, then blocks EXCLUSIVE (handler
    // used). A reserved writer blocks SHARED-to-RESERVED (handler skipped).
    conn.execute_batch("ROLLBACK; BEGIN IMMEDIATE;").unwrap();
    conn
}

#[test]
fn only_observed_plain_busy_is_retryable() {
    for code in [
        rusqlite::ffi::SQLITE_BUSY,
        rusqlite::ffi::SQLITE_BUSY_RECOVERY,
        rusqlite::ffi::SQLITE_BUSY_SNAPSHOT,
        rusqlite::ffi::SQLITE_BUSY_TIMEOUT,
        rusqlite::ffi::SQLITE_LOCKED,
        rusqlite::ffi::SQLITE_IOERR,
        rusqlite::ffi::SQLITE_CORRUPT,
    ] {
        let error = Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            Some("not classified by this text".into()),
        );
        assert_eq!(plain_busy(&error), code == rusqlite::ffi::SQLITE_BUSY);
    }
}

#[test]
fn genuine_wal_upgrade_busy_succeeds_after_reserved_writer_release() {
    let database = OwnedDatabase::new();
    let writer = retained_writer(&database);
    let follower = Connection::open(&database.0).unwrap();
    // The exact isolated pragma is the real failing phase, not Store::open's
    // outer error. Record its typed failure before exercising the helper.
    BUSY_CALLBACKS.with(|calls| calls.set(0));
    follower.busy_handler(Some(count_busy_callback)).unwrap();
    let original = follower
        .execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA secure_delete=ON;")
        .unwrap_err();
    assert!(plain_busy(&original), "{original}");
    assert_eq!(
        BUSY_CALLBACKS.with(Cell::get),
        0,
        "the real WAL upgrade bypasses SQLite's busy callback"
    );
    assert!(follower.is_autocommit());
    eprintln!(
        "original pragma batch under reserved writer: {original:?}; actual busy callbacks=0; autocommit=true"
    );
    let (observed, received) = mpsc::channel();
    let job = std::thread::spawn(move || {
        let mut first = Some(observed);
        with_busy_observer(&follower, || {
            if let Some(sender) = first.take() {
                sender.send(()).unwrap();
            }
        })
        .unwrap();
        // Continue through the actual unchanged IMMEDIATE migration owner;
        // application state in the unrelated preexisting table must survive.
        let store = crate::migration::configure(follower).unwrap();
        let follower = store.lock().unwrap();
        assert_eq!(
            follower
                .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            follower
                .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            5000
        );
        assert_eq!(
            follower
                .query_row("SELECT value FROM retained", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "unchanged"
        );
        eprintln!(
            "observed genuine early WAL BUSY then released reserved writer: actual IMMEDIATE migration complete; journal=wal; busy_timeout=5000; retained row unchanged"
        );
    });
    // Release only after an actual failed helper attempt. There is no assumed
    // sleep duration or weakened concurrency assertion.
    received.recv_timeout(BUDGET).unwrap();
    writer.execute_batch("COMMIT;").unwrap();
    job.join().unwrap();
}

#[test]
fn retained_reader_exhausts_one_budget_and_preserves_original_error_and_rows() {
    let database = OwnedDatabase::new();
    let reader = retained_reader(&database);
    let follower = Connection::open(&database.0).unwrap();
    let started = Instant::now();
    let error = ensure_wal(&follower).unwrap_err();
    assert!(plain_busy(&error), "{error}");
    assert!(started.elapsed() >= BUDGET);
    // Scheduler delays are outside SQLite's timeout, so timing is recorded
    // rather than mistaken for a portable upper-bound performance assertion.
    eprintln!(
        "retained WAL reader exhausted shared budget: {:?}",
        started.elapsed()
    );
    assert!(follower.is_autocommit());
    assert_eq!(
        follower
            .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        5000
    );
    assert_eq!(
        reader
            .query_row("SELECT value FROM retained", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "unchanged"
    );
    reader.execute_batch("COMMIT;").unwrap();
    assert_eq!(
        follower
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
}

#[test]
fn many_skipped_handler_attempts_share_one_budget_under_retained_writer() {
    let database = OwnedDatabase::new();
    let writer = retained_writer(&database);
    let follower = Connection::open(&database.0).unwrap();
    let mut attempts = 0_u32;
    let started = Instant::now();
    let error = with_busy_observer(&follower, || attempts += 1).unwrap_err();
    assert!(plain_busy(&error), "{error}");
    assert!(
        attempts > 1,
        "this control must exercise actual early BUSY retries"
    );
    assert!(started.elapsed() >= BUDGET);
    eprintln!(
        "retained reserved WAL writer: {attempts} real early BUSY attempts in {:?}; final error={error:?}",
        started.elapsed()
    );
    assert!(follower.is_autocommit());
    assert_eq!(
        follower
            .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        5000
    );
    assert_eq!(
        writer
            .query_row("SELECT value FROM retained", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "unchanged"
    );
    writer.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(
        follower
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
}

#[test]
fn early_writer_then_waiting_reader_share_the_actual_sqlite_timeout() {
    let database = OwnedDatabase::new();
    let writer = retained_writer(&database);
    let reader = Connection::open(&database.0).unwrap();
    reader.execute_batch("BEGIN;").unwrap();
    assert_eq!(
        reader
            .query_row("SELECT value FROM retained", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "unchanged"
    );
    let follower = Connection::open(&database.0).unwrap();
    let started = Instant::now();
    let mut early_attempts = 0_u32;
    let mut released_at = None;
    // Keep the reader's actual SHARED lock throughout. Only after repeated
    // real skipped-handler failures release RESERVED, so the next pragma
    // acquires RESERVED and genuinely waits for that reader's EXCLUSIVE lock.
    let error = attempts(&follower, || {
        early_attempts += 1;
        if released_at.is_none() && started.elapsed() >= Duration::from_secs(1) {
            writer.execute_batch("ROLLBACK;").unwrap();
            released_at = Some(started.elapsed());
        }
    })
    .unwrap_err();
    let elapsed = started.elapsed();
    let released_at = released_at.expect("real writer phase must precede reader wait");
    let last_timeout = follower
        .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, u32>(0))
        .unwrap();
    assert!(plain_busy(&error), "{error}");
    assert!(early_attempts > 1);
    // Inspect the actual SQLite handler budget before the wrapper restores it.
    // A fresh-five-second-per-attempt mutant deterministically leaves 5000;
    // no scheduler-sensitive upper elapsed-time assertion is needed.
    assert!(last_timeout > 0 && last_timeout < 4000, "{last_timeout}");
    assert!(elapsed - released_at >= Duration::from_millis(u64::from(last_timeout)));
    assert!(elapsed >= BUDGET);
    assert!(follower.is_autocommit());
    assert_eq!(
        follower
            .query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(
        reader
            .query_row("SELECT value FROM retained", [], |row| row
                .get::<_, String>(0))
            .unwrap(),
        "unchanged"
    );
    eprintln!(
        "mixed real WAL contention: {early_attempts} early BUSY attempts; writer released at {released_at:?}; final actual SQLite timeout={last_timeout}ms; total={elapsed:?}; error={error:?}"
    );
    reader.execute_batch("COMMIT;").unwrap();
}

#[test]
fn caller_transaction_is_not_retried_or_rolled_back() {
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE TABLE retained(value TEXT); BEGIN; INSERT INTO retained VALUES ('caller-owned');",
    )
    .unwrap();
    let result = with_busy_observer(&conn, || panic!("active caller transaction cannot retry"));
    // SQLite may leave an in-memory journal unchanged without raising an error;
    // either result must preserve caller ownership and its uncommitted row.
    if let Err(error) = result {
        assert!(!plain_busy(&error));
    }
    assert!(!conn.is_autocommit());
    assert_eq!(
        conn.query_row("SELECT value FROM retained", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "caller-owned"
    );
    conn.execute_batch("ROLLBACK;").unwrap();
}

#[test]
fn disk_read_transaction_retains_ownership_after_forbidden_mode_change() {
    let database = OwnedDatabase::new();
    let conn = retained_reader(&database);
    let error =
        with_busy_observer(&conn, || panic!("caller transaction cannot retry")).unwrap_err();
    assert!(!plain_busy(&error));
    assert!(!conn.is_autocommit());
    assert_eq!(
        conn.query_row("SELECT value FROM retained", [], |row| row
            .get::<_, String>(0))
            .unwrap(),
        "unchanged"
    );
    conn.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(
        conn.query_row("PRAGMA journal_mode", [], |row| row.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
}
