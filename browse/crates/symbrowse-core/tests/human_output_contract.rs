#![deny(unsafe_code)]

//! OUT-003: dedicated human renderers and the yaml.v3 layout of `--output
//! yaml`. The expectations are the Go unit tests of `internal/output`
//! (`human_renderers_test.go`, `TestWriteHumanEnvelopeVariants`; Go sources at
//! 57c26c76e) plus layout rules read from `gopkg.in/yaml.v3@v3.0.1`
//! (`emitterc.go`, `sorter.go`). The byte-for-byte observations of the real Go
//! binary for every command renderer live in
//! `symbrowse-cli/tests/fixtures/cli/cli_oracle.json` (`human-*` scenarios).

use serde_json::{Value, json};
use symbrowse_core::{
    error::ErrorCode,
    output::{Envelope, Format},
};

fn text(data: Value) -> String {
    Envelope::ok(data, Vec::new())
        .render(Format::Text)
        .expect("render text")
}

fn yaml(data: Value) -> String {
    Envelope::ok(data, Vec::new())
        .render(Format::Yaml)
        .expect("render yaml")
}

#[test]
fn navigation_payload_is_one_line() {
    assert_eq!(
        text(json!({"action": "open", "url": "https://example.com/", "http_status": 200})),
        "open https://example.com/ (HTTP 200)\n"
    );
    assert_eq!(
        text(json!({"action": "back", "url": "https://example.com/", "http_status": 0})),
        "back https://example.com/\n"
    );
}

#[test]
fn snapshot_payload_prints_the_tree_not_go_map_syntax() {
    let got = text(json!({
        "snapshot_id": "snap-1",
        "tree": "- document \"Example Domain\" [ref=e1]\n  - heading \"Example Domain\" [ref=e2]",
        "refs": {"e1": {"role": "document"}},
    }));
    assert_eq!(
        got,
        "snap-1 tree:\n- document \"Example Domain\" [ref=e1]\n  - heading \"Example Domain\" [ref=e2]\n"
    );
    assert!(!got.contains("map["));
}

#[test]
fn state_payload_lists_metadata_without_go_map_syntax() {
    let got = text(json!({
        "saved": "demo",
        "metadata": {
            "name": "demo",
            "schema_version": 3,
            "key_source": "environment",
            "origins": [{
                "origin": "https://example.com",
                "cookie_count": 1,
                "local_storage_keys": 2,
                "session_storage_keys": 0,
            }],
        },
    }));
    for want in [
        "saved: demo\n",
        "schema version: 3\n",
        "key source: environment\n",
        "https://example.com (cookies=1, local=2, session=0)\n",
    ] {
        assert!(got.contains(want), "{got:?} lacks {want:?}");
    }
    assert!(!got.contains("map["));
}

#[test]
fn cookie_payload_never_prints_values() {
    let got = text(json!({
        "origin": "https://example.com",
        "cookies": [{
            "name": "session",
            "value": "s3cret",
            "domain": ".example.com",
            "path": "/",
            "secure": true,
            "http_only": true,
        }],
    }));
    assert_eq!(
        got,
        "origin: https://example.com\ncookies:\n- session domain=.example.com path=/ secure httpOnly\n"
    );
    assert!(!got.contains("s3cret"));
}

#[test]
fn tabs_payload_marks_the_active_tab() {
    assert_eq!(
        text(json!({
            "active": "t1",
            "tabs": [
                {"id": "t1", "label": "research", "url": "https://example.com", "active": true},
                {"id": "t2", "url": "about:blank", "active": false},
            ],
        })),
        "active: t1\ntabs:\n- t1 \"research\" https://example.com [active]\n- t2 about:blank\n"
    );
}

#[test]
fn unknown_payload_uses_key_sorted_indented_json() {
    assert_eq!(
        text(json!({"z": "last", "a": 1})),
        "{\n  \"a\": 1,\n  \"z\": \"last\"\n}\n"
    );
}

#[test]
fn envelope_variants() {
    let ok = |data: Value| text(data);
    assert_eq!(
        Envelope {
            success: false,
            data: Value::Null,
            warnings: Vec::new(),
            error: None,
        }
        .render(Format::Text)
        .unwrap(),
        "error\n"
    );
    assert_eq!(
        Envelope::failure(ErrorCode::Internal, "something failed")
            .render(Format::Text)
            .unwrap(),
        "something failed\n"
    );
    assert_eq!(ok(Value::Null), "ok\n");
    assert_eq!(ok(json!("hello")), "hello\n");
    assert_eq!(ok(json!(42)), "42\n");
    assert_eq!(
        ok(json!({
            "truncated": true,
            "tokens_returned": 90,
            "tokens_total": 18400,
            "cache_id": "out_abc",
            "hint": "symbrowse cache get out_abc --range 40-120",
            "head": "HEAD TEXT",
            "foot": "FOOT TEXT",
        })),
        "HEAD TEXT\n\n… [truncated: 90 of 18400 tokens] …\n\nFOOT TEXT\n\nfull output: symbrowse cache get out_abc --range 40-120\n"
    );
    assert_eq!(
        ok(json!({"truncated": "yes"})),
        "{\n  \"truncated\": \"yes\"\n}\n"
    );
}

#[test]
fn yaml_is_four_column_indented_with_trailing_newline() {
    assert_eq!(
        yaml(json!({"a": {"b": [1, 2]}, "c": "x"})),
        "success: true\ndata:\n    a:\n        b:\n            - 1\n            - 2\n    c: x\nwarnings: []\nerror: null\n"
    );
}

#[test]
fn yaml_sequence_items_follow_the_yaml_v3_column_rules() {
    // A mapping in a sequence item starts two columns right of the dash; a
    // sequence below such a key moves to the next 4-column stop.
    assert_eq!(
        yaml(json!({"l": [{"a": 1, "b": [2]}, [3, [4]], {}, []]})),
        "success: true\ndata:\n    l:\n        - a: 1\n          b:\n            - 2\n        - - 3\n          - - 4\n        - {}\n        - []\nwarnings: []\nerror: null\n"
    );
}

#[test]
fn yaml_keys_use_the_natural_order_of_yaml_v3() {
    // Digit runs compare numerically; a letter sorts after a non-letter.
    assert_eq!(
        yaml(json!({"e10": 1, "e2": 2, "a_b": 3, "aB": 4, "Z": 5})),
        "success: true\ndata:\n    Z: 5\n    a_b: 3\n    aB: 4\n    e2: 2\n    e10: 1\nwarnings: []\nerror: null\n"
    );
}

#[test]
fn yaml_multiline_strings_are_literal_blocks_with_chomping_hints() {
    assert_eq!(
        yaml(json!({"a": "x\ny", "b": "x\n", "c": "x\n\n", "d": "\nx"})),
        "success: true\ndata:\n    a: |-\n        x\n        y\n    b: |\n        x\n    c: |+\n        x\n\n    d: |4-\n        x\nwarnings: []\nerror: null\n"
    );
}

#[test]
fn yaml_scalar_style_matches_the_emitter_analysis() {
    let got = yaml(json!({
        "tab": "a\tb",
        "emoji": "x\u{1F600}",
        "yes": "yes",
        "num": "123",
        "empty": "",
        "colon": "a: b",
        "bom": "\u{feff}a",
    }));
    assert_eq!(
        got,
        "success: true\ndata:\n    bom: \"\\uFEFF\\x61\"\n    colon: 'a: b'\n    emoji: \"x\\U0001F600\"\n    empty: \"\"\n    num: \"123\"\n    tab: \"a\\tb\"\n    \"yes\": \"yes\"\nwarnings: []\nerror: null\n"
    );
}
