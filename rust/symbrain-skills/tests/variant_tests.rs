use std::collections::BTreeMap;
use std::fs;

use serde_json::Value;
use symbrain_skills::variant::{
    CODE_BLOCK_CLOSE_MISMATCH, CODE_BLOCK_DUPLICATE_ID, CODE_BLOCK_ID_INVALID, CODE_BLOCK_NESTED,
    CODE_BLOCK_UNCLOSED, CODE_BLOCK_UNMATCHED_CLOSE, CODE_MARKER_MALFORMED, CODE_TARGET_LIST_EMPTY,
    CODE_TERM_DEFAULT_REQUIRED, CODE_TERM_NAME_INVALID, CODE_TERM_UNKNOWN, DEFAULT_KEY, Kind,
    Options, Problem, Region, apply, check_overrides, check_region_targets, check_terms,
    find_mentions, scan_text, unscoped_text,
};

fn options(target: &str) -> Options {
    Options {
        target: target.into(),
        overrides: BTreeMap::new(),
        terms: BTreeMap::new(),
    }
}

fn has(problems: &[symbrain_skills::variant::Problem], code: &str) -> bool {
    problems.iter().any(|problem| problem.code == code)
}

#[test]
fn variant_fixture_matches_each_target_resolution() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../internal/skills/render/testdata/variant-source");
    let bundle = symbrain_skills::load_bundle(&root).expect("variant fixture");
    let targets = [
        "antigravity",
        "claude",
        "codex",
        "hermes",
        "openclaw",
        "opencode",
    ];
    for target in targets {
        let mut opts = options(target);
        opts.terms = bundle.manifest.terms.clone();
        opts.overrides = bundle
            .block_overrides
            .get(target)
            .cloned()
            .unwrap_or_default();
        let (result, problems) = apply(&bundle.body, &opts);
        assert!(problems.is_empty(), "{target}: {problems:?}");
        assert!(
            !result.text.contains("symskills:"),
            "{target}: marker leaked"
        );
        assert!(!result.text.contains("{{term:"), "{target}: term leaked");
    }
    let (hermes, _) = apply(
        &bundle.body,
        &Options {
            target: "hermes".into(),
            overrides: bundle.block_overrides["hermes"].clone(),
            terms: bundle.manifest.terms.clone(),
        },
    );
    assert!(hermes.text.contains("~/.hermes/reports/variant-fixture/"));
    assert!(hermes.text.contains("Provider-backed children are allowed"));
    assert!(!hermes.text.contains("Use whatever isolation mechanism"));
    let (claude, _) = apply(
        &bundle.body,
        &Options {
            target: "claude".into(),
            overrides: bundle.block_overrides["claude"].clone(),
            terms: bundle.manifest.terms.clone(),
        },
    );
    assert!(
        claude
            .text
            .contains("Dispatch each group with the Agent tool")
    );
    assert!(!claude.text.contains("Provider-backed children"));
}

#[test]
fn apply_is_identity_without_markers_and_strips_markers_with_noop_regions() {
    let plain = "# Title\n\nAn ordinary <!-- comment --> and {{not_a_term}}.\n";
    let (result, problems) = apply(plain, &options("claude"));
    assert!(problems.is_empty());
    assert_eq!(result.text, plain);
    let marked =
        "before\n<!-- symskills:block worker -->\ncanonical\n<!-- /symskills:block -->\nafter\n";
    let (result, problems) = apply(marked, &options("claude"));
    assert!(problems.is_empty());
    assert_eq!(result.text, "before\ncanonical\nafter\n");
}

#[test]
fn apply_handles_blocks_scopes_and_terms() {
    let source = "head\n<!-- symskills:block worker -->\ncanonical\n<!-- /symskills:block -->\n<!-- symskills:only hermes, codex -->\nscoped\n<!-- /symskills:only -->\n<!-- symskills:except hermes -->\nother\n<!-- /symskills:except -->\n{{term:report_dir}}\n";
    let mut opts = options("hermes");
    opts.overrides
        .insert("worker".into(), "replacement\n".into());
    opts.terms.insert(
        "report_dir".into(),
        [
            (DEFAULT_KEY.into(), "default".into()),
            ("hermes".into(), "hermes-value".into()),
        ]
        .into_iter()
        .collect(),
    );
    let (result, problems) = apply(source, &opts);
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(result.text, "head\nreplacement\nscoped\nhermes-value\n");
    assert_eq!(result.blocks, ["worker"]);
    assert_eq!(result.terms["report_dir"], "hermes-value");
}

#[test]
fn malformed_markers_are_reported_and_unresolvable_terms_remain() {
    let cases = [
        ("<!-- symskills:block a -->\ntext\n", CODE_BLOCK_UNCLOSED),
        (
            "text\n<!-- /symskills:block -->\n",
            CODE_BLOCK_UNMATCHED_CLOSE,
        ),
        (
            "<!-- symskills:block a -->\n<!-- symskills:block b -->\nx\n<!-- /symskills:block -->\n<!-- /symskills:block -->\n",
            CODE_BLOCK_NESTED,
        ),
        (
            "<!-- symskills:block a -->\nx\n<!-- /symskills:block -->\n<!-- symskills:block a -->\ny\n<!-- /symskills:block -->\n",
            CODE_BLOCK_DUPLICATE_ID,
        ),
        (
            "<!-- symskills:only hermes -->\nx\n<!-- /symskills:block -->\n",
            CODE_BLOCK_CLOSE_MISMATCH,
        ),
        (
            "<!-- symskills:block Bad_ID -->\nx\n<!-- /symskills:block -->\n",
            CODE_BLOCK_ID_INVALID,
        ),
        ("<!-- symskills:blok a -->\n", CODE_MARKER_MALFORMED),
        (
            "<!-- symskills:only , -->\nx\n<!-- /symskills:only -->\n",
            CODE_TARGET_LIST_EMPTY,
        ),
    ];
    for (source, expected) in cases {
        let (_, problems) = scan_text(source);
        assert!(has(&problems, expected), "{expected}: {problems:?}");
    }
    let mut opts = options("claude");
    opts.terms.insert(
        "no_default".into(),
        [("hermes".into(), "value".into())].into_iter().collect(),
    );
    for (source, expected) in [
        ("{{term:missing}}\n", CODE_TERM_UNKNOWN),
        ("{{term:no_default}}\n", CODE_TERM_DEFAULT_REQUIRED),
        ("{{term:Bad Name}}\n", CODE_TERM_NAME_INVALID),
    ] {
        let (result, problems) = apply(source, &opts);
        assert!(has(&problems, expected), "{expected}: {problems:?}");
        assert_eq!(result.text, source);
    }
}

#[test]
fn unscoped_mentions_ignore_scoped_and_fenced_text() {
    let source = "outside hermes\n<!-- symskills:only hermes -->\ninside claude\n<!-- /symskills:only -->\n```bash\nclaude command\n```\n";
    let text = unscoped_text(source);
    assert!(text.contains("outside hermes"));
    assert!(!text.contains("inside claude"));
    assert!(!text.contains("claude command"));
    let mentions = find_mentions(source, &["claude".into(), "hermes".into()]);
    assert_eq!(
        mentions
            .iter()
            .map(|mention| mention.name.as_str())
            .collect::<Vec<_>>(),
        ["hermes"]
    );
}

#[test]
fn marker_grammar_matches_go_case_and_separator_rules() {
    let whitespace =
        "  <!--   symskills:  only hermes, codex   -->  \ntext\n  <!-- /symskills:only -->  \n";
    let (_, problems) = scan_text(whitespace);
    assert!(problems.is_empty(), "{problems:?}");
    let (_, problems) = scan_text("<!-- Symskills:block worker -->\n");
    assert!(
        problems.is_empty(),
        "uppercase marker is literal: {problems:?}"
    );
    let (_, problems) = scan_text("<!-- symskills:blocker -->\n");
    assert!(has(&problems, CODE_MARKER_MALFORMED), "{problems:?}");
    let (_, problems) = apply("{{term:a-_b}} {{term:a-_b}}\n", &options("hermes"));
    assert_eq!(
        problems
            .iter()
            .filter(|problem| problem.code == CODE_TERM_NAME_INVALID)
            .count(),
        1
    );
    let (_, problems) = apply("{{term:a-b_c}}\n", &options("hermes"));
    assert!(!has(&problems, CODE_TERM_NAME_INVALID), "{problems:?}");
}

#[test]
fn diagnostic_messages_match_go_oracle() {
    let oracle_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oracle_expectations.json");
    let oracle: Value =
        serde_json::from_slice(&fs::read(oracle_path).expect("oracle JSON")).expect("oracle JSON");
    let known = ["claude".to_string(), "hermes".to_string()];
    for case in oracle["diagnostics"].as_array().expect("diagnostics") {
        let id = case["id"].as_str().expect("diagnostic id");
        let problems: Vec<Problem> = match id {
            "unknown_region_target" => check_region_targets(
                &[Region {
                    kind: Kind::Only,
                    id: String::new(),
                    targets: vec!["hermez".into()],
                    line: 4,
                }],
                &known,
            ),
            "unknown_override" => check_overrides(
                &["worker".into()],
                &[("claude".into(), vec!["invented".into()])]
                    .into_iter()
                    .collect(),
            ),
            "term_without_default" => check_terms(
                &[(
                    "report_dir".into(),
                    [("hermes".into(), "value".into())].into_iter().collect(),
                )]
                .into_iter()
                .collect(),
                &known,
            ),
            "term_unknown_target" => check_terms(
                &[(
                    "report_dir".into(),
                    [
                        (DEFAULT_KEY.into(), "value".into()),
                        ("hermez".into(), "value".into()),
                    ]
                    .into_iter()
                    .collect(),
                )]
                .into_iter()
                .collect(),
                &known,
            ),
            "invalid_term_reference" => apply("{{term:Bad Name}}\n", &options("hermes")).1,
            "unknown_term_reference" => apply("{{term:missing}}\n", &options("hermes")).1,
            "missing_term_default" => {
                let mut opts = options("hermes");
                opts.terms.insert(
                    "no_default".into(),
                    [("claude".into(), "value".into())].into_iter().collect(),
                );
                apply("{{term:no_default}}\n", &opts).1
            }
            other => panic!("unknown diagnostic fixture {other}"),
        };
        let actual = problems
            .iter()
            .map(|problem| {
                serde_json::json!({
                    "code": problem.code,
                    "severity": problem.severity,
                    "message": problem.message,
                    "line": problem.line,
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            Value::Array(actual),
            case["problems"],
            "diagnostic drift for {id}"
        );
    }
}

#[test]
fn variant_outputs_match_go_target_goldens() {
    let oracle_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/oracle_expectations.json");
    let oracle: Value = serde_json::from_slice(&fs::read(oracle_path).expect("oracle fixture"))
        .expect("oracle JSON");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../internal/skills/render/testdata/variant-source");
    let bundle = symbrain_skills::load_bundle(&root).expect("variant fixture");
    for case in oracle["variant_cases"].as_array().expect("variant cases") {
        let target = case["target"].as_str().expect("target");
        let mut opts = options(target);
        opts.overrides = bundle
            .block_overrides
            .get(target)
            .cloned()
            .unwrap_or_default();
        opts.terms = bundle.manifest.terms.clone();
        let (body, issues) = apply(&bundle.body, &opts);
        assert!(issues.is_empty(), "Go issues for {target}");
        assert_eq!(body.text, case["body"].as_str().expect("golden body"));
        let expected_files = case["files"].as_object().expect("golden files");
        assert_eq!(expected_files.len(), bundle.markdown.len());
        for (path, source) in &bundle.markdown {
            let source = String::from_utf8_lossy(source);
            let (resolved, file_issues) = apply(&source, &opts);
            assert!(file_issues.is_empty(), "Go issues for {target}/{path}");
            assert_eq!(
                resolved.text,
                expected_files[path].as_str().expect("golden file")
            );
        }
    }
}
