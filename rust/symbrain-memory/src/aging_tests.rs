//! Behavior tests for aging and working-memory eviction with an injected
//! clock and in-memory stores (#760).

use crate::Store;
use crate::aging::{AgingConfig, decay_factor};
use chrono::{DateTime, Duration, TimeZone, Utc};
use rusqlite::params;

fn at(day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap() + Duration::days(i64::from(day))
}

fn insert(
    store: &Store,
    id: &str,
    created: DateTime<Utc>,
    last: Option<DateTime<Utc>>,
    count: i64,
) {
    store
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at,last_access,access_count) VALUES(?,?, 'global','{}','[]',?,?,?,?)",
            params![
                id,
                id,
                crate::gotime::format(created),
                crate::gotime::format(created),
                last.map(crate::gotime::format),
                count
            ],
        )
        .unwrap();
}

#[test]
fn decay_curve_matches_the_shipped_formula() {
    let config = AgingConfig::default();
    let created = at(0);
    let cases = [
        // (days since creation, last access days ago, access count, min, max)
        (0, None, 1, 0.999, 1.0),
        (120, None, 1, 0.49, 0.51),
        (365, None, 1, 0.1, 0.13),
        (400, None, 1, 0.0, 0.1),
        (3650, None, 1, 0.02, 0.02),
        (365, Some(1), 40, 0.75, 1.0),
    ];
    for (days, accessed, count, low, high) in cases {
        let last = accessed.map(|ago| at(days) - Duration::days(ago));
        let factor = decay_factor(&config, created, last, count, at(days));
        assert!(
            (low..=high).contains(&factor),
            "days={days} accessed={accessed:?} -> {factor}"
        );
    }
    let off = AgingConfig {
        enabled: false,
        ..AgingConfig::default()
    };
    assert!((decay_factor(&off, created, None, 1, at(4000)) - 1.0).abs() < f64::EPSILON);
    // A clock before the access never boosts above 1.
    assert!(decay_factor(&config, at(10), None, 1, at(0)) <= 1.0);
}

#[test]
fn unaccessed_memory_never_gets_the_access_boost() {
    let config = AgingConfig::default();
    let never = decay_factor(&config, at(0), None, 1000, at(240));
    let touched = decay_factor(&config, at(0), Some(at(0)), 1000, at(240));
    assert!(never < touched);
}

#[test]
fn pass_decays_retires_and_dry_run_writes_nothing() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, "fresh", at(0), None, 1);
    insert(&store, "middle", at(0), None, 1);
    insert(&store, "stale", at(0), None, 1);
    insert(&store, "loved", at(0), Some(at(360)), 30);
    let config = AgingConfig::default();

    // Day 100: everything is a little decayed, nothing is retired.
    let first = store.run_aging(&config, at(100), true).unwrap();
    // `loved` was accessed after `now`, so its clock-skewed recency stays 1.0.
    assert_eq!((first.scanned, first.decayed, first.retired), (4, 3, 0));
    assert!(first.dry_run);
    let untouched: i64 = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM memories WHERE decay_factor != 1.0 OR retired_at IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(untouched, 0, "dry run must not write");

    // Day 400: unaccessed memories fall below the floor and are flagged.
    let second = store.run_aging(&config, at(400), false).unwrap();
    assert_eq!(second.scanned, 4);
    assert_eq!(second.retired, 3, "{second}");
    assert_eq!(second.retired_ids, ["fresh", "middle", "stale"]);
    assert!(
        second
            .to_string()
            .starts_with("aging pass: scanned 4, decayed 1, retired 3")
    );
    let (retired, rows): (i64, i64) = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT SUM(retired_at IS NOT NULL), COUNT(*) FROM memories",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((retired, rows), (3, 4), "retired is a flag, rows survive");

    // Retired rows are not scanned again; unretire restores full weight.
    let third = store.run_aging(&config, at(500), false).unwrap();
    assert_eq!(third.scanned, 1);
    store.unretire_memory("stale").unwrap();
    let factor: f64 = store
        .lock()
        .unwrap()
        .query_row(
            "SELECT decay_factor FROM memories WHERE id='stale'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((factor - 1.0).abs() < f64::EPSILON);
    assert!(store.unretire_memory("missing").is_err());
}

#[test]
fn disabled_pass_is_a_no_op() {
    let store = Store::open_in_memory().unwrap();
    insert(&store, "a", at(0), None, 1);
    let config = AgingConfig {
        enabled: false,
        ..AgingConfig::default()
    };
    let report = store.run_aging(&config, at(900), false).unwrap();
    assert_eq!((report.scanned, report.decayed, report.retired), (0, 0, 0));
}

fn working(store: &Store, id: &str, expires: Option<DateTime<Utc>>, status: &str) {
    store
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at,tier,expires_at,consolidation_status) VALUES(?,?, 'global','{}','[]',?,?, 'working',?,?)",
            params![
                id,
                id,
                crate::gotime::format(at(0)),
                crate::gotime::format(at(0)),
                expires.map(crate::gotime::format),
                status
            ],
        )
        .unwrap();
}

#[test]
fn compaction_handles_none_one_and_many_expired() {
    let store = Store::open_in_memory().unwrap();
    working(&store, "live", Some(at(50)), "raw");
    working(&store, "open-ended", None, "raw");
    let none = store
        .compact_working_memories(at(10), false, |_| panic!("nothing to consolidate"))
        .unwrap();
    assert_eq!((none.expired_count, none.evicted_count), (0, 0));

    working(&store, "one", Some(at(5)), "raw");
    let dry = store
        .compact_working_memories(at(10), true, |_| {
            panic!("single memory is not consolidated")
        })
        .unwrap();
    assert_eq!((dry.expired_count, dry.evicted_count), (1, 0));
    let single = store
        .compact_working_memories(at(10), false, |_| {
            panic!("single memory is not consolidated")
        })
        .unwrap();
    assert_eq!((single.expired_count, single.evicted_count), (1, 1));

    working(&store, "a", Some(at(2)), "raw");
    working(&store, "b", Some(at(3)), "raw");
    working(&store, "archived", Some(at(1)), "archived");
    let mut seen = Vec::new();
    let many = store
        .compact_working_memories(at(10), false, |ids| {
            seen = ids.to_vec();
            Ok(())
        })
        .unwrap();
    assert_eq!(seen, ["a", "b"], "oldest expiry first, archived skipped");
    assert_eq!((many.expired_count, many.evicted_count), (2, 3));
    // `live` and `open-ended` survive; the eviction delete also takes the
    // archived expired row, exactly like the shipped DELETE.
    let left: i64 = store
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM memories", [], |row| row.get(0))
        .unwrap();
    assert_eq!(left, 2);
}

#[test]
fn consolidation_failure_prevents_eviction() {
    let store = Store::open_in_memory().unwrap();
    working(&store, "a", Some(at(2)), "raw");
    working(&store, "b", Some(at(3)), "raw");
    let error = store.compact_working_memories(at(10), false, |_| {
        Err(crate::StoreError::Invalid("llm unavailable".into()))
    });
    assert!(error.is_err());
    assert_eq!(store.expired_working_memories(at(10)).unwrap().len(), 2);
}
