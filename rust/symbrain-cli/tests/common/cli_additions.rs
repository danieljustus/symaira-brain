//! Declared CLI additions as the tests see them (`migration/cli-additions.md`).
//!
//! Reads `assets/cli-additions.json` and applies it to a recorded text with a
//! plain anchor replacement. This is deliberately not the product's renderer
//! (`src/cli_additions.rs`): the replay compares the product against an
//! independent rendering of the same declarations.
#![allow(dead_code)]

use std::path::PathBuf;

/// Last recorded line of the first command group; additions follow it.
pub const ANCHOR: &str =
    "  guard       Absorbed symguard commands (decide, scan, doctor, grants, version)\n";
/// A line that only the top-level usage text contains.
const HEADER: &str = "symbrain — portable agent-context layer for AI harnesses";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Declared {
    pub name: String,
    pub short: String,
    pub platform: String,
}

pub fn declared() -> Vec<Declared> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/cli-additions.json");
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("read additions"))
            .expect("parse additions");
    assert_eq!(json["schema_version"], 1);
    json["additions"]
        .as_array()
        .expect("additions array")
        .iter()
        .map(|entry| {
            let field = |key: &str| entry[key].as_str().expect(key).to_owned();
            Declared {
                name: field("name"),
                short: field("short"),
                platform: field("platform"),
            }
        })
        .collect()
}

/// The list line the declaration adds to the usage.
pub fn line(addition: &Declared) -> String {
    let note = if addition.platform == "macos" {
        " (macOS only)"
    } else {
        ""
    };
    format!("  {:<11} {}{note}\n", addition.name, addition.short)
}

/// `recorded` with the declared additions applied when it holds the top-level
/// usage; any other text is returned unchanged.
pub fn with_additions(recorded: &str) -> String {
    if !recorded.contains(HEADER) {
        return recorded.to_owned();
    }
    assert_eq!(recorded.matches(ANCHOR).count(), 1, "anchor line");
    let lines: String = declared().iter().map(line).collect();
    recorded.replacen(ANCHOR, &format!("{ANCHOR}{lines}"), 1)
}
