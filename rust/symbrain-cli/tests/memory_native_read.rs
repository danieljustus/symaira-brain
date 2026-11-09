//! Read commands (`search`, `list`, `rules`, `delete`) against the Go oracle
//! for the configurations and database states that used to stay on Go (#758).

#[path = "support/memory_native_oracle.rs"]
mod oracle;

use oracle::{FakeReply, RecordedRequest, Scenario, argv, compare};
use std::ffi::OsString;
use std::sync::Arc;

const BIN: &str = env!("CARGO_BIN_EXE_symbrain");

/// `doc<N>` texts map to e0 with N positive side components, so the Hamming
/// distance to another document is |N - M| while every document stays in the
/// same LSH bucket. The side value is large enough that neighbouring
/// documents differ in cosine by far more than the run-time dependent
/// recency term can move.
fn vector_for(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0_f32; 768];
    vector[0] = 1.0;
    let number = text
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse::<usize>().ok())
        .unwrap_or(0);
    for slot in vector.iter_mut().skip(1).take(number) {
        *slot = 0.1;
    }
    vector
}

fn embedder() -> Arc<dyn Fn(&RecordedRequest) -> FakeReply + Send + Sync> {
    Arc::new(|request: &RecordedRequest| {
        let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap_or_default();
        let text = body["input"][0].as_str().unwrap_or_default().to_owned();
        FakeReply {
            status: 200,
            body: serde_json::json!({"data": [{"index": 0, "embedding": vector_for(&text)}]})
                .to_string()
                .into_bytes(),
            content_type: "application/json",
        }
    })
}

fn set(content: &str) -> Vec<OsString> {
    argv(&["set", content, "--kind", "user", "--db", "{DB}"])
}

fn search(query: &str, extra: &[&str]) -> Vec<OsString> {
    let mut args = vec!["search", query, "--db", "{DB}"];
    args.extend_from_slice(extra);
    argv(&args)
}

const CONFIG: &str = "ollama.url = \"{EMBED}\"\n[conflict]\nenabled = false\n[hybrid_search]\nquantize_to_binary = true\nprefilter_enabled = true\n";

fn seeded<'a>(name: &'a str, config: &str, documents: usize) -> Scenario<'a> {
    let mut scenario = Scenario::new(name, BIN);
    config.clone_into(&mut scenario.config);
    scenario.embed = Some(embedder());
    scenario.seed = (0..documents)
        .map(|index| set(&format!("doc{index} marker")))
        .collect();
    scenario
}

#[test]
fn prefilter_selects_the_hamming_nearest_candidates() {
    let mut s = seeded("prefilter", CONFIG, 90);
    s.steps = vec![
        search("doc40 marker", &["--json"]),
        search("doc40 marker", &["--limit", "3"]),
        search("doc3 marker", &["--limit", "50", "--json"]),
        search("doc85 marker", &["--scope", "global"]),
    ];
    compare(&s);
}

#[test]
fn prefilter_is_inert_below_the_cutoff_and_without_binary_vectors() {
    let mut small = seeded("prefilter-small", CONFIG, 20);
    small.steps = vec![search("doc10 marker", &["--json"])];
    compare(&small);

    // Rows written without binary quantization disable the prefilter.
    let config = "ollama.url = \"{EMBED}\"\n[conflict]\nenabled = false\n[hybrid_search]\nprefilter_enabled = true\n";
    let mut plain = seeded("prefilter-no-binary", config, 90);
    plain.steps = vec![search("doc40 marker", &["--json"])];
    compare(&plain);
}

#[test]
fn conflict_recall_honours_the_prefilter() {
    let config = "ollama.url = \"{EMBED}\"\n[hybrid_search]\nquantize_to_binary = true\nprefilter_enabled = true\n";
    let mut s = seeded("prefilter-conflict", config, 5);
    s.steps = vec![set("doc2 marker new")];
    compare(&s);
}

// --- Rows the shipped row hydration treats unusually (#759) --------------

const PLAIN_CONFIG: &str = "ollama.url = \"{EMBED}\"\n[conflict]\nenabled = false\n";

fn plain(name: &str, mutations: &[&str]) -> Scenario<'static> {
    let mut scenario = Scenario::new(Box::leak(name.to_owned().into_boxed_str()), BIN);
    PLAIN_CONFIG.clone_into(&mut scenario.config);
    scenario.embed = Some(embedder());
    scenario.seed_key = Some("rows3");
    scenario.seed = (0..3)
        .map(|index| set(&format!("doc{index} marker")))
        .collect();
    scenario.mutate_sql = mutations.iter().map(|sql| (*sql).to_owned()).collect();
    scenario
}

fn read_steps() -> Vec<Vec<OsString>> {
    vec![
        argv(&["list", "--db", "{DB}", "--json"]),
        argv(&["list", "--db", "{DB}"]),
        search("doc1 marker", &["--json"]),
        search("doc1 marker", &[]),
        argv(&["rules", "--db", "{DB}", "--json"]),
        argv(&["rules", "--db", "{DB}"]),
    ]
}

#[test]
fn metadata_hydration_matches_for_every_stored_shape() {
    let shapes = [
        ("broken", "broken"),
        ("empty", ""),
        ("array", "[]"),
        ("null", "null"),
        ("false", "false"),
        ("number-member", "{\"n\":1}"),
        ("null-member", "{\"n\":null}"),
        ("duplicate-keys", "{\"n\":\"a\",\"n\":\"b\"}"),
        ("nested", "{\"n\":{\"x\":\"y\"}}"),
        ("trailing", "{\"n\":\"a\"} x"),
        ("spaced", " {\"n\" : \"a\"} "),
        ("escapes", "{\"n\":\"\\u00e9\\n<&>\\u2028\"}"),
        ("big-key", "{\"\":\"empty key\"}"),
    ];
    for (label, value) in shapes {
        let sql = format!(
            "UPDATE memories SET metadata='{}' WHERE rowid=2",
            value.replace('\'', "''")
        );
        let mut s = plain(&format!("meta-{label}"), &[&sql]);
        s.steps = read_steps();
        compare(&s);
    }
}

#[test]
fn embedding_hydration_matches_for_every_stored_shape() {
    let shapes = [
        ("broken", "broken"),
        ("empty", ""),
        ("object", "{}"),
        ("null", "null"),
        ("strings", "[\"x\"]"),
        ("overflow", "[1e39]"),
        ("null-member", "[null]"),
        ("short", "[1,2]"),
        ("integers", "[1,0,0]"),
        ("empty-list", "[]"),
        ("trailing", "[1.0] x"),
        ("nan-text", "[NaN]"),
    ];
    for (label, value) in shapes {
        let sql = format!("UPDATE memories SET embedding='{value}' WHERE rowid=2");
        let mut s = plain(&format!("embedding-{label}"), &[&sql]);
        s.steps = read_steps();
        compare(&s);
    }
}

#[test]
fn timestamp_and_numeric_hydration_matches() {
    let shapes = [
        (
            "created-invalid",
            "UPDATE memories SET created_at='invalid' WHERE rowid=2",
        ),
        (
            "created-leap",
            "UPDATE memories SET created_at='2000-01-01 00:00:60 +0000 UTC' WHERE rowid=2",
        ),
        (
            "created-rfc3339",
            "UPDATE memories SET created_at='2020-05-06T07:08:09Z' WHERE rowid=2",
        ),
        (
            "created-offset",
            "UPDATE memories SET created_at='2020-05-06 07:08:09.123456789 +0200 CEST' WHERE rowid=2",
        ),
        (
            "created-integer",
            "UPDATE memories SET created_at=1700000000 WHERE rowid=2",
        ),
        (
            "created-empty",
            "UPDATE memories SET created_at='' WHERE rowid=2",
        ),
        (
            "updated-invalid",
            "UPDATE memories SET updated_at='not a time' WHERE rowid=2",
        ),
        (
            "valid-to-invalid",
            "UPDATE memories SET valid_to='x' WHERE rowid=2",
        ),
        (
            "importance-text",
            "UPDATE memories SET importance='high' WHERE rowid=2",
        ),
        (
            "access-count-real",
            "UPDATE memories SET access_count=1.5 WHERE rowid=2",
        ),
        (
            "scope-blob",
            "UPDATE memories SET scope=x'ff00fe' WHERE rowid=2",
        ),
        (
            "content-blob",
            "UPDATE memories SET content=x'ff00fe41' WHERE rowid=2",
        ),
        (
            "content-int",
            "UPDATE memories SET content=42 WHERE rowid=2",
        ),
        ("kind-empty", "UPDATE memories SET kind='' WHERE rowid=2"),
        (
            "expires-text",
            "UPDATE memories SET expires_at='soon' WHERE rowid=2",
        ),
        (
            "last-access-fractional",
            "UPDATE memories SET last_access='2020-01-02 03:04:05.5 +0000 UTC' WHERE rowid=2",
        ),
    ];
    for (label, sql) in shapes {
        let mut s = plain(&format!("row-{label}"), &[sql]);
        s.steps = read_steps();
        compare(&s);
    }
}

#[test]
fn rules_hydration_matches() {
    let insert = |id: &str, scope: &str, metadata: &str, created: &str| {
        format!(
            "INSERT INTO rules(id, content, scope, metadata, created_at) VALUES('{id}','rule {id}','{scope}','{metadata}','{created}')"
        )
    };
    let good = insert("r1", "global", "{}", "2020-01-02 03:04:05 +0000 UTC");
    let cases = [
        ("plain", vec![good.clone()]),
        (
            "metadata-broken",
            vec![
                good.clone(),
                insert("r2", "global", "broken", "2020-01-02 03:04:05 +0000 UTC"),
            ],
        ),
        (
            "metadata-number",
            vec![
                good.clone(),
                insert("r2", "global", "{\"a\":1}", "2020-01-02 03:04:05 +0000 UTC"),
            ],
        ),
        (
            "metadata-null",
            vec![
                good.clone(),
                insert("r2", "global", "null", "2020-01-02 03:04:05 +0000 UTC"),
            ],
        ),
        (
            "created-invalid",
            vec![good.clone(), insert("r2", "global", "{}", "yesterday")],
        ),
        (
            "scoped",
            vec![
                good,
                insert(
                    "r2",
                    "project:x",
                    "{\"k\":\"v\"}",
                    "2021-01-02 03:04:05.5 +0000 UTC",
                ),
            ],
        ),
    ];
    for (label, statements) in cases {
        let mut s = plain(&format!("rules-{label}"), &[]);
        s.mutate_sql = statements;
        s.steps = vec![
            argv(&["rules", "--db", "{DB}", "--json"]),
            argv(&["rules", "--db", "{DB}"]),
            argv(&["rules", "--db", "{DB}", "--scope", "project:x", "--json"]),
        ];
        compare(&s);
    }
}

#[test]
fn delete_matches_for_stored_rows_and_identifiers() {
    use std::os::unix::ffi::OsStringExt;
    let mut s = plain("delete-plain", &[]);
    s.steps = vec![
        argv(&["delete", "{FIRST_ID}", "--db", "{DB}", "--json"]),
        argv(&["delete", "{FIRST_ID}", "--db", "{DB}"]),
        argv(&["delete", "no-such-id", "--db", "{DB}", "--json"]),
        argv(&["delete", "  ", "--db", "{DB}"]),
        argv(&["delete", "--db", "{DB}"]),
        argv(&["delete", "a", "b", "--db", "{DB}"]),
        argv(&["list", "--db", "{DB}", "--json"]),
    ];
    compare(&s);

    let mut raw = plain("delete-non-utf8-id", &[]);
    raw.steps = vec![
        vec![
            OsString::from("delete"),
            OsString::from_vec(vec![b'x', 0xff, 0xfe, b'y']),
            OsString::from("--db"),
            OsString::from("{DB}"),
        ],
        vec![
            OsString::from("delete"),
            OsString::from_vec(vec![b'x', 0xff]),
            OsString::from("--db"),
            OsString::from("{DB}"),
            OsString::from("--json"),
        ],
    ];
    compare(&raw);

    let mut spaced = plain("delete-trimmed-id", &[]);
    spaced.steps = vec![
        argv(&["delete", " {FIRST_ID} ", "--db", "{DB}"]),
        argv(&["list", "--db", "{DB}", "--json"]),
    ];
    compare(&spaced);

    let mut html = plain("delete-escaped-id", &[]);
    html.steps = vec![
        argv(&["delete", "a<b>&\u{2028}\"", "--db", "{DB}", "--json"]),
        argv(&["delete", "a<b>&\u{2028}\"", "--db", "{DB}"]),
    ];
    compare(&html);
}

#[test]
fn delete_of_malformed_rows_matches() {
    for (label, sql) in [
        (
            "metadata",
            "UPDATE memories SET metadata='broken' WHERE rowid=1",
        ),
        (
            "embedding",
            "UPDATE memories SET embedding='broken' WHERE rowid=1",
        ),
        (
            "created",
            "UPDATE memories SET created_at='invalid' WHERE rowid=1",
        ),
        (
            "leap",
            "UPDATE memories SET created_at='2000-01-01 00:00:60 +0000 UTC' WHERE rowid=1",
        ),
        (
            "importance",
            "UPDATE memories SET importance='x' WHERE rowid=1",
        ),
    ] {
        let mut s = plain(&format!("delete-malformed-{label}"), &[sql]);
        s.steps = vec![
            argv(&["delete", "{FIRST_ID}", "--db", "{DB}", "--json"]),
            argv(&["list", "--db", "{DB}", "--json"]),
        ];
        compare(&s);
    }
}
