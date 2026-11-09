//! Conflict resolution, the LLM verdict tier, entities, projects and raw-byte
//! arguments of `symbrain memory set` against the Go oracle (#758). The LLM
//! and embedding endpoints are local fake servers; no real provider is used.

#[path = "support/memory_native_oracle.rs"]
mod oracle;

use oracle::{FakeReply, RecordedRequest, Scenario, argv, compare};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::sync::Arc;

const BIN: &str = env!("CARGO_BIN_EXE_symbrain");

type Handler = Arc<dyn Fn(&RecordedRequest) -> FakeReply + Send + Sync>;

/// Maps a text to a unit-ish vector so cosine similarities are controlled:
/// `near` is 0.97 from `original`, `band` is 0.85 from it, everything else
/// is orthogonal.
fn vector_for(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0_f32; 768];
    let (main, side_index, side) = if text.contains("alpha near") {
        (0.97_f32, 1, 0.243_f32)
    } else if text.contains("alpha band") {
        (0.85, 2, 0.527)
    } else if text.contains("alpha") {
        (1.0, 1, 0.0)
    } else {
        let slot = 10 + text.len() % 500;
        vector[slot] = 1.0;
        return vector;
    };
    vector[0] = main;
    vector[side_index] = side;
    vector
}

fn embedder() -> Handler {
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

fn llm(reply: &'static str) -> Handler {
    Arc::new(move |_| FakeReply {
        status: 200,
        body: format!(
            "{}\n",
            serde_json::json!({"model": "m", "response": reply, "done": true})
        )
        .into_bytes(),
        content_type: "application/x-ndjson",
    })
}

fn failing_llm(status: u16, body: &'static str) -> Handler {
    Arc::new(move |_| FakeReply {
        status,
        body: body.as_bytes().to_vec(),
        content_type: "text/plain",
    })
}

fn set(content: &str, extra: &[&str]) -> Vec<OsString> {
    let mut args = vec!["set", content, "--kind", "user", "--db", "{DB}"];
    args.extend_from_slice(extra);
    argv(&args)
}

fn scenario<'a>(name: &'a str, config: &str) -> Scenario<'a> {
    let mut scenario = Scenario::new(name, BIN);
    config.clone_into(&mut scenario.config);
    scenario.embed = Some(embedder());
    scenario
}

const BASE: &str = "ollama.url = \"{EMBED}\"\n";

fn llm_config(provider: &str) -> String {
    format!(
        "{BASE}[conflict]\nllm_provider = \"{provider}\"\nllm_url = \"{{LLM}}\"\nllm_model = \"fake\"\n"
    )
}

fn has(db: &[String], needle: &str) -> bool {
    db.iter().any(|row| row.contains(needle))
}

#[test]
fn near_duplicate_replaces_the_older_row() {
    let mut s = scenario("near-dup", BASE);
    s.seed = vec![set("alpha original statement", &[])];
    s.steps = vec![set("alpha near statement", &["--json"])];
    let compared = compare(&s);
    // The shipped supersede statement writes NULL into a NOT NULL column, so
    // the loser is never closed and only a warning is logged.
    let log = String::from_utf8_lossy(&compared.go[0].stderr).into_owned();
    assert!(log.contains("failed to mark memory superseded"), "{log}");
    assert!(!has(&compared.db, "\\\"supersede"), "{:#?}", compared.db);
}

#[test]
fn higher_importance_keeps_the_existing_fact() {
    let mut s = scenario("importance", &llm_config("ollama"));
    s.llm = Some(llm(
        r#"{"verdicts":[{"pair":0,"verdict":"contradiction"}]}"#,
    ));
    s.seed = vec![set("alpha original statement", &[])];
    s.mutate_sql = vec!["UPDATE memories SET importance = 5".to_owned()];
    s.steps = vec![set("alpha band statement", &["--json"])];
    let compared = compare(&s);
    assert!(has(&compared.db, "importance\\\""), "{:#?}", compared.db);
    assert!(
        has(&compared.db, "action=\"supersede\""),
        "{:#?}",
        compared.db
    );
}

#[test]
fn band_pair_without_llm_is_surfaced_as_pending() {
    let mut s = scenario("band-pending", BASE);
    s.seed = vec![set("alpha original statement", &[])];
    s.steps = vec![set("alpha band statement", &[])];
    let compared = compare(&s);
    assert!(has(&compared.db, "conflict_pending"), "{:#?}", compared.db);
}

#[test]
fn llm_verdicts_resolve_the_band() {
    for (name, reply, expect) in [
        (
            "llm-contradiction",
            r#"{"verdicts":[{"pair":0,"verdict":"contradiction"}]}"#,
            "failed to mark memory superseded",
        ),
        (
            "llm-repeat",
            r#"{"verdicts":[{"pair":0,"verdict":"repeat"}]}"#,
            "failed to mark memory superseded",
        ),
        (
            "llm-ambiguous",
            r#"{"verdicts":[{"pair":0,"verdict":"ambiguous"}]}"#,
            "conflict_pending",
        ),
        (
            "llm-salvaged",
            r#"here you go: "verdict": "Contradiction""#,
            "failed to mark memory superseded",
        ),
        ("llm-garbage", "no verdicts at all", "conflict_pending"),
    ] {
        let mut s = scenario(name, &llm_config("ollama"));
        s.llm = Some(llm(reply));
        s.seed = vec![set("alpha original statement", &[])];
        s.steps = vec![set("alpha band statement", &["--json"])];
        let compared = compare(&s);
        let log = String::from_utf8_lossy(&compared.go[0].stderr).into_owned();
        assert!(
            has(&compared.db, expect) || log.contains(expect),
            "{name}: {log} {:#?}",
            compared.db
        );
        let go = compared.go_requests.1;
        let rust = compared.rust_requests.1;
        assert_eq!(go.len(), 1, "{name}: one verdict call");
        assert_eq!(go.len(), rust.len(), "{name}: request count");
        assert_eq!(go[0].request_line, rust[0].request_line, "{name}");
        let parse = |request: &RecordedRequest| {
            serde_json::from_slice::<serde_json::Value>(&request.body).expect("json body")
        };
        let (go_body, rust_body) = (parse(&go[0]), parse(&rust[0]));
        assert_eq!(go_body["model"], "fake");
        assert_eq!(
            go_body["system"], rust_body["system"],
            "{name}: system prompt"
        );
        // The prompt carries fresh timestamps' redaction-free text only.
        assert_eq!(go_body["prompt"], rust_body["prompt"], "{name}: prompt");
        assert_eq!(go_body["format"], rust_body["format"], "{name}: schema");
        assert_eq!(go_body["stream"], rust_body["stream"], "{name}: stream");
    }
}

#[test]
fn failing_llm_degrades_to_storing_both() {
    for (name, status, body) in [
        ("llm-500", 500, "boom"),
        ("llm-404", 404, "no such model"),
        ("llm-401", 401, "denied"),
        ("llm-429", 429, "slow down"),
    ] {
        let mut s = scenario(name, &llm_config("ollama"));
        s.llm = Some(failing_llm(status, body));
        s.seed = vec![set("alpha original statement", &[])];
        s.steps = vec![set("alpha band statement", &[])];
        s.stderr_as_set = true;
        let compared = compare(&s);
        assert!(
            has(&compared.db, "conflict_pending"),
            "{name}: {:#?}",
            compared.db
        );
        let log = String::from_utf8_lossy(&compared.go[0].stderr).into_owned();
        assert!(log.contains("verdict tier failed"), "{name}: {log}");
    }
}

#[test]
fn openai_provider_without_a_key_degrades() {
    let mut s = scenario("openai-nokey", &llm_config("openai"));
    s.seed = vec![set("alpha original statement", &[])];
    s.steps = vec![set("alpha band statement", &[])];
    s.stderr_as_set = true;
    let compared = compare(&s);
    assert!(has(&compared.db, "conflict_pending"));
}

#[test]
fn unreachable_llm_degrades() {
    let mut s = scenario("llm-unreachable", &llm_config("ollama"));
    s.seed = vec![set("alpha original statement", &[])];
    s.steps = vec![set("alpha band statement", &[])];
    s.stderr_as_set = true;
    compare(&s);
}

#[test]
fn threshold_overrides_change_the_outcome() {
    let config = format!(
        "{BASE}[conflict]\nnear_dup_threshold = 0.99\ncontradiction_threshold = 0.5\nmax_candidates = 3\n"
    );
    let mut s = scenario("thresholds", &config);
    s.seed = vec![set("alpha original statement", &[])];
    s.steps = vec![set("alpha near statement", &[])];
    let compared = compare(&s);
    assert!(has(&compared.db, "conflict_pending"));
}

#[test]
fn conflict_disabled_keeps_the_legacy_write() {
    let config = format!("{BASE}[conflict]\nenabled = false\n");
    let mut s = scenario("disabled", &config);
    s.seed = vec![set("alpha original statement", &[])];
    s.steps = vec![set("alpha original statement", &["--json"])];
    compare(&s);
}

#[test]
fn project_scope_entities_and_metadata() {
    let mut s = scenario("project-entities", BASE);
    s.steps = vec![
        set(
            "ship the release",
            &[
                "--scope",
                "project",
                "--entities",
                " Alice , bob ,, ",
                "--metadata",
                r#"{"team":"core","note":"a<b&c"}"#,
            ],
        ),
        set(
            "second note",
            &["--scope", "project", "--entities", "ALICE,Carol"],
        ),
    ];
    let compared = compare(&s);
    assert!(has(&compared.db, "project_name"));
    assert!(has(&compared.db, "entity_create"));
}

#[test]
fn entity_resolved_through_an_alias() {
    let mut s = scenario("alias", BASE);
    s.seed = vec![set("seed note", &["--entities", "Alice"])];
    s.mutate_sql = vec![
        "INSERT INTO entities_aliases(entity_id, alias) SELECT id, 'Al' FROM entities WHERE name='Alice'"
            .to_owned(),
    ];
    s.steps = vec![set("later note", &["--entities", "al"])];
    compare(&s);
}

#[test]
fn metadata_and_scope_errors_match() {
    let mut s = scenario("errors", BASE);
    s.steps = vec![
        set("x note", &["--metadata", "broken"]),
        set("x note", &["--metadata", "[1]"]),
        set("x note", &["--metadata", r#"{"n":1}"#]),
        set("x note", &["--metadata", r#"{"n":null}"#]),
        set("x note", &["--metadata", "null"]),
        set("x note", &["--metadata", "{\"a\":\"b\"} trailing"]),
        set("x note", &["--scope", "nowhere"]),
        set("x note", &["--scope", ""]),
        set("x note", &["--kind", "bogus"]),
    ];
    compare(&s);
}

#[test]
fn pii_in_metadata_and_secondary_facts() {
    let mut s = scenario("pii-meta", BASE);
    s.steps = vec![set(
        "I use mail me@example.com daily. my favorite is tea from alice@example.org",
        &["--metadata", r#"{"contact":"bob@example.com"}"#, "--json"],
    )];
    s.stderr_as_set = true;
    compare(&s);
}

#[test]
fn secondary_facts_with_conflict_and_dedupe() {
    let mut s = scenario("facts-conflict", BASE);
    s.seed = vec![set("I like tea", &[])];
    s.steps = vec![
        set(
            "I like tea. I use vim. Another unrelated sentence here",
            &[],
        ),
        set(
            "I like tea. I use vim. Another unrelated sentence here",
            &[],
        ),
    ];
    compare(&s);
}

#[test]
fn raw_bytes_in_author_entities_and_content() {
    let mut s = scenario("raw-bytes", BASE);
    let mut author = b"cli:".to_vec();
    author.push(0xff);
    let mut entity = b"Zo".to_vec();
    entity.push(0xe9);
    let mut content = b"caf".to_vec();
    content.push(0xe9);
    content.extend_from_slice(b" bytes");
    let mut args = set("placeholder", &[]);
    args[1] = OsString::from_vec(content);
    args.push(OsString::from("--author"));
    args.push(OsString::from_vec(author));
    args.push(OsString::from("--entities"));
    args.push(OsString::from_vec(entity));
    s.steps = vec![args];
    compare(&s);
}

#[test]
fn ollama_embeddings_record_model_and_source() {
    let mut s = scenario(
        "ollama-source",
        &format!("{BASE}ollama.model = \"my-model\"\n"),
    );
    s.steps = vec![
        set("alpha original statement", &[]),
        set("alpha near statement", &[]),
    ];
    s.embed = Some(embedder());
    let compared = compare(&s);
    assert!(has(&compared.db, "my-model"));
}

#[test]
fn binary_quantization_stores_sign_bits() {
    let config = format!("{BASE}[hybrid_search]\nquantize_to_binary = true\n");
    let mut s = scenario("quantize", &config);
    s.steps = vec![set("alpha original statement", &[])];
    compare(&s);
}
