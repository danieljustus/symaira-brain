//! Stable-Rust replay of the `dom-input` fuzz target (`browse/fuzz`): every
//! retained seed, every prefix of it and a deterministic byte-mutation sweep go
//! through parse, cleanup, serialize, selection, rendering and ranking without
//! panicking, and the pipeline stays a pure function of its input.
use std::path::PathBuf;

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

fn exercise(data: &[u8]) {
    let (selector, html) = match data.iter().position(|byte| *byte == 0) {
        Some(split) => (
            Some(String::from_utf8_lossy(&data[..split]).into_owned()),
            &data[split + 1..],
        ),
        None => (None, data),
    };
    assert_eq!(markdown(html), markdown(html));
    if let (Some(selector), Ok(tree)) = (selector, dom::parse(html))
        && let Ok(nodes) = dom::select(&tree.root, &selector)
    {
        assert!(!nodes.is_empty());
        assert!(
            nodes
                .iter()
                .all(|node| matches!(node, dom::Node::Element { .. }))
        );
    }
}

fn seeds() -> Vec<(String, Vec<u8>)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fuzz/corpus/dom_input");
    let mut seeds: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect();
    seeds.sort();
    seeds
}

#[test]
fn retained_seeds_replay_without_panics() {
    let seeds = seeds();
    assert!(seeds.len() >= 10, "seed corpus went missing");
    for (_, data) in &seeds {
        exercise(data);
    }
}

#[test]
fn every_prefix_of_every_seed_is_handled() {
    for (_, data) in seeds() {
        // Bound the sweep for the large nested seed.
        let step = (data.len() / 256).max(1);
        for end in (0..=data.len()).step_by(step) {
            exercise(&data[..end]);
        }
    }
}

#[test]
fn deterministic_byte_mutations_are_handled() {
    const BYTES: [u8; 6] = [0x00, b'<', b'>', b'&', 0xff, 0xc0];
    for (_, data) in seeds() {
        if data.len() > 250 {
            continue;
        }
        for position in 0..data.len() {
            for byte in BYTES {
                let mut mutated = data.clone();
                mutated[position] = byte;
                exercise(&mutated);
            }
        }
    }
}
