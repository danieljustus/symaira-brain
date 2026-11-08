//! OpenMemory export importer (`openmemory.go`).

use super::{
    ExportRef, MemoryToolImporter, MetaValue, ToolFact, default_root, discover_json, nullable,
    parse_or_zero, read_json,
};
use crate::importer::ImportResult;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Imports `{"memories": [...], "facts": [...]}` from `<home>/.openmemory`.
#[derive(Debug, Clone)]
pub struct OpenMemoryImporter {
    home: Option<PathBuf>,
    now: fn() -> DateTime<Utc>,
}

#[derive(Deserialize, Default)]
struct Export {
    #[serde(default, deserialize_with = "nullable")]
    memories: Vec<Memory>,
    #[serde(default, deserialize_with = "nullable")]
    facts: Vec<Fact>,
}

#[derive(Deserialize, Default)]
struct Memory {
    #[serde(default, deserialize_with = "nullable")]
    id: String,
    #[serde(default, deserialize_with = "nullable")]
    content: String,
    #[serde(default, deserialize_with = "nullable")]
    created_at: String,
}

#[derive(Deserialize, Default)]
struct Fact {
    #[serde(default, deserialize_with = "nullable")]
    subject: String,
    #[serde(default, deserialize_with = "nullable")]
    predicate: String,
    #[serde(default, deserialize_with = "nullable")]
    object: String,
    #[serde(default, deserialize_with = "nullable")]
    confidence: f64,
}

impl Default for OpenMemoryImporter {
    fn default() -> Self {
        Self {
            home: None,
            now: Utc::now,
        }
    }
}

impl OpenMemoryImporter {
    /// Defaults below `$HOME`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Defaults below an explicit `home`, with an injected clock for the
    /// triple facts (Go stamped them with `time.Now()`).
    #[must_use]
    pub fn with_home(home: PathBuf, now: fn() -> DateTime<Utc>) -> Self {
        Self {
            home: Some(home),
            now,
        }
    }
}

impl MemoryToolImporter for OpenMemoryImporter {
    fn name(&self) -> &'static str {
        "openmemory"
    }

    fn discover_exports(&self, path: &str) -> ImportResult<Vec<ExportRef>> {
        let root = if path.is_empty() {
            default_root(self.home.as_deref(), ".openmemory")?
        } else {
            PathBuf::from(path)
        };
        discover_json("openmemory", &root)
    }

    fn import_export(&self, export: &ExportRef) -> ImportResult<Vec<ToolFact>> {
        let parsed: Export = read_json(&export.path)?;
        let mut facts = Vec::new();
        for memory in parsed.memories {
            facts.push(ToolFact {
                content: memory.content,
                source: "openmemory".into(),
                timestamp: parse_or_zero(&memory.created_at),
                metadata: BTreeMap::from([("openmemory_id".to_owned(), MetaValue::Str(memory.id))]),
            });
        }
        for fact in parsed.facts {
            facts.push(ToolFact {
                content: format!("{} {} {}", fact.subject, fact.predicate, fact.object),
                source: "openmemory".into(),
                timestamp: (self.now)(),
                metadata: BTreeMap::from([(
                    "confidence".to_owned(),
                    MetaValue::Num(fact.confidence),
                )]),
            });
        }
        Ok(facts)
    }
}
