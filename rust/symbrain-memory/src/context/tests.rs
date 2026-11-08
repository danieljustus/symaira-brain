//! Assembler behavior with an in-memory source and an injected clock (#760).

use super::source::ContextSource;
use super::*;
use crate::consolidation::hash_embed;
use crate::model::StoreError;
use chrono::{Duration, TimeZone};
use std::cell::RefCell;

pub(super) fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 6, 1, 12, 0, 0).unwrap()
}

pub(super) fn recalled(id: &str, content: &str, kind: &str, score: f32) -> Recalled {
    Recalled {
        id: id.into(),
        content: content.into(),
        scope: "project".into(),
        kind: kind.into(),
        tier: "long_term".into(),
        created_at: t0() - Duration::days(3),
        score,
    }
}

#[derive(Default)]
pub(super) struct Fake {
    pub(super) summary: RefCell<String>,
    pub(super) working: Vec<Recalled>,
    pub(super) vector: Vec<Recalled>,
    pub(super) keyword: Vec<Recalled>,
}

impl ContextSource for Fake {
    fn session_summary(&self, _: &str) -> Result<String, StoreError> {
        Ok(self.summary.borrow().clone())
    }
    fn save_session_summary(&self, _: &str, summary: &str) -> Result<(), StoreError> {
        summary.clone_into(&mut self.summary.borrow_mut());
        Ok(())
    }
    fn working_memories(&self, limit: usize) -> Result<Vec<Recalled>, StoreError> {
        Ok(self.working.iter().take(limit).cloned().collect())
    }
    fn keyword_search(&self, _: &str, limit: usize) -> Result<Vec<Recalled>, StoreError> {
        Ok(self.keyword.iter().take(limit).cloned().collect())
    }
    fn vector_search(&self, _: &[f32], _: &str, limit: usize) -> Result<Vec<Recalled>, StoreError> {
        Ok(self.vector.iter().take(limit).cloned().collect())
    }
}

pub(super) fn embedder(text: &str) -> (Vec<f32>, String) {
    let (vector, source, _) = hash_embed(text);
    (vector, source)
}

#[test]
fn token_estimate_table() {
    for (text, want) in [
        ("", 0),
        ("abcd", 2),
        ("abcdefgh", 3),
        ("a\nb\nc", 4),
        ("日本語のテキスト", 3),
    ] {
        assert_eq!(default_token_estimate(text), want, "{text:?}");
    }
}

#[test]
fn layers_arrive_in_priority_order_and_working_context_is_trimmed() {
    let now = || t0();
    let fake = Fake {
        working: vec![recalled(
            "w1",
            "keep the daemon on port 8787",
            "project",
            0.0,
        )],
        vector: vec![recalled("r1", "tea is preferred", "user", 0.9)],
        ..Fake::default()
    };
    fake.save_session_summary("s", "- earlier summary").unwrap();
    let assembler = Assembler::new(Some(&fake), ContextConfig::default(), &now)
        .with_embedder(embedder)
        .with_working_memory(WorkingMemoryConfig {
            max_items: 0,
            include_in_context: true,
        })
        .with_degradation(DegradationConfig::default());
    let session: String = (1..=14).fold(String::new(), |mut s, i| {
        s.push_str(&format!("line {i}\n"));
        s
    });
    let got = assembler.assemble("tea", session.trim_end(), "s").unwrap();
    let layers: Vec<_> = got.pieces.iter().map(|p| p.layer).collect();
    assert_eq!(
        layers,
        [
            ContextLayer::WorkingContext,
            ContextLayer::WorkingMemory,
            ContextLayer::Summary,
            ContextLayer::Retrieval
        ]
    );
    // max_working_turns=5 keeps the last ten lines.
    assert!(
        got.pieces[0].content.starts_with("line 5"),
        "{}",
        got.pieces[0].content
    );
    assert_eq!(
        got.pieces[1].content,
        "Active working memories:\n- keep the daemon on port 8787\n"
    );
    assert_eq!(got.pieces[3].content, "- tea is preferred");
    assert_eq!(
        got.used_tokens,
        got.pieces.iter().map(|p| p.tokens).sum::<i64>()
    );
    let report = got.budget_report.expect("budget report");
    assert!(report.fit && report.dropped_pieces == 0, "{report:?}");
}

/// (name, tokens per piece, ceiling, kept, dropped layers, fit)
type Case = (&'static str, i64, i64, usize, &'static [&'static str], bool);

fn piece(layer: ContextLayer, tokens: i64) -> AssembledPiece {
    AssembledPiece {
        layer,
        content: format!("{}-content", layer.as_str()),
        tokens,
        receipt: String::new(),
    }
}

fn context_of(pieces: Vec<AssembledPiece>) -> AssembledContext {
    AssembledContext {
        query: String::new(),
        budget: 0,
        used_tokens: pieces.iter().map(|p| p.tokens).sum(),
        pieces,
        budget_report: None,
        expandable_sources: Vec::new(),
        delta: None,
    }
}

#[test]
fn hard_budget_drops_lowest_priority_first_but_never_the_first_two() {
    use super::budget::enforce;
    let layers = [
        ContextLayer::WorkingContext,
        ContextLayer::WorkingMemory,
        ContextLayer::Summary,
        ContextLayer::Profile,
        ContextLayer::Retrieval,
    ];
    let cases: [Case; 5] = [
        ("no budget", 10, 0, 5, &[], true),
        ("fits", 10, 50, 5, &[], true),
        (
            "drops from the end",
            10,
            32,
            3,
            &["retrieval", "profile"],
            false,
        ),
        (
            "pinned pair survives overflow",
            10,
            5,
            2,
            &["retrieval", "profile", "summary"],
            false,
        ),
        ("exact bound fits", 10, 50, 5, &[], true),
    ];
    for (name, tokens, ceiling, kept, dropped, fit) in cases {
        let mut ctx = context_of(layers.map(|l| piece(l, tokens)).to_vec());
        enforce(&mut ctx, ceiling);
        assert_eq!(ctx.pieces.len(), kept, "{name}");
        let Some(report) = &ctx.budget_report else {
            assert_eq!(ceiling, 0, "{name}: report expected");
            continue;
        };
        assert_eq!(report.fit, fit, "{name}");
        assert_eq!(report.dropped_ids, dropped, "{name}");
        assert_eq!(
            report.dropped_pieces,
            i64::try_from(dropped.len()).unwrap(),
            "{name}"
        );
        assert_eq!(
            report.estimated_tokens,
            ctx.pieces.iter().map(|p| p.tokens).sum::<i64>(),
            "{name}"
        );
        assert_eq!(report.max_tokens, ceiling, "{name}");
        assert!(!report.reason.is_empty());
    }
    let mut ctx = context_of(layers.map(|l| piece(l, 10)).to_vec());
    enforce(&mut ctx, 32);
    assert!(ctx.budget_report.unwrap().reason.contains("never dropped"));
}

#[test]
fn assemble_always_reports_the_budget_it_enforced() {
    let now = || t0();
    let fake = Fake::default();
    let assembler = Assembler::new(
        Some(&fake),
        ContextConfig {
            token_budget: 60,
            max_working_turns: 5,
        },
        &now,
    );
    let got = assembler.assemble("", "hello world", "").unwrap();
    assert_eq!(got.budget, 60);
    assert!(
        got.budget_report
            .is_some_and(|r| r.max_tokens == 60 && r.fit)
    );
    // A piece that alone exceeds the budget is skipped by the layer check.
    let long = "word ".repeat(200);
    let got = assembler.assemble("", &long, "").unwrap();
    assert!(got.pieces.is_empty());
    let zero = Assembler::new(
        None,
        ContextConfig {
            token_budget: 0,
            max_working_turns: 5,
        },
        &now,
    );
    assert_eq!(zero.assemble("", "x", "").unwrap().budget, 2000);
}

#[test]
fn degradation_ladder_orders_by_band_then_kind_and_degrades_to_fit() {
    let now = || t0();
    let long = "A long sentence that keeps going and going. ".repeat(12);
    let fake = Fake {
        vector: vec![
            recalled("ref-1", "project chatter", "project", 0.80),
            recalled("usr-1", "identity fact", "user", 0.81),
            recalled("big-1", &long, "reference", 0.95),
        ],
        ..Fake::default()
    };
    let assembler = Assembler::new(Some(&fake), ContextConfig::default(), &now)
        .with_embedder(embedder)
        .with_degradation(DegradationConfig {
            enabled: true,
            full_tokens: 40,
            summary_tokens: 30,
            ref_tokens: 30,
        });
    let got = assembler.assemble("q", "", "").unwrap();
    let texts: Vec<_> = got.pieces.iter().map(|p| p.content.as_str()).collect();
    // The big item is best-scored first but only fits as a summary; the
    // two 0.8-band items follow with the user-kind one ahead of project.
    assert_eq!(texts.len(), 3, "{texts:?}");
    assert!(texts[0].contains("[ref: big-1]"), "{}", texts[0]);
    assert_eq!(texts[1], "- identity fact");
    assert_eq!(texts[2], "- project chatter");
}

#[test]
fn legacy_block_when_degradation_is_off_and_receipts_when_on() {
    let now = || t0();
    let fake = Fake {
        vector: vec![recalled("m1", "daemon runs on port 8787", "project", 0.9)],
        ..Fake::default()
    };
    let plain = Assembler::new(Some(&fake), ContextConfig::default(), &now).with_embedder(embedder);
    let got = plain.assemble("q", "", "").unwrap();
    assert_eq!(
        got.pieces[0].content,
        "Relevant stored memories:\n- daemon runs on port 8787\n"
    );
    assert_eq!(got.pieces[0].receipt, "");

    let receipts = Assembler::new(Some(&fake), ContextConfig::default(), &now)
        .with_embedder(embedder)
        .with_degradation(DegradationConfig::default())
        .with_recall_receipts(true);
    let got = receipts.assemble("q", "", "").unwrap();
    assert_eq!(
        got.pieces[0].receipt,
        "◉ memory: \"daemon runs on port 8787\" (project, 3d)"
    );
    let json = serde_json::to_value(&got).unwrap();
    assert_eq!(json["pieces"][0]["layer"], "retrieval");
    assert!(json["pieces"][0]["receipt"].is_string());
}

#[test]
fn credentials_never_reach_prompt_context() {
    let now = || t0();
    let fake = Fake {
        vector: vec![recalled("m1", "mail me at carol@example.com", "user", 0.9)],
        ..Fake::default()
    };
    let assembler = Assembler::new(Some(&fake), ContextConfig::default(), &now)
        .with_embedder(embedder)
        .with_degradation(DegradationConfig::default());
    let got = assembler.assemble("q", "", "").unwrap();
    assert!(
        !got.pieces[0].content.contains("carol@example.com"),
        "{}",
        got.pieces[0].content
    );
}

#[test]
fn profile_and_session_sources_layers() {
    let now = || t0();
    let fake = Fake {
        keyword: vec![
            recalled(
                "k1",
                "user prefers German and timezone Europe/Berlin",
                "user",
                1.0,
            ),
            recalled("k2", "likes espresso", "user", 0.5),
        ],
        ..Fake::default()
    };
    fake.save_session_summary("s", "- a summary").unwrap();
    let assembler = Assembler::new(Some(&fake), ContextConfig::default(), &now)
        .with_profile_layer(ProfileLayerConfig::default())
        .with_session_context(SessionContextConfig::default());
    let got = assembler.assemble("", "", "s").unwrap();
    let layers: Vec<_> = got.pieces.iter().map(|p| p.layer).collect();
    assert_eq!(
        layers,
        [
            ContextLayer::Summary,
            ContextLayer::Profile,
            ContextLayer::SessionContext
        ]
    );
    let profile = &got.pieces[1].content;
    assert!(
        profile
            .starts_with("Profile:\n[static] lang: german, tz: Europe/Berlin\n[dynamic] topics: "),
        "{profile}"
    );
    assert!(
        profile.ends_with("scopes: global, project, user, agent, access: read_write"),
        "{profile}"
    );
    assert_eq!(got.expandable_sources[0].source_type, "session");
    assert_eq!(got.expandable_sources[0].content_hash.len(), 64);
    assert!(got.pieces[2].content.starts_with(
        "Session context sources:\n- [s] Session summary (type: session, expands: 3) ["
    ));
    assert_eq!(got.expandable_sources.len(), 3);
}
