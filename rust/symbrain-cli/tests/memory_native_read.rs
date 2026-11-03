//! Read commands (`search`, `list`, `rules`, `delete`) against the Go oracle
//! for the configurations and database states that used to stay on Go (#758).

#![cfg(unix)]

#[path = "support/memory_native_oracle.rs"]
mod oracle;

use oracle::{FakeReply, RecordedRequest, Scenario, argv, compare};
use std::ffi::OsString;
use std::sync::Arc;

const BIN: &str = env!("CARGO_BIN_EXE_symbrain");

/// `doc<N>` texts map to e0 with N positive side components, so the Hamming
/// distance to another document is |N - M| while every document stays in the
/// same LSH bucket.
fn vector_for(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0_f32; 768];
    vector[0] = 1.0;
    let number = text
        .split(|c: char| !c.is_ascii_digit())
        .find(|part| !part.is_empty())
        .and_then(|part| part.parse::<usize>().ok())
        .unwrap_or(0);
    for slot in vector.iter_mut().skip(1).take(number) {
        *slot = 0.001;
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
    scenario.config = config.to_owned();
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
