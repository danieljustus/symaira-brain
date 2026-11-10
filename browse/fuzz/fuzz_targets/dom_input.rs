#![no_main]

//! Arbitrary bytes through the static document pipeline: HTML5 parse, cleanup,
//! serialization, content selection, Markdown rendering, section ranking and
//! the bounded selector. A NUL byte splits the input into `selector\0html`;
//! without one the whole input is HTML. Keep the body in sync with
//! `crates/symbrowse-fetch/tests/dom_input_smoke.rs`, which replays the seeds
//! on stable Rust.

use libfuzzer_sys::fuzz_target;
use symbrowse_fetch::{dom, relevance, render, semantic};

fn markdown(html: &[u8]) -> Option<String> {
    let mut tree = dom::parse(html).ok()?;
    dom::cleanup(&mut tree.root);
    let _ = dom::serialize(&tree.root);
    let content = semantic::best_block(&tree.root, 20);
    let built = render::build_document(&tree, content, "https://example.com/source", 0);
    let markdown = render::markdown(&built.document, Some(content), true);
    let _ = render::markdown(&built.document, Some(content), false);
    let _ = render::json(&built.document);
    let sections = relevance::split_markdown_sections(&markdown);
    let _ = relevance::rank_sections("important content", &sections, 3);
    Some(markdown)
}

fuzz_target!(|data: &[u8]| {
    let (selector, html) = match data.iter().position(|byte| *byte == 0) {
        Some(split) => (
            Some(String::from_utf8_lossy(&data[..split]).into_owned()),
            &data[split + 1..],
        ),
        None => (None, data),
    };
    // The pipeline is a pure function of its input bytes.
    assert_eq!(markdown(html), markdown(html));
    if let (Some(selector), Ok(tree)) = (selector, dom::parse(html)) {
        if let Ok(nodes) = dom::select(&tree.root, &selector) {
            assert!(!nodes.is_empty());
            assert!(
                nodes
                    .iter()
                    .all(|node| matches!(node, dom::Node::Element { .. }))
            );
        }
    }
});
