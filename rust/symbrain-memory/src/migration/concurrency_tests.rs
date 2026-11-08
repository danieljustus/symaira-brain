//! #825: concurrent opens of an already-current store never need the writer.
//!
//! The Linux coverage failure surfaced as `DatabaseBusy` while eight workers
//! opened one store. Before #957 every open reserved the single SQLite writer
//! (`BEGIN IMMEDIATE`) and ran the whole migration, so openers queued behind
//! each other and could exhaust the five-second busy budget on a slow
//! (instrumented) runner. This control removes timing from the verdict: a
//! writer is retained for the whole observation, so an open can only finish
//! while it is held if it never asks for the writer.

use super::*;
use std::sync::{Arc, Barrier, mpsc};
use std::time::{Duration, Instant};

const OPENERS: usize = 8;

#[test]
fn concurrent_opens_of_a_current_store_complete_while_the_writer_is_retained() {
    let database = test_support::OwnedDatabase::new();
    let id = {
        let store = Store::open(database.path()).unwrap();
        store
            .set(
                "current store row",
                "global",
                "user",
                serde_json::Map::new(),
                false,
            )
            .unwrap()
            .id
    };
    let writer = Connection::open(database.path()).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    let barrier = Arc::new(Barrier::new(OPENERS));
    let (sender, receiver) = mpsc::channel();
    let threads = (0..OPENERS)
        .map(|_| {
            let (path, id, barrier, sender) = (
                database.path().to_path_buf(),
                id.clone(),
                barrier.clone(),
                sender.clone(),
            );
            std::thread::spawn(move || {
                barrier.wait();
                let started = Instant::now();
                let opened = Store::open(&path).map(|store| store.get(&id).unwrap().is_some());
                let _ = sender.send((opened.map_err(|error| error.to_string()), started.elapsed()));
            })
        })
        .collect::<Vec<_>>();
    drop(sender);
    // Far above any open, far below "waited for a writer that never leaves".
    let deadline = Instant::now() + Duration::from_secs(30);
    let outcomes = (0..OPENERS)
        .map(|_| receiver.recv_timeout(deadline.saturating_duration_since(Instant::now())))
        .collect::<Vec<_>>();
    // Release the writer on every path so a failing assertion cannot hang the
    // join, and close it before the directory is removed.
    writer.execute_batch("ROLLBACK").unwrap();
    drop(writer);
    for thread in threads {
        thread.join().unwrap();
    }
    for outcome in outcomes {
        let (result, elapsed) = outcome.expect("an open waited on the retained writer");
        assert_eq!(result, Ok(true), "open after {elapsed:?}");
    }
}
