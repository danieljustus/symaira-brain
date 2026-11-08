//! Mem0, OpenMemory, smart import and the session adapter on synthetic
//! exports (#761). No real user data, no network.

use super::adapter::SessionAdapter;
use super::mem0::Mem0Importer;
use super::openmemory::OpenMemoryImporter;
use super::smart::SmartImporter;
use super::{ExportRef, MemoryToolImporter, MetaValue, ToolFact, go_float, zero_time};
use crate::Store;
use crate::importer::test_support::{TestDir, touch};
use crate::importer::{PrivacyLevel, SessionImporter};
use chrono::{DateTime, TimeZone, Utc};
use std::collections::BTreeMap;

fn fixed_now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap()
}

fn export_ref(tool: &str, path: &std::path::Path) -> ExportRef {
    ExportRef {
        tool: tool.into(),
        path: path.to_string_lossy().into_owned(),
        format: "json".into(),
        modified_at: fixed_now(),
    }
}

#[test]
fn mem0_maps_fields_and_tolerates_bad_timestamps() {
    let dir = TestDir::new("mem0");
    let path = dir.write(
        "export.json",
        r#"{"memories":[
          {"id":"m1","content":"likes tea","user_id":"u1","agent_id":"a1","created_at":"2024-03-04T05:06:07Z"},
          {"id":"m2","content":"no user","created_at":"not a time"},
          {"id":null,"content":null}]}"#,
    );
    let facts = Mem0Importer::new()
        .import_export(&export_ref("mem0", &path))
        .unwrap();
    assert_eq!(facts.len(), 3);
    assert_eq!(facts[0].content, "likes tea");
    assert_eq!(
        facts[0].timestamp,
        Utc.with_ymd_and_hms(2024, 3, 4, 5, 6, 7).unwrap()
    );
    let keys: Vec<_> = facts[0].metadata.keys().map(String::as_str).collect();
    assert_eq!(keys, ["agent_id", "mem0_id", "prompt_mode", "user_id"]);
    assert_eq!(facts[1].timestamp, zero_time());
    let keys: Vec<_> = facts[1].metadata.keys().map(String::as_str).collect();
    assert_eq!(keys, ["mem0_id", "prompt_mode"]);
    assert_eq!(facts[1].metadata["prompt_mode"].sprint(), "code");
    assert_eq!(facts[2].content, "");

    let bad = dir.write("bad.json", "[1,2");
    let err = Mem0Importer::new()
        .import_export(&export_ref("mem0", &bad))
        .unwrap_err();
    assert!(err.contains("failed to parse JSON"), "{err}");
}

#[test]
fn openmemory_emits_memories_then_triples() {
    let dir = TestDir::new("openmemory");
    let path = dir.write(
        "o.json",
        r#"{"memories":[{"id":"x1","content":"remember","created_at":"2025-01-02T03:04:05Z","metadata":{"k":"v"}}],
            "facts":[{"subject":"Ada","predicate":"wrote","object":"notes","confidence":0.9},
                     {"subject":"A","predicate":"b","object":"c","confidence":1}]}"#,
    );
    let importer = OpenMemoryImporter::with_home(dir.path().to_path_buf(), fixed_now);
    let facts = importer
        .import_export(&export_ref("openmemory", &path))
        .unwrap();
    assert_eq!(facts.len(), 3);
    assert_eq!(facts[0].metadata["openmemory_id"].sprint(), "x1");
    assert_eq!(facts[1].content, "Ada wrote notes");
    assert_eq!(facts[1].timestamp, fixed_now());
    assert_eq!(facts[1].metadata["confidence"].sprint(), "0.9");
    assert_eq!(facts[2].metadata["confidence"].sprint(), "1");
}

#[test]
fn go_float_matches_percent_v() {
    for (value, want) in [
        (0.0, "0"),
        (1.0, "1"),
        (0.9, "0.9"),
        (-2.5, "-2.5"),
        (1e21, "1e+21"),
        (1.5e-7, "1.5e-07"),
        (123_456_789.0, "123456789"),
    ] {
        assert_eq!(go_float(value), want);
    }
}

#[test]
fn discovery_walks_json_files_and_honors_default_roots() {
    let home = TestDir::new("tool-home");
    let a = home.write(".mem0/a.json", "{}");
    home.write(".mem0/deep/b.json", "{}");
    home.write(".mem0/readme.md", "x");
    home.write(".openmemory/o.json", "{}");
    let mem0 = Mem0Importer::with_home(home.path().to_path_buf());
    assert_eq!(mem0.discover_exports("").unwrap().len(), 2);
    let one = mem0.discover_exports(&a.to_string_lossy()).unwrap();
    assert_eq!(
        (one.len(), one[0].tool.as_str(), one[0].format.as_str()),
        (1, "mem0", "json")
    );
    let md = home.path().join(".mem0/readme.md");
    assert!(
        mem0.discover_exports(&md.to_string_lossy())
            .unwrap()
            .is_empty()
    );
    assert!(mem0.discover_exports("/nonexistent/path").is_err());
    let open = OpenMemoryImporter::with_home(home.path().to_path_buf(), fixed_now);
    let refs = open.discover_exports("").unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].tool, "openmemory");
    let empty = TestDir::new("tool-empty-home");
    assert!(
        Mem0Importer::with_home(empty.path().to_path_buf())
            .discover_exports("")
            .is_err()
    );
}

fn tool_fact(content: &str) -> ToolFact {
    ToolFact {
        content: content.into(),
        source: "chatgpt".into(),
        timestamp: Utc.with_ymd_and_hms(2025, 5, 5, 5, 5, 5).unwrap(),
        metadata: BTreeMap::from([
            ("tool".to_owned(), MetaValue::Str("chatgpt".into())),
            ("confidence".to_owned(), MetaValue::Num(0.5)),
        ]),
    }
}

fn rows(store: &Store) -> Vec<(String, String, String, String)> {
    let conn = store.lock().unwrap();
    let mut s = conn
        .prepare(
            "SELECT content, scope, metadata, consolidation_status FROM memories ORDER BY content",
        )
        .unwrap();
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn smart_import_redacts_dedups_and_honors_dry_run() {
    let store = Store::open_in_memory().unwrap();
    let smart = SmartImporter::new(&store).with_clock(fixed_now);
    let facts = vec![
        tool_fact("User email: carol@example.com"),
        tool_fact("plain fact"),
    ];

    let dry = smart.import_facts(&facts, true).unwrap();
    assert_eq!((dry.created, dry.skipped, dry.merged), (2, 0, 0));
    assert!(rows(&store).is_empty());

    let first = smart.import_facts(&facts, false).unwrap();
    assert_eq!((first.created, first.skipped), (2, 0));
    let stored = rows(&store);
    assert_eq!(stored.len(), 2);
    assert!(
        stored.iter().all(|r| !r.0.contains("carol@example.com")),
        "{stored:?}"
    );
    assert!(stored.iter().all(|r| r.1 == "agent" && r.3 == "raw"));
    assert!(
        stored[0].2.contains("\"confidence\":\"0.5\""),
        "{}",
        stored[0].2
    );

    // Dedup is by the hash of the original text, so a re-import skips both.
    let again = smart.import_facts(&facts, false).unwrap();
    assert_eq!((again.created, again.skipped), (0, 2));
    assert_eq!(rows(&store).len(), 2);
}

#[test]
fn adapter_exposes_exports_as_confidential_sessions() {
    let dir = TestDir::new("adapter");
    let old = dir.write("old.json", r#"{"memories":[]}"#);
    let new = dir.write(
        "new.json",
        r#"{"memories":[{"id":"1","content":"c","created_at":"2025-01-01T00:00:00Z","user_id":"u"}]}"#,
    );
    touch(&old, Utc.with_ymd_and_hms(2020, 1, 1, 0, 0, 0).unwrap());
    touch(&new, Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap());
    let adapter = SessionAdapter::new(Box::new(Mem0Importer::new()), &dir.path().to_string_lossy());
    assert_eq!(adapter.name(), "mem0");
    assert_eq!(adapter.category(), Some("memory-tool"));
    assert_eq!(adapter.privacy_level(), Some(PrivacyLevel::Confidential));
    assert!(adapter.requires_pii_guard());

    let since = Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap();
    let sessions = adapter.discover_sessions(since).unwrap();
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, sessions[0].path);
    assert!(sessions[0].path.ends_with("new.json"));

    let facts = adapter.import_session(&sessions[0]).unwrap();
    assert_eq!(facts.len(), 1);
    assert_eq!(facts[0].session_id, sessions[0].session_id);
    assert_eq!(facts[0].metadata["mem0_id"], "1");
    assert_eq!(facts[0].metadata["user_id"], "u");
    assert!(facts[0].evidence.is_empty());
}
