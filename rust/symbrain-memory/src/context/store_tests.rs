//! Delta packs and the store-backed source (#760).

use super::source::{ContextSource, StoreSource};
use super::tests::{Fake, embedder, t0};
use super::*;
use crate::Store;
use crate::consolidation::hash_embed;
use crate::model::SetOptions;
use chrono::Duration;
use std::cell::RefCell;

#[test]
fn delta_packs_follow_growth_and_spacing() {
    let clock = RefCell::new(t0());
    let now = || *clock.borrow();
    let fake = Fake::default();
    let assembler = Assembler::new(Some(&fake), ContextConfig::default(), &now)
        .with_delta_packs(DeltaPackConfig::default());

    let first = assembler.assemble("", "short note", "s").unwrap();
    assert!(first.delta.is_none(), "first snapshot has no delta");
    assert_eq!(assembler.snapshots("s").len(), 1);

    // Growth is large enough but the interval has not elapsed.
    *clock.borrow_mut() = t0() + Duration::seconds(30);
    let soon = assembler
        .assemble("", &"a much longer working note ".repeat(10), "s")
        .unwrap();
    assert!(soon.delta.is_none());

    *clock.borrow_mut() = t0() + Duration::seconds(61);
    let later = assembler
        .assemble("", &"a much longer working note ".repeat(10), "s")
        .unwrap();
    let delta = later.delta.expect("delta after interval and growth");
    assert_eq!(delta.token_delta, later.used_tokens - first.used_tokens);
    assert_eq!(delta.added_pieces.len(), 1);
    assert_eq!(delta.added_pieces[0].tokens, -1);
    assert_eq!(delta.from_snapshot, assembler.snapshots("s")[0].key());
    assert_eq!(delta.to_snapshot, assembler.snapshots("s")[1].key());

    // Shrinking never snapshots; unknown sessions have an empty chain.
    *clock.borrow_mut() = t0() + Duration::seconds(200);
    assert!(assembler.assemble("", "tiny", "s").unwrap().delta.is_none());
    assert!(assembler.snapshots("other").is_empty());
}

#[test]
fn store_source_round_trips_summary_working_keyword_and_vector() {
    let store = Store::open_in_memory().unwrap();
    let now = || t0();
    let source = StoreSource::new(&store, &now);
    assert_eq!(source.session_summary("s").unwrap(), "");
    source.save_session_summary("s", "first").unwrap();
    source.save_session_summary("s", "second").unwrap();
    assert_eq!(source.session_summary("s").unwrap(), "second");

    let opts = SetOptions {
        working: true,
        actor: "test".into(),
        ..SetOptions::default()
    };
    store
        .set_with_options(
            "scratch note about espresso",
            "agent",
            "reference",
            serde_json::Map::new(),
            &opts,
        )
        .unwrap();
    store
        .set(
            "espresso beans are stored in the pantry",
            "agent",
            "user",
            serde_json::Map::new(),
            false,
        )
        .unwrap();
    let working = source.working_memories(10).unwrap();
    assert_eq!(working.len(), 1);
    assert_eq!(working[0].tier, "working");

    let keyword = source
        .keyword_search("where are espresso beans?", 5)
        .unwrap();
    assert!(
        keyword.iter().any(|r| r.content.contains("pantry")),
        "{keyword:?}"
    );
    assert!(source.keyword_search("the a of", 5).unwrap().is_empty());

    let (vector, space, _) = hash_embed("espresso beans are stored in the pantry");
    let near = source.vector_search(&vector, &space, 3).unwrap();
    assert_eq!(near.first().map(|r| r.kind.as_str()), Some("user"));

    let assembler = Assembler::new(Some(&source), ContextConfig::default(), &now)
        .with_embedder(embedder)
        .with_degradation(DegradationConfig::default());
    let got = assembler
        .assemble("espresso beans are stored in the pantry", "", "s")
        .unwrap();
    assert!(
        got.pieces
            .iter()
            .any(|p| p.layer == ContextLayer::Summary && p.content == "second")
    );
    assert!(
        got.pieces
            .iter()
            .any(|p| p.layer == ContextLayer::Retrieval)
    );

    let produced = assembler
        .produce_session_summary(
            "We decided to keep the beans in the pantry. The grinder is broken and needs repair.",
            "s2",
        )
        .unwrap();
    assert!(!produced.is_empty());
    assert_eq!(source.session_summary("s2").unwrap(), produced);
    assert_eq!(assembler.produce_session_summary("", "s3").unwrap(), "");
}
