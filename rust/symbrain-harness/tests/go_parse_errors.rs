//! Replays Go-observed `harness list` parse results (#769 slice 2, item D1).
//!
//! `tests/fixtures/go_parse_errors.json` is recorded from the pinned Go oracle
//! by `scripts/config-harness-oracle/run.sh --record-parse-fixture`; each
//! case is one harness config and the `ConfigInventory` Go printed for it.
#![cfg(unix)]

use std::path::Path;

use serde_json::Value;
use symbrain_harness::{list_for_env, lookup};

fn write_config(home: &Path, harness: &str, content: &[u8]) {
    let env = vec![
        ("HOME".to_owned(), home.to_string_lossy().into_owned()),
        (
            "XDG_CONFIG_HOME".to_owned(),
            home.join("xdg").to_string_lossy().into_owned(),
        ),
    ];
    let path = lookup(harness)
        .unwrap()
        .config_path_for("linux", &env)
        .unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn decode_hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|index| u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).unwrap())
        .collect()
}

fn inventory_for(home: &Path, harness: &str) -> Value {
    let env = vec![
        ("HOME".to_owned(), home.to_string_lossy().into_owned()),
        (
            "XDG_CONFIG_HOME".to_owned(),
            home.join("xdg").to_string_lossy().into_owned(),
        ),
    ];
    let inventory = list_for_env(None, "linux", &env);
    let entry = inventory
        .harnesses
        .iter()
        .find(|item| item.name.as_str() == harness)
        .unwrap();
    let mut value = serde_json::to_value(&entry.global).unwrap();
    value.as_object_mut().unwrap().remove("path");
    value
}

#[test]
fn go_recorded_parse_results_are_reproduced_without_a_go_binary() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/go_parse_errors.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.len() >= 200, "fixture shrank to {}", cases.len());
    let mut errors = 0;
    for case in cases {
        let harness = case["harness"].as_str().unwrap();
        let home = tempfile::tempdir().unwrap();
        write_config(
            home.path(),
            harness,
            &decode_hex(case["content_hex"].as_str().unwrap()),
        );
        let actual = inventory_for(home.path(), harness);
        assert_eq!(
            actual, case["expected"],
            "{} ({})",
            case["name"], case["family"]
        );
        errors += usize::from(actual.get("error").is_some());
    }
    assert!(
        errors >= 150,
        "fixture must contain parse errors, saw {errors}"
    );
}

#[test]
fn deeply_nested_json_is_parsed_and_dropped_without_overflowing_the_stack() {
    // Go's decoder has no nesting limit below its 8 MiB input cap.
    let depth = 200_000;
    let mut content = b"{\"a\":".to_vec();
    content.extend(std::iter::repeat_n(b'[', depth));
    content.extend(std::iter::repeat_n(b']', depth));
    content.push(b'}');
    let home = tempfile::tempdir().unwrap();
    write_config(home.path(), "claude", &content);
    let global = inventory_for(home.path(), "claude");
    assert_eq!(global["parsed"], true);
    assert!(global.get("error").is_none());

    let mut unterminated = b"{\"a\":".to_vec();
    unterminated.extend(std::iter::repeat_n(b'[', depth));
    write_config(home.path(), "claude", &unterminated);
    let global = inventory_for(home.path(), "claude");
    assert_eq!(
        global["error"],
        "harness: claude config is not valid json; refusing to edit a config symbrain cannot parse: parse json: EOF"
    );
}

#[test]
fn parse_error_messages_use_go_wording() {
    let cases: [(&str, &[u8], &str); 8] = [
        (
            "claude",
            b"{\"a\":}",
            "invalid character '}' looking for beginning of value",
        ),
        (
            "claude",
            b"{\"a\":1,}",
            "invalid character '}' looking for beginning of object key string",
        ),
        ("claude", b"{x", "invalid character 'x'"),
        ("claude", b"[1]", "expected {, got ["),
        (
            "claude",
            b"{} x",
            "unexpected trailing content after the top-level value",
        ),
        ("claude", b"{\"a\":tru", "unexpected EOF"),
        (
            "codex",
            b"a = x",
            "toml: line 1 (last key \"a\"): expected value but found \"x\" instead",
        ),
        (
            "codex",
            b"[a]\n[a]",
            "toml: line 2: Key 'a' has already been defined.",
        ),
    ];
    for (harness, content, detail) in cases {
        let home = tempfile::tempdir().unwrap();
        write_config(home.path(), harness, content);
        let format = if harness == "codex" { "toml" } else { "json" };
        assert_eq!(
            inventory_for(home.path(), harness)["error"],
            format!(
                "harness: {harness} config is not valid {format}; refusing to edit a config symbrain cannot parse: parse {format}: {detail}"
            )
        );
    }
}
