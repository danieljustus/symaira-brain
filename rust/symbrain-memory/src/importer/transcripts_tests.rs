//! Synthetic-fixture tests for the transcript importers (#761).

use super::*;
use crate::importer::test_support::{TestDir, touch};
use chrono::TimeZone;

fn at(day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 4, day, 9, 0, 0).unwrap()
}

const LONG: &str = "This assistant answer is intentionally longer than fifty bytes in total.";

#[test]
fn claude_code_discovers_and_extracts() {
    let dir = TestDir::new("claude");
    let new = dir.write("proj-a/abc.jsonl", &format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"hi\"}},\"project\":\"proj-a\"}}\n\
         not json\n\n\
         {{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":\"{LONG}\"}},\"timestamp\":\"2026-04-10T10:00:00Z\",\"model\":\"m1\"}}\n\
         {{\"message\":{{\"role\":\"assistant\",\"content\":\"short\"}}}}\n\
         {{\"message\":{{\"role\":\"assistant\",\"content\":\"{LONG} (by role only)\"}},\"timestamp\":\"bad\"}}\n\
         {{\"type\":\"assistant\",\"message\":{{\"content\":[1,2]}}}}\n"
    ));
    let old = dir.write("proj-b/old.jsonl", "{}\n");
    dir.write("proj-b/ignored.txt", "x");
    touch(&new, at(12));
    touch(&old, at(1));
    let importer = ClaudeCode::new(&dir.path().to_string_lossy());
    let sessions = importer.discover_sessions(at(5)).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "abc");
    assert_eq!(sessions[0].metadata["project_dir"], "proj-a");
    assert_eq!(sessions[0].modified_at, at(12));
    let facts = importer.import_session(&sessions[0]).unwrap();
    assert_eq!(
        facts.len(),
        2,
        "short, malformed and array-content lines are skipped"
    );
    assert_eq!(facts[0].content, LONG);
    assert_eq!(
        facts[0].timestamp,
        Utc.with_ymd_and_hms(2026, 4, 10, 10, 0, 0).unwrap()
    );
    assert_eq!(
        facts[0].metadata["project"], "proj-a",
        "project carried from an earlier entry"
    );
    assert_eq!(facts[0].metadata["model"], "m1");
    assert_eq!(
        facts[1].timestamp,
        crate::sync_state::zero_time(),
        "bad timestamp -> zero time"
    );
    assert!(importer.is_transcript() && importer.category().is_none());
    assert!(
        importer
            .import_session(&SessionRef {
                path: "/nonexistent/x".into(),
                ..SessionRef::default()
            })
            .unwrap_err()
            .starts_with("failed to read session file: ")
    );
}

#[test]
fn claude_code_overlong_line_stops_with_the_scanner_error() {
    let dir = TestDir::new("claude-long");
    let huge = "x".repeat(1024 * 1024);
    let path = dir.write(
        "p/s.jsonl",
        &format!("{{\"type\":\"assistant\",\"message\":{{\"content\":\"{LONG}\"}}}}\n{huge}\n"),
    );
    let session = SessionRef {
        path: path.to_string_lossy().into_owned(),
        ..SessionRef::default()
    };
    let error = ClaudeCode::new("").import_session(&session).unwrap_err();
    assert_eq!(error, "bufio.Scanner: token too long");
}

#[test]
fn codex_only_takes_rollout_files_from_archived_sessions() {
    let dir = TestDir::new("codex");
    let rollout = dir.write(
        "archived_sessions/rollout-2026-04-10.jsonl",
        &format!(
            "{{\"timestamp\":\"2026-04-10T10:00:00+02:00\",\"type\":\"message\",\"payload\":{{\"role\":\"assistant\",\"content\":\"{LONG}\"}}}}\n\
             {{\"payload\":{{\"role\":\"user\",\"content\":\"{LONG}\"}}}}\n{{\"payload\":null}}\nbroken\n"
        ),
    );
    let index = dir.write("archived_sessions/session_index.jsonl", "{}");
    let other = dir.write("archived_sessions/notes.jsonl", "{}");
    let elsewhere = dir.write("sessions/rollout-live.jsonl", "{}");
    for path in [&rollout, &index, &other, &elsewhere] {
        touch(path, at(15));
    }
    let importer = Codex::new(&dir.path().to_string_lossy());
    let sessions = importer.discover_sessions(at(1)).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "rollout-2026-04-10");
    assert!(
        importer.discover_sessions(at(20)).unwrap().is_empty(),
        "since filter"
    );
    let facts = importer.import_session(&sessions[0]).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(
        facts[0].timestamp,
        Utc.with_ymd_and_hms(2026, 4, 10, 8, 0, 0).unwrap()
    );
    assert_eq!(facts[0].source, "codex");
}

#[test]
fn aider_splits_turns_by_markers() {
    let dir = TestDir::new("aider");
    let history = dir.write(
        "work/repo/.aider.chat.history.md",
        &format!(
            "# aider chat started\n\n**Human**: add a flag\nplease\n**Assistant**:\n{LONG}\nsecond line\n\n**Human**: thanks\n**Assistant**:\ntiny\n## notes\n**Assistant**: ignored same-line text\n{LONG} final\n"
        ),
    );
    dir.write("work/repo/other.md", "x");
    touch(&history, at(11));
    let importer = Aider::new(vec![dir.path().join("work").to_string_lossy().into_owned()]);
    let sessions = importer.discover_sessions(at(1)).unwrap();
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].session_id.ends_with("work/repo"));
    assert_eq!(sessions[0].metadata["project"], sessions[0].session_id);
    let facts = importer.import_session(&sessions[0]).unwrap();
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].content, format!("{LONG}\nsecond line\n\n"));
    assert_eq!(facts[1].content, format!("{LONG} final\n"));
    assert_eq!(facts[0].timestamp, at(11));
    assert!(!importer.is_transcript());
    assert!(
        Aider::new(vec![
            dir.path().join("missing").to_string_lossy().into_owned()
        ])
        .discover_sessions(at(1))
        .unwrap()
        .is_empty()
    );
}
