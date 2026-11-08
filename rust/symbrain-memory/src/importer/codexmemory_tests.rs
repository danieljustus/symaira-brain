//! Synthetic-tree tests for the Codex memory importer (#761).

use super::*;
use crate::importer::markdown::{parse_applications, parse_markdown, section_body};
use crate::importer::test_support::{TestDir, touch};
use chrono::TimeZone;

fn at(day: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, 12, 0, 0).unwrap()
}

fn tree() -> TestDir {
    let dir = TestDir::new("codexmem");
    let files = [
        (
            "memories/MEMORY.md",
            "---\ntitle: \"Main\"\ndescription: d\napplications:\n  - com.apple.Notes\n  - \"com.x.Editor\"\n---\n# Memory\nBody text\n\n## Citations\nsrc-1\nsrc-2\n\n## Other\nz\n",
        ),
        ("memories/instructions.md", "ignored"),
        ("memories/raw_memories.md", "   \n"),
        ("memories/rollout_summaries/r1.md", "rollout summary"),
        ("memories/rollout_summaries/deep/r2.md", "too deep"),
        (
            "memories/extensions/skysight/resources/2026-10-05T06-00-00-abc-6h-summary.md",
            "six hour",
        ),
        (
            "memories/extensions/skysight/resources/2026-10-05T07-10-00-abc-10min-note.md",
            "covered ten minutes",
        ),
        (
            "memories/extensions/skysight/resources/2026-10-06T07-10-00-abc-10min-note.md",
            "lonely ten minutes",
        ),
        (
            "memories/extensions/skysight/other/2026-10-06T07-10-00-abc-10min-x.md",
            "wrong dir",
        ),
    ];
    for (rel, body) in files {
        touch(&dir.write(rel, body), at(20));
    }
    dir
}

fn importer(dir: &TestDir, policy: &ApplicationPolicy) -> CodexMemory {
    CodexMemory::new(&dir.path().join("memories").to_string_lossy(), policy)
}

#[test]
fn discovery_selects_the_curated_tier_only() {
    let dir = tree();
    let importer = importer(&dir, &ApplicationPolicy::default());
    let sessions = importer.discover_sessions(at(1)).unwrap();
    let ids: Vec<_> = sessions
        .iter()
        .map(|s| s.session_id.trim_start_matches("codex-memory:"))
        .collect();
    assert_eq!(
        ids,
        [
            "MEMORY.md",
            "extensions/skysight/resources/2026-10-05T06-00-00-abc-6h-summary.md",
            "extensions/skysight/resources/2026-10-06T07-10-00-abc-10min-note.md",
            "raw_memories.md",
            "rollout_summaries/r1.md",
        ],
        "10min covered by 6h and unrelated files are left out"
    );
    let six = &sessions[1].metadata;
    assert_eq!(
        (
            six["granularity"].as_str(),
            six["promotable"].as_str(),
            six["extension"].as_str()
        ),
        ("6h", "candidate", "skysight")
    );
    assert_eq!(six["activity_ends_at"], "2026-10-05T12:00:00Z");
    let ten = &sessions[2].metadata;
    assert_eq!(
        (ten["granularity"].as_str(), ten["promotable"].as_str()),
        ("10min", "false")
    );
    assert_eq!(ten["activity_ends_at"], "2026-10-06T07:20:00Z");
    assert_eq!(sessions[0].metadata["granularity"], "consolidated");
    assert!(
        importer.discover_sessions(at(25)).unwrap().is_empty(),
        "since filter"
    );
    assert!(
        CodexMemory::new(
            &dir.path().join("missing").to_string_lossy(),
            &ApplicationPolicy::default()
        )
        .discover_sessions(at(1))
        .unwrap()
        .is_empty()
    );
}

#[test]
fn import_parses_frontmatter_citations_and_evidence() {
    let dir = tree();
    let importer = importer(&dir, &ApplicationPolicy::default());
    let sessions = importer.discover_sessions(at(1)).unwrap();
    let facts = importer.import_session(&sessions[0]).unwrap();
    assert_eq!(facts.len(), 1);
    let fact = &facts[0];
    assert!(
        fact.content.starts_with("# Memory\nBody text") && fact.content.ends_with("## Other\nz")
    );
    assert_eq!(fact.metadata["title"], "Main");
    assert_eq!(
        fact.metadata["applications"],
        "com.apple.Notes,com.x.Editor"
    );
    assert_eq!(fact.metadata["citations"], "src-1\nsrc-2");
    assert_eq!(fact.metadata["sync_exclude"], "true");
    assert_eq!(fact.timestamp, at(20));
    assert_eq!(
        fact.evidence[0].span.end,
        i64::try_from(fact.content.len()).unwrap()
    );
    assert_eq!(fact.evidence[0].alignment_status, "exact");
    assert_eq!(fact.evidence[0].source.kind, "codex-memory");
    // Empty body and instructions.md yield nothing.
    let raw = sessions
        .iter()
        .find(|s| s.session_id.ends_with("raw_memories.md"))
        .unwrap();
    assert!(importer.import_session(raw).unwrap().is_empty());
    let instructions = SessionRef {
        path: dir
            .path()
            .join("memories/instructions.md")
            .to_string_lossy()
            .into_owned(),
        ..SessionRef::default()
    };
    assert!(importer.import_session(&instructions).unwrap().is_empty());
    // A path outside the root is refused.
    let outside = dir.write("elsewhere.md", "outside body");
    let outside = SessionRef {
        path: outside.to_string_lossy().into_owned(),
        ..SessionRef::default()
    };
    assert!(importer.import_session(&outside).unwrap().is_empty());
}

#[test]
fn application_policy_and_cursor_contract() {
    let dir = tree();
    let deny = importer(
        &dir,
        &ApplicationPolicy {
            denied: vec!["com.x.Editor".into()],
            ..ApplicationPolicy::default()
        },
    );
    assert!(
        !deny
            .discover_sessions(at(1))
            .unwrap()
            .iter()
            .any(|s| s.session_id.ends_with("MEMORY.md"))
    );
    let allow = importer(
        &dir,
        &ApplicationPolicy {
            allowed: vec!["com.apple.Notes".into()],
            ..ApplicationPolicy::default()
        },
    );
    let ids: Vec<_> = allow
        .discover_sessions(at(1))
        .unwrap()
        .into_iter()
        .map(|s| s.session_id)
        .collect();
    assert_eq!(
        ids,
        ["codex-memory:MEMORY.md"],
        "only documents naming an allowed application"
    );
    let both = importer(
        &dir,
        &ApplicationPolicy {
            allowed: vec!["com.apple.Notes".into()],
            denied: vec!["com.apple.Notes".into()],
        },
    );
    assert!(
        both.discover_sessions(at(1)).unwrap().is_empty(),
        "deny wins"
    );

    let plain = importer(&dir, &ApplicationPolicy::default());
    assert_eq!(plain.last_import_time().unwrap(), None);
    let session = |day| SessionRef {
        modified_at: at(day),
        ..SessionRef::default()
    };
    plain.mark_imported(&session(5)).unwrap();
    plain.mark_imported(&session(3)).unwrap();
    assert_eq!(plain.last_import_time().unwrap(), Some(at(5)));
    assert_eq!(
        (
            plain.category(),
            plain.privacy_level(),
            plain.is_transcript(),
            plain.stage_imported_facts(),
            plain.content_is_untrusted()
        ),
        (
            Some("activity"),
            Some(PrivacyLevel::Confidential),
            true,
            true,
            true
        )
    );
}

#[test]
fn markdown_helpers() {
    let (front, body) = parse_markdown("---\r\nk: 'v'\r\nbad line\r\nlist: [a, b]\r\n---\r\nrest");
    assert_eq!(
        (front["k"].as_str(), front["list"].as_str(), body.as_str()),
        ("v", "[a, b]", "rest")
    );
    let (front, body) = parse_markdown("no frontmatter\n---\nx");
    assert!(front.is_empty() && body == "no frontmatter\n---\nx");
    assert_eq!(parse_applications("[\"b\", \"a\", \"a\"]"), ["a", "b"]);
    assert_eq!(parse_applications("[x, y]"), ["x", "y"]);
    assert_eq!(parse_applications(" "), Vec::<String>::new());
    assert_eq!(
        section_body("## Citations\nA\n## Next\nB", "citations"),
        "A"
    );
    assert_eq!(section_body("nothing", "Citations"), "");
}
