//! Names-only session recording; never arguments, results, secrets or policies.
use crate::Gateway;
use chrono::{SecondsFormat, Utc};
use std::io;
use symbrain_patterns::{Episode, Step, Store};

#[cfg(test)]
#[path = "session_patterns_tests.rs"]
mod tests;

impl Gateway {
    /// Supplies resolved Brain options without changing tool exposure.
    #[must_use]
    pub fn with_patterns(mut self, enabled: bool, threshold: i64) -> Self {
        self.pattern_threshold = usize::try_from(threshold)
            .ok()
            .filter(|v| *v > 0)
            .unwrap_or(3);
        self.episode = std::sync::Mutex::new(None);
        if enabled {
            self.episode = std::sync::Mutex::new(Some(Episode {
                profile: self.profile.name.clone(),
                steps: Vec::new(),
                started_at: Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                ended_at: String::new(),
            }));
        }
        self
    }
    pub(super) fn record_step(&self, server: &str, tool: &str) {
        if let Ok(mut episode) = self.episode.lock()
            && let Some(episode) = episode.as_mut()
        {
            episode.steps.push(Step {
                server: server.into(),
                tool: tool.into(),
            });
        }
    }
    /// Completes this invocation's episode once; callers log failures and continue.
    /// # Errors
    /// Returns owned path/store errors, never a tool failure or authorization change.
    pub fn finish_patterns(&self) -> io::Result<()> {
        let Some(mut episode) = self
            .episode
            .lock()
            .map_err(|_| io::Error::other("patterns recorder poisoned"))?
            .take()
        else {
            return Ok(());
        };
        if episode.steps.is_empty() {
            return Ok(());
        }
        episode.ended_at = Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
        let directory = symbrain_core::xdg::patterns_dir()
            .ok_or_else(|| io::Error::other("patterns: resolve data directory"))?;
        Store::new_private(directory.join(format!("{}.jsonl", self.profile.name))).append(&episode)
    }
}
