//! Mem0 export importer (`mem0.go`).

use super::{
    ExportRef, MemoryToolImporter, MetaValue, ToolFact, default_root, discover_json, nullable,
    parse_or_zero, read_json,
};
use crate::consolidation_prompts::{PROMPT_FAMILY_CODE, PROMPT_MODE_KEY};
use crate::importer::ImportResult;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Imports `{"memories": [...]}` exports from `<home>/.mem0`.
#[derive(Debug, Clone, Default)]
pub struct Mem0Importer {
    home: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
struct Export {
    #[serde(default, deserialize_with = "nullable")]
    memories: Vec<Memory>,
}

#[derive(Deserialize, Default)]
struct Memory {
    #[serde(default, deserialize_with = "nullable")]
    id: String,
    #[serde(default, deserialize_with = "nullable")]
    content: String,
    #[serde(default, deserialize_with = "nullable")]
    user_id: String,
    #[serde(default, deserialize_with = "nullable")]
    agent_id: String,
    #[serde(default, deserialize_with = "nullable")]
    created_at: String,
}

impl Mem0Importer {
    /// Defaults below `$HOME`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Defaults below an explicit `home`.
    #[must_use]
    pub fn with_home(home: PathBuf) -> Self {
        Self { home: Some(home) }
    }
}

impl MemoryToolImporter for Mem0Importer {
    fn name(&self) -> &'static str {
        "mem0"
    }

    fn discover_exports(&self, path: &str) -> ImportResult<Vec<ExportRef>> {
        let root = if path.is_empty() {
            default_root(self.home.as_deref(), ".mem0")?
        } else {
            PathBuf::from(path)
        };
        discover_json("mem0", &root)
    }

    fn import_export(&self, export: &ExportRef) -> ImportResult<Vec<ToolFact>> {
        let parsed: Export = read_json(&export.path)?;
        Ok(parsed
            .memories
            .into_iter()
            .map(|memory| {
                let mut metadata = BTreeMap::new();
                metadata.insert("mem0_id".to_owned(), MetaValue::Str(memory.id));
                // #483: coding-session material selects the code prompt family.
                metadata.insert(
                    PROMPT_MODE_KEY.to_owned(),
                    MetaValue::Str(PROMPT_FAMILY_CODE.into()),
                );
                if !memory.user_id.is_empty() {
                    metadata.insert("user_id".into(), MetaValue::Str(memory.user_id));
                }
                if !memory.agent_id.is_empty() {
                    metadata.insert("agent_id".into(), MetaValue::Str(memory.agent_id));
                }
                ToolFact {
                    content: memory.content,
                    source: "mem0".into(),
                    timestamp: parse_or_zero(&memory.created_at),
                    metadata,
                }
            })
            .collect())
    }
}
