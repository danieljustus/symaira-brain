use chrono::{DateTime, Utc};
use rusqlite::params;

use crate::{Store, gotime};

fn time(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .unwrap()
        .with_timezone(&Utc)
}

fn seed(store: &Store) {
    let conn = store.activity_conn().unwrap();
    for (id, start, end, expiry) in [
        (
            "expired",
            "2026-01-01T00:00:00Z",
            "2026-01-01T00:10:00Z",
            "2026-01-02T00:00:00Z",
        ),
        (
            "active",
            "2026-01-02T00:00:00Z",
            "2026-01-02T00:10:00Z",
            "2026-01-05T00:00:00Z",
        ),
    ] {
        conn.execute("INSERT INTO activity_segments (id,source,granularity,started_at,ended_at,applications,redacted_summary,raw_ref,prior_segment_ids,superseded_by,expires_at) VALUES (?,'owned','10min',?,?,'[]','fixture','','[]','',?)",
            params![id, gotime::format(time(start)), gotime::format(time(end)), gotime::format(time(expiry))]).unwrap();
        conn.execute("INSERT INTO activity_episodes (id,title,scope,started_at,ended_at,confidence,sources,citations,expires_at) VALUES (?,'fixture','agent',?,?,0.5,'[]','[]',?)",
            params![id, gotime::format(time(start)), gotime::format(time(end)), gotime::format(time(expiry))]).unwrap();
    }
}

fn count(store: &Store, table: &str) -> i64 {
    store
        .activity_conn()
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn expiry_deletes_equal_expiry_and_preserves_future_rows() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    let result = store.activity_expire(Some(time("2026-01-02T00:00:00Z")));
    assert!(result.error.is_none());
    assert_eq!(result.deleted.segments, 1);
    assert_eq!(result.deleted.episodes, 1);
    assert_eq!(count(&store, "activity_segments"), 1);
}

#[test]
fn clear_range_uses_overlap_not_inclusive_touching_edges() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    let result =
        store.activity_clear_time_range(time("2026-01-01T00:10:00Z"), time("2026-01-02T00:00:00Z"));
    assert!(result.error.is_none());
    assert_eq!(result.deleted.segments, 0);
    assert_eq!(count(&store, "activity_segments"), 2);
    let result =
        store.activity_clear_time_range(time("2026-01-01T00:09:59Z"), time("2026-01-02T00:00:00Z"));
    assert_eq!(result.deleted.segments, 1);
    assert_eq!(result.deleted.episodes, 1);
}

#[test]
fn invalid_range_does_not_delete_any_rows() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    let result =
        store.activity_clear_time_range(time("2026-01-02T00:00:00Z"), time("2026-01-02T00:00:00Z"));
    assert!(result.error.is_some());
    assert_eq!(result.deleted.segments, 0);
    assert_eq!(count(&store, "activity_episodes"), 2);
}

#[test]
fn segment_failure_rolls_back_prior_episode_deletion() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    store.activity_conn().unwrap().execute_batch("CREATE TRIGGER refusal BEFORE DELETE ON activity_segments BEGIN SELECT RAISE(ABORT,'owned refusal'); END;").unwrap();
    let result = store.activity_clear_all();
    assert!(result.error.is_some());
    assert_eq!(result.deleted.episodes, 0);
    assert_eq!(count(&store, "activity_episodes"), 2);
    assert_eq!(count(&store, "activity_segments"), 2);
}

#[test]
fn clear_all_preserves_import_state() {
    let store = Store::open_in_memory().unwrap();
    seed(&store);
    store.import_mark_imported("owned", "session", 1).unwrap();
    let result = store.activity_clear_all();
    assert!(result.error.is_none());
    assert_eq!(result.deleted.episodes, 2);
    assert_eq!(result.deleted.segments, 2);
    assert!(store.import_is_imported("owned", "session").unwrap());
    let again = store.activity_clear_all();
    assert!(again.error.is_none());
    assert_eq!(again.deleted.segments, 0);
}

#[test]
fn duplicate_import_marker_updates_count_without_a_second_session() {
    let store = Store::open_in_memory().unwrap();
    assert!(!store.import_is_imported("owned", "session").unwrap());
    assert!(store.import_last_time("owned").unwrap().is_none());
    store.import_mark_imported("owned", "session", 3).unwrap();
    store.import_mark_imported("owned", "session", -1).unwrap();
    assert_eq!(store.import_count("owned").unwrap(), 1);
    let count: i64 = store
        .activity_conn()
        .unwrap()
        .query_row(
            "SELECT memory_count FROM import_state WHERE tool='owned'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, -1);
    assert!(
        store
            .import_last_time("owned")
            .unwrap_err()
            .to_string()
            .contains("driver.Value type string into type *time.Time")
    );
}
