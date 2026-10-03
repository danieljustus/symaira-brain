//! Executed Go evidencekit v0.17.0 contracts, not Rust-generated expectations.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use symbrain_memory::evidence::{self, Extraction, Span};

#[derive(Deserialize)]
struct Alignment {
    source: String,
    evidence: String,
    span: Span,
    status: String,
    exact: Option<Span>,
    normalized: Option<Span>,
    fuzzy: Option<Span>,
}

#[derive(Deserialize)]
struct Validation {
    extraction: Extraction,
    strict: String,
    fuzzy: String,
}

#[derive(Deserialize)]
struct Oracle {
    alignments: Vec<Alignment>,
    validations: Vec<Validation>,
    extractions: Vec<Extraction>,
    jsonl: String,
    jsonl_sha256: String,
    decoded: Vec<Extraction>,
}

fn oracle() -> Oracle {
    serde_json::from_str(include_str!("fixtures/memory_evidence_go_v017.json")).unwrap()
}

#[test]
fn all_alignment_strategies_offsets_and_ties_match_executed_go() {
    let oracle = oracle();
    assert_eq!(oracle.alignments.len(), 32, "a missing case must fail");
    for case in oracle.alignments {
        let label = format!("source={:?}, evidence={:?}", case.source, case.evidence);
        let (span, status) = evidence::align(&case.source, &case.evidence);
        assert_eq!(span, case.span, "{label}");
        assert_eq!(status, case.status, "{label}");
        assert_eq!(
            evidence::align_exact(&case.source, &case.evidence),
            case.exact,
            "{label}"
        );
        assert_eq!(
            evidence::align_normalized(&case.source, &case.evidence),
            case.normalized,
            "{label}"
        );
        assert_eq!(
            evidence::align_fuzzy(&case.source, &case.evidence),
            case.fuzzy,
            "{label}"
        );
    }
}

#[test]
fn every_validation_boundary_and_error_priority_matches_go() {
    let oracle = oracle();
    assert_eq!(oracle.validations.len(), 48);
    for case in oracle.validations {
        let error = case
            .extraction
            .validate()
            .err()
            .map_or_else(String::new, |error| error.to_string());
        assert_eq!(error, case.strict, "{:?}", case.extraction);
        let error = case
            .extraction
            .validate_with_options(true)
            .err()
            .map_or_else(String::new, |error| error.to_string());
        assert_eq!(error, case.fuzzy, "{:?}", case.extraction);
    }
}

#[test]
fn jsonl_bytes_hash_and_go_roundtrip_match() {
    let oracle = oracle();
    assert_eq!(oracle.extractions.len(), 3);
    assert_eq!(oracle.extractions, oracle.decoded);
    let mut encoded = Vec::new();
    evidence::encode_jsonl(&mut encoded, &oracle.extractions).unwrap();
    assert_eq!(encoded, oracle.jsonl.as_bytes());
    assert_eq!(
        format!("{:x}", Sha256::digest(&encoded)),
        oracle.jsonl_sha256
    );
}

#[test]
fn jsonl_writer_failure_propagates_without_faking_success() {
    struct Broken;
    impl std::io::Write for Broken {
        fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "closed",
            ))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let error = evidence::encode_jsonl(&mut Broken, &[Extraction::default()]).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
}
