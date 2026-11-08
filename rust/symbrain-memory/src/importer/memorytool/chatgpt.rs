//! ChatGPT `conversations.json` importer (`chatgpt.go`).

use super::{
    ExportRef, MemoryToolImporter, MetaValue, ToolFact, chatgpt_fs, default_root, nullable,
};
use crate::consolidation_prompts::{PROMPT_FAMILY_CODE, PROMPT_MODE_KEY};
use crate::importer::ImportResult;
use chrono::DateTime;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Imports assistant replies longer than 50 bytes from ChatGPT exports found
/// below `<home>/Downloads` (or an explicit path).
#[derive(Debug, Clone, Default)]
pub struct ChatGptImporter {
    home: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
struct Conversation {
    #[serde(default, deserialize_with = "nullable")]
    title: String,
    #[serde(default, deserialize_with = "nullable")]
    create_time: f64,
    // BTreeMap: Go iterated a map in random order; sorted ids keep runs
    // reproducible.
    #[serde(default, deserialize_with = "nullable")]
    mapping: BTreeMap<String, Node>,
}

#[derive(Deserialize, Default)]
struct Node {
    #[serde(default)]
    message: Option<Message>,
}

#[derive(Deserialize, Default)]
struct Message {
    #[serde(default, deserialize_with = "nullable")]
    author: Author,
    #[serde(default, deserialize_with = "nullable")]
    content: Content,
}

#[derive(Deserialize, Default)]
struct Author {
    #[serde(default, deserialize_with = "nullable")]
    role: String,
}

#[derive(Deserialize, Default)]
struct Content {
    #[serde(default, deserialize_with = "nullable")]
    parts: Vec<String>,
}

impl ChatGptImporter {
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

impl MemoryToolImporter for ChatGptImporter {
    fn name(&self) -> &'static str {
        "chatgpt"
    }

    fn discover_exports(&self, path: &str) -> ImportResult<Vec<ExportRef>> {
        let root = if path.is_empty() {
            default_root(self.home.as_deref(), "Downloads")?
        } else {
            PathBuf::from(path)
        };
        chatgpt_fs::discover(&root)
    }

    fn import_export(&self, export: &ExportRef) -> ImportResult<Vec<ToolFact>> {
        let data = chatgpt_fs::read_export(Path::new(&export.path))
            .map_err(|e| format!("failed to read file: {e}"))?;
        let conversations: Option<Vec<Option<Conversation>>> =
            serde_json::from_slice(&data).map_err(|e| format!("failed to parse JSON: {e}"))?;
        let mut facts = Vec::new();
        for conversation in conversations.into_iter().flatten().flatten() {
            #[allow(clippy::cast_possible_truncation)]
            let timestamp =
                DateTime::from_timestamp(conversation.create_time as i64, 0).unwrap_or_default();
            for node in conversation.mapping.into_values() {
                let Some(message) = node.message else {
                    continue;
                };
                if message.author.role != "assistant" {
                    continue;
                }
                let content = message.content.parts.join(" ");
                if content.len() <= 50 {
                    continue;
                }
                facts.push(ToolFact {
                    content,
                    source: "chatgpt".into(),
                    timestamp,
                    metadata: BTreeMap::from([
                        (
                            "title".to_owned(),
                            MetaValue::Str(conversation.title.clone()),
                        ),
                        // #483: coding-session material selects the code prompt family.
                        (
                            PROMPT_MODE_KEY.to_owned(),
                            MetaValue::Str(PROMPT_FAMILY_CODE.into()),
                        ),
                    ]),
                });
            }
        }
        Ok(facts)
    }
}
