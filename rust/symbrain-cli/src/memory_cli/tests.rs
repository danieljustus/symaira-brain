//! Existing native memory CLI regression tests.

use super::search::escape_html;
use super::*;
use rusqlite::Connection;
use serde_json::json;
use tempfile::tempdir;

fn fixture() -> tempfile::TempDir {
    let directory = tempdir().expect("fixture directory");
    let path = directory.path().join("memory.sqlite");
    let store = Store::open(&path).expect("initialize fixture database");
    drop(store);
    let connection = Connection::open(path).expect("open fixture database");
    connection
            .execute_batch(
                "INSERT INTO query_log(id,actor,scope,session,tool,query_text,params,duration_ms,created_at) VALUES
                ('q1','claude','global','s1','memory_search','tabs',NULL,12,'2026-03-01 00:00:00 +0000 UTC'),
                ('q2','codex','global','s2','memory_list',NULL,NULL,3,'2026-03-01 00:01:00 +0000 UTC'),
                ('q3',NULL,NULL,NULL,'memory_search','legacy','{}',7,'2026-03-01 00:02:00.123456 +0000 UTC');",
            )
            .expect("seed fixture database");
    directory
}

#[test]
fn query_log_json_matches_go_fixture() {
    let directory = fixture();
    let path = directory.path().join("memory.sqlite");
    let args = [OsString::from("--db"), path.into_os_string()];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Json),
        exit::OK
    );
    assert!(stderr.is_empty());
    let actual: serde_json::Value = serde_json::from_slice(&stdout).expect("valid JSON");
    assert_eq!(
        actual,
        json!({
            "total_queries": 3,
            "tool_breakdown": {"memory_list": 1, "memory_search": 2},
            "actor_breakdown": {"(unknown)": 1, "claude": 1, "codex": 1},
            "recent_entries": [
                {"id": "q3", "tool": "memory_search", "query_text": "legacy", "params": "{}", "duration_ms": 7, "created_at": "2026-03-01T00:02:00.123456Z"},
                {"id": "q2", "actor": "codex", "scope": "global", "session": "s2", "tool": "memory_list", "duration_ms": 3, "created_at": "2026-03-01T00:01:00Z"},
                {"id": "q1", "actor": "claude", "scope": "global", "session": "s1", "tool": "memory_search", "query_text": "tabs", "duration_ms": 12, "created_at": "2026-03-01T00:00:00Z"}
            ]
        })
    );
}

#[test]
fn query_log_table_and_actor_filter_match_go_shape() {
    let directory = fixture();
    let path = directory.path().join("memory.sqlite");
    let args = [
        OsString::from("--db"),
        path.into_os_string(),
        OsString::from("--actor"),
        OsString::from("codex"),
        OsString::from("--limit=99999"),
    ];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Table),
        exit::OK
    );
    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).expect("UTF-8 table"),
        "Total queries: 1\n\nWHEN\tTOOL\tACTOR\tMS\tQUERY\n2026-03-01T00:01:00Z\tmemory_list\tcodex\t3\t\n"
    );
}

#[test]
fn query_log_rejects_positionals_and_unknown_flags() {
    for args in [
        [OsString::from("unexpected")],
        [OsString::from("--nonsense")],
    ] {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        assert_eq!(
            run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Table),
            exit::USAGE
        );
        assert!(stdout.is_empty());
        assert!(!stderr.is_empty());
    }
}

#[test]
fn query_log_table_sanitizes_multiline_query_text() {
    let directory = fixture();
    let path = directory.path().join("memory.sqlite");
    let connection = Connection::open(&path).expect("open fixture database");
    connection
            .execute(
                "INSERT INTO query_log(id,tool,query_text,duration_ms,created_at) VALUES (?, ?, ?, ?, ?)",
                (
                    "q4",
                    "memory_search",
                    "line one\tline two\rline three\nline four",
                    4,
                    "2026-03-01T00:03:00Z",
                ),
            )
            .expect("seed multiline query");
    drop(connection);

    let args = [OsString::from("--db"), path.into_os_string()];
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_query_log(&args, &mut stdout, &mut stderr, OutputFormat::Table),
        exit::OK
    );
    let output = String::from_utf8(stdout).expect("UTF-8 table");
    assert!(output.contains("line one line two line three line four"));
    assert!(!output.contains("line one\tline two"));
    assert!(!output.contains("line three\nline four"));
}

#[test]
fn query_log_normalizes_schema_default_timestamp() {
    let directory = fixture();
    let path = directory.path().join("memory.sqlite");
    let connection = Connection::open(&path).expect("open fixture database");
    connection
            .execute(
                "INSERT INTO query_log(id,tool,query_text,duration_ms,created_at) VALUES (?, ?, ?, ?, ?)",
                (
                    "q5",
                    "memory_list",
                    "schema default",
                    5,
                    "2026-03-01 00:03:00",
                ),
            )
            .expect("seed schema-default timestamp");
    drop(connection);

    let args = [OsString::from("--db"), path.into_os_string()];
    let mut json_output = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_query_log(&args, &mut json_output, &mut stderr, OutputFormat::Json),
        exit::OK
    );
    let summary: serde_json::Value =
        serde_json::from_slice(&json_output).expect("valid JSON summary");
    assert_eq!(
        summary["recent_entries"][0]["created_at"],
        "2026-03-01T00:03:00Z"
    );

    let mut table_output = Vec::new();
    assert_eq!(
        run_query_log(&args, &mut table_output, &mut stderr, OutputFormat::Table),
        exit::OK
    );
    assert!(
        String::from_utf8(table_output)
            .expect("UTF-8 table")
            .contains("2026-03-01T00:03:00Z\tmemory_list")
    );
}

#[test]
fn search_flag_parser_rejects_unsupported_query_shapes() {
    let allowed = |args: &[&str]| {
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        flags::parse(&args, "search", &mut Vec::new())
            .is_ok_and(|parsed| parsed.positional.len() == 1)
    };
    assert!(allowed(&["alpha"]));
    assert!(allowed(&["alpha", "--scope", "global"]));
    assert!(allowed(&["alpha", "-s=project", "--limit", "3"]));
    assert!(allowed(&["--db", "/tmp/x.db", "alpha"]));
    assert!(allowed(&["--limit=0", "two words"]));
    // Missing, doubled or flag-shaped queries, and any unknown flag, keep
    // the shipped usage or flag error.
    assert!(!allowed(&[]));
    assert!(!allowed(&["alpha", "beta"]));
    assert!(!allowed(&["alpha", "--bogus"]));
    assert!(!allowed(&["--help"]));
    assert!(!allowed(&["alpha", "--limit"]));
    assert!(!allowed(&["alpha", "--scope"]));
}

#[test]
fn search_refuses_a_missing_query_with_the_shipped_message() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(
        run_search(&[], &mut stdout, &mut stderr, OutputFormat::Table),
        exit::USAGE
    );
    assert!(stdout.is_empty());
    assert_eq!(
        String::from_utf8(stderr).expect("UTF-8 stderr"),
        "usage: symbrain memory search <query> [--scope <scope>] [--limit <N>] [--db <path>]\n"
    );
}

#[test]
fn search_prints_the_shipped_usage_for_help() {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let args = [OsString::from("--help")];
    assert_eq!(
        run_search(&args, &mut stdout, &mut stderr, OutputFormat::Table),
        exit::OK
    );
    assert!(stderr.is_empty());
    assert!(
        String::from_utf8(stdout)
            .expect("UTF-8 stdout")
            .starts_with("symbrain memory search — search memories by semantic relevance\n")
    );
}

#[test]
fn search_flags_consume_their_values() {
    let cases = [
        vec!["search", "alpha", "--db", "/tmp/memory.db"],
        vec!["search", "alpha", "--limit", "2", "--db", "/tmp/memory.db"],
        vec![
            "search",
            "alpha",
            "-l",
            "2",
            "-s",
            "global",
            "--db=/tmp/memory.db",
        ],
    ];
    for case in cases {
        let args = case.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(
            !requires_go_fallback(&args),
            "{case:?} should stay native so the parser owns the values"
        );
    }
}

#[test]
fn json_output_escapes_what_the_shipped_encoder_escapes() {
    assert_eq!(
        escape_html("a&b<c>d\u{2028}e\u{2029}"),
        "a\\u0026b\\u003cc\\u003ed\\u2028e\\u2029"
    );
}

/// The shapes the native path owns because the shipped implementation
/// writes them itself instead of going through the flag package.
#[test]
fn hand_written_usage_shapes_stay_native() {
    let native = [
        vec![],
        vec!["-h"],
        vec!["--help"],
        vec!["help"],
        vec!["list", "--help"],
        vec!["search"],
        vec!["set"],
        vec!["delete"],
        vec!["sync"],
    ];
    for case in native {
        let args = case.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(
            !requires_go_fallback(&args),
            "{case:?} is a hand-written usage or help shape and must stay native"
        );
    }
}

/// Real synchronisation work must never run natively: only the bare
/// `memory sync` shape is ported, everything that names a remote keeps the
/// shipped implementation.
#[test]
fn memory_sync_with_arguments_stays_on_go() {
    for case in [
        vec!["sync", "--remote", "https://example.test"],
        vec!["sync", "--pull"],
        vec!["sync", "--remote=https://example.test", "--push"],
    ] {
        let args = case.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(
            requires_go_fallback(&args),
            "{case:?} carries synchronisation arguments and must go to Go"
        );
    }
}

#[test]
fn default_conflict_route_admits_existing_store_but_not_creation_or_bad_hydration() {
    let directory = fixture();
    let path = directory.path().join("memory.sqlite");
    let command = |database: &std::path::Path| vec![
        OsString::from("set"), OsString::from("hello world"),
        OsString::from("--kind"), OsString::from("reference"),
        OsString::from("--db"), database.as_os_str().to_owned(),
    ];
    assert!(!requires_go_fallback(&command(&path)));
    assert!(requires_go_fallback(&command(&directory.path().join("missing.sqlite"))));
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES('bad','hello world','global','{}','[]','invalid','invalid')").unwrap();
    assert!(requires_go_fallback(&command(&path)));
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM memories", [], |row| row.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn prefilter_admission_keeps_creation_and_malformed_reads_on_go() {
    let directory = fixture();
    let path = directory.path().join("memory.sqlite");
    assert!(!Store::direct_prefilter_supported(&directory.path().join("absent.sqlite")));
    assert_eq!(Store::direct_prefilter_supported(&path), usize::BITS == 64);
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES('bad','retained','global','{}','[]','invalid','invalid')").unwrap();
    assert!(!Store::direct_prefilter_supported(&path));
    assert_eq!(conn.query_row("SELECT created_at FROM memories WHERE id='bad'", [], |row| row.get::<_, String>(0)).unwrap(), "invalid");
    for limit in [i64::MIN, -1, 0, 17, i64::MAX / 4 + 1, i64::MAX] {
        let args = [OsString::from("query"), OsString::from(format!("--limit={limit}"))];
        let parsed = flags::parse(&args, "search", &mut Vec::new()).unwrap();
        assert_eq!(parsed.limit, limit, "do not clamp before Go's signed multiplication");
    }
}
