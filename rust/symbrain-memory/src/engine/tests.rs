//! Prepared algorithm assertions. These are not executed oracle evidence.

use chrono::{DateTime, Utc};

use super::{
    AgingConfig, BudgetPiece, PatternExtractor, decay_factor, enforce_budget, estimate_tokens,
    summarize_session,
};

fn time(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw)
        .expect("fixed timestamp")
        .with_timezone(&Utc)
}

#[test]
fn extracted_facts_reuse_original_source_byte_spans_and_distinct_order() {
    let text = "Änderung. I like TypeScript. I like TypeScript! Ich mag golang.";
    let facts = PatternExtractor::new().expect("fixed rules").extract(text);
    assert_eq!(facts.len(), 2);
    assert_eq!(facts[0].content, "User prefers TypeScript");
    assert_eq!(facts[1].content, "User prefers golang");
    for fact in facts {
        let extraction = &fact.evidence[0];
        extraction.validate().expect("strict grounded evidence");
        let span = extraction.span;
        let start = usize::try_from(span.start).expect("nonnegative span");
        let end = usize::try_from(span.end).expect("nonnegative span");
        assert_eq!(&text[start..end], extraction.evidence_text);
        assert!(extraction.source.id.is_empty());
    }
}

#[test]
fn go_regex_whitespace_does_not_admit_nonbreaking_space_as_a_trigger() {
    let extractor = PatternExtractor::new().expect("rules");
    assert!(extractor.extract("I\u{a0}like coffee").is_empty());
    assert_eq!(
        extractor.extract("I\tlike coffee")[0].content,
        "User prefers coffee"
    );
}

#[test]
fn every_matching_rule_can_yield_a_fact_from_the_same_sentence() {
    let facts = PatternExtractor::new()
        .expect("rules")
        .extract("I like Rust and I use SQLite");
    assert_eq!(
        facts
            .iter()
            .map(|fact| fact.category.as_str())
            .collect::<Vec<_>>(),
        ["preference", "identity"]
    );
}

#[test]
fn summary_retains_non_ascii_first_byte_contract() {
    let mut expected = b"Session Context Summary:\n- ".to_vec();
    expected.extend_from_slice("Ã".as_bytes());
    expected.extend_from_slice(&"écho sentence long enough".as_bytes()[1..]);
    expected.push(b'\n');
    assert_eq!(
        summarize_session("écho sentence long enough".as_bytes(), 1),
        expected
    );
    assert!(std::str::from_utf8(&expected).is_err());
}

#[test]
fn summary_zero_limit_preserves_header_but_empty_session_does_not() {
    assert_eq!(
        summarize_session(b"User: project sentence long enough", 0),
        b"Session Context Summary:\n"
    );
    assert!(summarize_session(b"\t\n", 5).is_empty());
}

#[test]
fn budget_keeps_two_positional_pieces_and_reports_original_fit() {
    let mut pieces = vec![
        BudgetPiece {
            layer: b"retrieval".to_vec(),
            tokens: 30,
        },
        BudgetPiece {
            layer: b"summary".to_vec(),
            tokens: 30,
        },
        BudgetPiece {
            layer: vec![0xff],
            tokens: 20,
        },
    ];
    let report = enforce_budget(&mut pieces, 10).expect("report");
    assert_eq!(pieces.len(), 2);
    assert_eq!(report.estimated_tokens, 60);
    assert!(!report.fit);
    assert_eq!(report.dropped_ids, [vec![0xff]]);
}

#[test]
fn budget_nonpositive_limits_and_malformed_rune_count_are_explicit() {
    let mut pieces = vec![BudgetPiece {
        layer: b"working".to_vec(),
        tokens: 20,
    }];
    let original = pieces.clone();
    assert!(enforce_budget(&mut pieces, 0).is_none());
    assert_eq!(pieces, original);
    // Go consumes each truncated UTF-8 byte as an individual RuneError.
    assert_eq!(estimate_tokens(&[0xf0, 0x90, 0x80, 0x80, b'\n']), 2);
    assert_eq!(estimate_tokens(&[0xf0, 0x90, 0x80, b'\n']), 3);
}

#[test]
fn aging_uses_injected_clock_and_does_not_boost_legacy_access_count() {
    let created = time("2025-01-01T00:00:00Z");
    let now = time("2025-05-01T00:00:00Z");
    let config = AgingConfig::default();
    assert_eq!(
        decay_factor(config, created, None, 1, now).to_bits(),
        decay_factor(config, created, None, 500, now).to_bits()
    );
    assert_eq!(
        decay_factor(
            config,
            created,
            Some(time("0001-01-01T00:00:00Z")),
            500,
            now
        )
        .to_bits(),
        decay_factor(config, created, None, 0, now).to_bits()
    );
    assert!(decay_factor(config, created, Some(created), 20, now) > 0.74);
}

#[test]
fn disabled_aging_and_future_times_do_not_decay() {
    let created = time("2025-01-01T00:00:00Z");
    let earlier = time("2024-01-01T00:00:00Z");
    assert_eq!(
        decay_factor(AgingConfig::default(), created, None, 0, earlier).to_bits(),
        1.0_f64.to_bits()
    );
    assert_eq!(
        decay_factor(
            AgingConfig {
                enabled: false,
                ..AgingConfig::default()
            },
            earlier,
            None,
            0,
            created
        )
        .to_bits(),
        1.0_f64.to_bits()
    );
}

#[test]
fn invalid_access_count_preserves_nan_instead_of_silent_retirement() {
    let now = time("2025-01-01T00:00:00Z");
    assert!(decay_factor(AgingConfig::default(), now, Some(now), -2, now).is_nan());
}
