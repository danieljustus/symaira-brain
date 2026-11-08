//! Pre-storage fact processing of the registry (`sanitizeImportedFacts` and
//! `extractFacts` from `registry.go`).

use super::ImportedFact;
use crate::evidence::SourceRef;
use crate::extract;
use crate::pii::sanitize_untrusted;
use crate::summarizer::summarize_session;
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// `sanitizeImportedFacts`.
pub(super) fn sanitize_facts(facts: &mut [ImportedFact]) {
    let clean =
        |text: &str| String::from_utf8_lossy(&sanitize_untrusted(text.as_bytes())).into_owned();
    for fact in facts {
        fact.content = clean(&fact.content);
        fact.metadata
            .insert("source_untrusted".into(), "true".into());
        for e in &mut fact.evidence {
            e.text = clean(&e.text);
            e.evidence_text = clean(&e.evidence_text);
            if e.alignment_status == "exact" && !e.text.is_empty() {
                e.span.start = 0;
                e.span.end = i64::try_from(e.text.len()).unwrap_or(i64::MAX);
            }
        }
    }
}

/// `extractFacts`: pattern facts plus an extractive summary of the batch, or
/// the original facts when neither produced anything.
pub(super) fn extract_facts(facts: Vec<ImportedFact>) -> Vec<ImportedFact> {
    let Some(first) = facts.first().cloned() else {
        return facts;
    };
    let mut combined = String::new();
    for fact in facts.iter().filter(|f| !f.content.is_empty()) {
        let _ = writeln!(combined, "{}", fact.content);
    }
    let combined = combined.trim();
    if combined.is_empty() {
        return facts;
    }
    let extracted = extract::extract_facts(combined.as_bytes());
    let summary = summarize_session(combined, 5);
    if extracted.is_empty() && summary.is_empty() {
        return facts;
    }
    let merge = |extra: BTreeMap<String, String>| {
        let mut merged = first.metadata.clone();
        merged.extend(extra);
        merged
    };
    let mut distilled: Vec<ImportedFact> = extracted
        .into_iter()
        .map(|fact| ImportedFact {
            content: fact.content,
            source: first.source.clone(),
            session_id: first.session_id.clone(),
            timestamp: first.timestamp,
            metadata: merge(fact.metadata),
            evidence: fact
                .evidence
                .into_iter()
                .map(|mut e| {
                    e.source = SourceRef {
                        id: first.session_id.clone(),
                        kind: first.source.clone(),
                    };
                    e
                })
                .collect(),
        })
        .collect();
    if !summary.is_empty() {
        let extra = BTreeMap::from([
            ("method".to_owned(), "extractive_summarization".to_owned()),
            ("fact_count".to_owned(), facts.len().to_string()),
            ("distilled_to".to_owned(), distilled.len().to_string()),
        ]);
        distilled.push(ImportedFact {
            content: summary,
            source: first.source.clone(),
            session_id: first.session_id.clone(),
            timestamp: first.timestamp,
            metadata: merge(extra),
            evidence: Vec::new(),
        });
    }
    distilled
}
