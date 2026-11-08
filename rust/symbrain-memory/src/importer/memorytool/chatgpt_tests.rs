//! ChatGPT importer on synthetic exports in a temp dir (#761).

use super::chatgpt::ChatGptImporter;
use super::chatgpt_fs::MAX_EXPORT_BYTES;
use super::{ExportRef, MemoryToolImporter};
use crate::importer::test_support::TestDir;
use chrono::Utc;
use std::path::Path;

const LONG: &str = "Hello! How can I help you today? This is a longer response that exceeds the fifty character minimum.";

fn export_ref(path: &Path) -> ExportRef {
    ExportRef {
        tool: "chatgpt".into(),
        path: path.to_string_lossy().into_owned(),
        format: "json".into(),
        modified_at: Utc::now(),
    }
}

fn conversation(parts: &str, role: &str) -> String {
    format!(
        r#"[{{"title":"Test Chat","create_time":1700000000,"mapping":{{"a":{{"message":{{"author":{{"role":"user"}},"content":{{"parts":["hello"]}}}}}},"b":{{"message":{{"author":{{"role":"{role}"}},"content":{{"parts":[{parts}]}}}}}},"c":{{"message":null}}}}}}]"#
    )
}

#[test]
fn imports_long_assistant_messages_only() {
    let cases: [(&str, &str, &str, usize); 5] = [
        ("long assistant", &format!("\"{LONG}\""), "assistant", 1),
        ("short assistant", "\"Short\"", "assistant", 0),
        ("user role", &format!("\"{LONG}\""), "user", 0),
        (
            "exactly 50 bytes",
            &format!("\"{}\"", "x".repeat(50)),
            "assistant",
            0,
        ),
        (
            "parts are joined",
            "\"first half of a long\",\"second half that pushes it past fifty bytes\"",
            "assistant",
            1,
        ),
    ];
    for (name, parts, role, want) in cases {
        let dir = TestDir::new("chatgpt-import");
        let path = dir.write("conversations.json", &conversation(parts, role));
        let facts = ChatGptImporter::new()
            .import_export(&export_ref(&path))
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(facts.len(), want, "{name}");
        if let Some(fact) = facts.first() {
            assert_eq!(fact.source, "chatgpt");
            assert_eq!(fact.timestamp.timestamp(), 1_700_000_000);
            assert_eq!(fact.metadata["title"].sprint(), "Test Chat");
            assert_eq!(fact.metadata["prompt_mode"].sprint(), "code");
        }
    }
}

#[test]
fn parse_and_read_errors_are_worded_like_go() {
    let dir = TestDir::new("chatgpt-errors");
    let bad = dir.write("conversations.json", "{invalid");
    let err = ChatGptImporter::new()
        .import_export(&export_ref(&bad))
        .unwrap_err();
    assert!(err.contains("failed to parse JSON"), "{err}");
    let err = ChatGptImporter::new()
        .import_export(&export_ref(&dir.path().join("missing.json")))
        .unwrap_err();
    assert!(err.starts_with("failed to read file:"), "{err}");
    let null_doc = dir.write("null.json", "null");
    assert!(
        ChatGptImporter::new()
            .import_export(&export_ref(&null_doc))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn size_boundary_is_exact() {
    for (extra, rejected) in [(0, false), (1, true)] {
        let dir = TestDir::new("chatgpt-size");
        let path = dir.write("conversations.json", "[]");
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_len(MAX_EXPORT_BYTES + extra).unwrap();
        let err = ChatGptImporter::new()
            .import_export(&export_ref(&path))
            .unwrap_err();
        assert_eq!(err.contains("exceeds maximum size"), rejected, "{err}");
    }
}

#[test]
fn discovery_finds_regular_exports_and_skips_the_rest() {
    let dir = TestDir::new("chatgpt-discover");
    let first = dir.write("conversations.json", "[]");
    let nested = dir.write("sub/2024-conversations.json", "[]");
    dir.write("sub/notes.txt", "x");
    let refs = ChatGptImporter::new()
        .discover_exports(&dir.path().to_string_lossy())
        .unwrap();
    let paths: Vec<_> = refs.iter().map(|r| r.path.clone()).collect();
    assert_eq!(
        paths,
        [first, nested].map(|p| p.to_string_lossy().into_owned())
    );
    assert!(
        refs.iter()
            .all(|r| r.tool == "chatgpt" && r.format == "json")
    );
    let single = ChatGptImporter::new().discover_exports(&paths[0]).unwrap();
    assert_eq!(single.len(), 1);
    let txt = dir.path().join("sub/notes.txt");
    assert!(
        ChatGptImporter::new()
            .discover_exports(&txt.to_string_lossy())
            .unwrap()
            .is_empty()
    );
    assert!(
        ChatGptImporter::new()
            .discover_exports("/nonexistent/path")
            .is_err()
    );
}

#[test]
fn empty_path_uses_downloads_below_the_given_home() {
    let home = TestDir::new("chatgpt-home");
    let export = home.write("Downloads/safe/conversations.json", "[]");
    let refs = ChatGptImporter::with_home(home.path().to_path_buf())
        .discover_exports("")
        .unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].path, export.to_string_lossy());
}

#[cfg(unix)]
#[test]
fn symlinks_and_special_files_are_never_followed() {
    use std::os::unix::fs::symlink;
    let root = TestDir::new("chatgpt-links");
    let outside = TestDir::new("chatgpt-outside");
    outside.write("conversations.json", "[]");
    symlink(outside.path(), root.path().join("escaped")).unwrap();
    let safe = root.write("safe/conversations.json", "[]");
    let target = root.write("real.json", "[]");
    symlink(&target, root.path().join("link.json")).unwrap();
    let fifo = root.path().join("conversations.json");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());

    let refs = ChatGptImporter::new()
        .discover_exports(&root.path().to_string_lossy())
        .unwrap();
    assert_eq!(refs.len(), 1, "{refs:?}");
    assert_eq!(refs[0].path, safe.to_string_lossy());

    let importer = ChatGptImporter::new();
    assert!(
        importer
            .import_export(&export_ref(&root.path().join("link.json")))
            .is_err()
    );
    // A FIFO must be rejected without opening (opening would block).
    let err = importer.import_export(&export_ref(&fifo)).unwrap_err();
    assert!(err.contains("not a regular file"), "{err}");
    assert!(
        importer
            .import_export(&export_ref(Path::new("/dev/null")))
            .is_err()
    );
    // A symlinked search root yields nothing.
    let via_link = root.path().join("escaped");
    assert!(
        importer
            .discover_exports(&via_link.to_string_lossy())
            .unwrap()
            .is_empty()
    );
}
