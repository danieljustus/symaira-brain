//! Behavior tests for the DB-backed activity store (#761) with an injected
//! clock, redaction, rollup idempotence, retention and episode staging.

use crate::Store;
use crate::activity_episode::ActivityEpisode;
use crate::activity_store::{
    ActivityOptions, ActivitySegment, GRANULARITY_6H, GRANULARITY_10MIN, segment_id,
};
use chrono::{DateTime, Duration, TimeZone, Utc};

fn base() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap()
}

fn fine(source: &str, index: i64, summary: &str) -> ActivitySegment {
    let start = base() + Duration::minutes(10 * index);
    ActivitySegment {
        id: String::new(),
        source: source.to_owned(),
        granularity: GRANULARITY_10MIN.to_owned(),
        started_at: start,
        ended_at: start + Duration::minutes(10),
        applications: vec!["Zed".into(), "Terminal".into(), "Zed".into()],
        redacted_summary: summary.to_owned(),
        raw_ref: "raw:1".to_owned(),
        prior_segment_ids: Vec::new(),
        superseded_by: String::new(),
        expires_at: None,
    }
}

fn episode(ids: &[String], confidence: f64) -> ActivityEpisode {
    ActivityEpisode {
        id: String::new(),
        title: "Fixing the sync bug".to_owned(),
        scope: "project:brain".to_owned(),
        started_at: base(),
        ended_at: base() + Duration::hours(1),
        confidence,
        segment_ids: ids.to_vec(),
        citations: vec!["doc:1".into()],
        expires_at: None,
    }
}

fn now() -> DateTime<Utc> {
    base() + Duration::hours(2)
}

#[test]
fn save_redacts_defaults_ids_and_expiry_and_upserts() {
    let store = Store::open_in_memory().unwrap();
    let options = ActivityOptions::default();
    let saved = store
        .save_activity_segment(
            fine(" laptop ", 0, "opened https://user:hunter2@example.com/x"),
            options,
        )
        .unwrap();
    assert_eq!(saved.source, "laptop");
    assert_eq!(
        saved.id,
        segment_id(
            "laptop",
            GRANULARITY_10MIN,
            saved.started_at,
            saved.ended_at
        )
    );
    assert!(saved.id.starts_with("activity-") && saved.id.len() == "activity-".len() + 32);
    assert!(
        !saved.redacted_summary.contains("hunter2"),
        "{}",
        saved.redacted_summary
    );
    assert_eq!(saved.expires_at, Some(saved.ended_at + Duration::hours(48)));
    let again = store
        .save_activity_segment(fine("laptop", 0, "second text"), options)
        .unwrap();
    assert_eq!(again.id, saved.id, "same window upserts");
    let stored = store.activity_segment(&saved.id).unwrap().unwrap();
    assert_eq!(stored.redacted_summary, "second text");
    assert_eq!(stored.applications, ["Zed", "Terminal", "Zed"]);
    assert_eq!(stored.prior_segment_ids, Vec::<String>::new());
}

#[test]
fn validation_rejects_bad_segments_and_episodes() {
    let store = Store::open_in_memory().unwrap();
    let options = ActivityOptions::default();
    let mut bad = fine("laptop", 0, "x");
    bad.granularity = "1h".to_owned();
    assert!(store.save_activity_segment(bad, options).is_err());
    let mut empty = fine("laptop", 0, "");
    empty.redacted_summary.clear();
    assert!(store.save_activity_segment(empty, options).is_err());
    let mut backwards = fine("laptop", 0, "x");
    backwards.ended_at = backwards.started_at;
    assert!(store.save_activity_segment(backwards, options).is_err());
    assert!(
        store
            .save_activity_segment(fine("  ", 0, "x"), options)
            .is_err()
    );
    assert!(
        store
            .save_activity_episode(episode(&[], 1.5), options)
            .is_err()
    );
    let mut untitled = episode(&[], 0.5);
    untitled.title.clear();
    assert!(store.save_activity_episode(untitled, options).is_err());
}

#[test]
fn listing_hides_expired_and_superseded_rows() {
    let store = Store::open_in_memory().unwrap();
    let options = ActivityOptions::default();
    for index in 0..3 {
        store
            .save_activity_segment(fine("laptop", index, &format!("work {index}")), options)
            .unwrap();
    }
    let (start, end) = (base(), base() + Duration::hours(1));
    assert_eq!(
        store
            .list_activity_segments(start, end, now(), false)
            .unwrap()
            .len(),
        3
    );
    // Past the 48h TTL everything is hidden.
    let later = base() + Duration::days(5);
    assert!(
        store
            .list_activity_segments(start, end, later, false)
            .unwrap()
            .is_empty()
    );
    let rollup = store
        .rollup_activity_6h("laptop", base(), options, now())
        .unwrap();
    let visible = store
        .list_activity_segments(start, end, now(), false)
        .unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, rollup.id);
    assert_eq!(
        store
            .list_activity_segments(start, end, now(), true)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn rollup_is_deterministic_idempotent_and_records_provenance() {
    let store = Store::open_in_memory().unwrap();
    let options = ActivityOptions::default();
    for index in 0..3 {
        store
            .save_activity_segment(fine("laptop", index, &format!("work {index}")), options)
            .unwrap();
    }
    assert!(
        store
            .rollup_activity_6h("laptop", base() + Duration::days(1), options, now())
            .is_err()
    );
    let first = store
        .rollup_activity_6h("laptop", base(), options, now())
        .unwrap();
    assert_eq!(first.granularity, GRANULARITY_6H);
    assert_eq!(first.redacted_summary, "work 0\nwork 1\nwork 2");
    assert_eq!(first.applications, ["Terminal", "Zed"]);
    assert_eq!(first.prior_segment_ids.len(), 3);
    assert_eq!(first.raw_ref, "rollup:laptop:2026-11-01T00:00:00Z");
    assert_eq!(
        first.expires_at,
        Some(base() + Duration::hours(6) + Duration::hours(48))
    );
    assert_eq!(
        first.id,
        segment_id(
            "laptop",
            GRANULARITY_6H,
            base(),
            base() + Duration::hours(6)
        )
    );
    let second = store
        .rollup_activity_6h("laptop", base(), options, now())
        .unwrap();
    assert_eq!(second, first, "re-running replaces the same row");
    for id in &first.prior_segment_ids {
        let fine = store.activity_segment(id).unwrap().unwrap();
        assert_eq!(fine.superseded_by, first.id);
    }
}

#[test]
fn episodes_round_trip_and_retention_clears_provenance() {
    let store = Store::open_in_memory().unwrap();
    let options = ActivityOptions::default();
    let segment = store
        .save_activity_segment(fine("laptop", 0, "work"), options)
        .unwrap();
    let saved = store
        .save_activity_episode(
            episode(&[segment.id.clone(), segment.id.clone()], 0.8),
            options,
        )
        .unwrap();
    assert_eq!(saved.segment_ids, std::slice::from_ref(&segment.id));
    assert_eq!(
        saved.expires_at,
        Some(saved.ended_at + Duration::hours(720))
    );
    assert_eq!(store.activity_episode(&saved.id).unwrap().unwrap(), saved);
    assert_eq!(
        store
            .list_activity_episodes(base(), base() + Duration::hours(2), now())
            .unwrap()
            .len(),
        1
    );

    // Expiry removes only what is due, and reports it.
    let early = store.expire_activity(base() + Duration::hours(10)).unwrap();
    assert_eq!((early.segments, early.episodes), (0, 0));
    let due = store.expire_activity(base() + Duration::days(3)).unwrap();
    assert_eq!((due.segments, due.episodes), (1, 0));
    let late = store.expire_activity(base() + Duration::days(60)).unwrap();
    assert_eq!((late.segments, late.episodes), (0, 1));
    assert_eq!(
        store.delete_activity_episode("missing").unwrap(),
        crate::RetentionResult::default()
    );
}

#[test]
fn delete_clear_and_last_session_remove_data() {
    let store = Store::open_in_memory().unwrap();
    let options = ActivityOptions::default();
    let a = store
        .save_activity_segment(fine("laptop", 0, "a"), options)
        .unwrap();
    let b = store
        .save_activity_segment(fine("laptop", 1, "b"), options)
        .unwrap();
    let mut first = episode(std::slice::from_ref(&a.id), 0.5);
    first.title = "first".into();
    first.ended_at = base() + Duration::hours(1);
    let mut second = episode(std::slice::from_ref(&b.id), 0.5);
    second.title = "second".into();
    second.started_at = base() + Duration::hours(3);
    second.ended_at = base() + Duration::hours(4);
    let first = store.save_activity_episode(first, options).unwrap();
    let second = store.save_activity_episode(second, options).unwrap();

    let last = store.clear_last_activity_session().unwrap();
    assert_eq!((last.segments, last.episodes), (1, 1));
    assert!(store.activity_episode(&second.id).unwrap().is_none());
    assert!(store.activity_segment(&b.id).unwrap().is_none());
    assert!(store.activity_episode(&first.id).unwrap().is_some());

    assert!(store.clear_activity_range(base(), base()).is_err());
    let ranged = store
        .clear_activity_range(base(), base() + Duration::hours(2))
        .unwrap();
    assert_eq!((ranged.segments, ranged.episodes), (1, 1));
    store
        .save_activity_segment(fine("laptop", 0, "again"), options)
        .unwrap();
    let all = store.clear_all_activity().unwrap();
    assert_eq!((all.segments, all.episodes), (1, 0));
    assert_eq!(
        store.clear_last_activity_session().unwrap(),
        crate::RetentionResult::default()
    );
}

#[test]
fn on_disk_deletion_leaves_no_wal_residue() {
    let directory = std::env::temp_dir().join(format!(
        "symbrain-activity-{}-{}",
        std::process::id(),
        base().timestamp()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("memory.db");
    let store = Store::open(&path).unwrap();
    let options = ActivityOptions::default();
    store
        .save_activity_segment(fine("laptop", 0, "secret-ish work"), options)
        .unwrap();
    store.clear_all_activity().unwrap();
    let wal = std::fs::metadata(format!("{}-wal", path.display())).map_or(0, |meta| meta.len());
    assert_eq!(wal, 0);
    drop(store);
    let bytes = std::fs::read(&path).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("secret-ish work"));
    std::fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn staging_creates_a_synced_out_unapproved_candidate_with_evidence() {
    let store = Store::open_in_memory().unwrap();
    let options = ActivityOptions::default();
    let a = store
        .save_activity_segment(fine("laptop", 0, "wrote tests"), options)
        .unwrap();
    let b = store
        .save_activity_segment(fine("laptop", 1, "fixed bug"), options)
        .unwrap();
    let saved = store
        .save_activity_episode(episode(&[a.id.clone(), b.id.clone()], 0.0), options)
        .unwrap();
    let memory_id = store.stage_activity_episode(&saved.id, now()).unwrap();
    let again = store.stage_activity_episode(&saved.id, now()).unwrap();
    assert_eq!(memory_id, again);
    let conn = store.lock().unwrap();
    let (content, scope, review, kind, importance, metadata): (String, String, String, String, f64, String) = conn
        .query_row(
            "SELECT content, scope, review_status, kind, importance, metadata FROM memories WHERE id = ?",
            [&memory_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?)),
        )
        .unwrap();
    assert_eq!(
        (
            content.as_str(),
            scope.as_str(),
            review.as_str(),
            kind.as_str()
        ),
        (
            "Fixing the sync bug",
            "project:brain",
            "staged",
            "reference"
        )
    );
    assert!(
        (importance - 0.5).abs() < f64::EPSILON,
        "zero confidence defaults to 0.5"
    );
    assert!(metadata.contains("\"sync_exclude\":\"true\""));
    let evidence: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM memory_evidence WHERE memory_id = ?",
            [&memory_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(evidence, 2, "restaging replaces, never stacks, evidence");
    let oplog: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sync_oplog WHERE memory_id = ?",
            [&memory_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        oplog, 0,
        "sync_exclude keeps the candidate out of the oplog"
    );
    drop(conn);

    // A lone fine segment and missing parts are refused.
    let lone = store
        .save_activity_episode(
            ActivityEpisode {
                title: "lone".into(),
                segment_ids: vec![a.id.clone()],
                ..episode(&[], 0.9)
            },
            options,
        )
        .unwrap();
    assert!(
        store
            .stage_activity_episode(&lone.id, now())
            .unwrap_err()
            .to_string()
            .contains("single 10min")
    );
    assert!(store.stage_activity_episode("nope", now()).is_err());
}
