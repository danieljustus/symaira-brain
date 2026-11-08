//! Synthesized profile layer (#403).

use super::ProfileLayerConfig;
use super::format::{prefix, truncate};
use super::source::ContextSource;
use crate::pii::redact;
use serde::Serialize;

/// Persistent profile attributes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct StaticProfile {
    /// User id.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub user_id: String,
    /// Display name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    /// Language.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub language: String,
    /// Time zone.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub timezone: String,
}

/// Session-derived profile attributes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DynamicProfile {
    /// Recent topics.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub recent_topics: Vec<String>,
    /// Scopes in use.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub preferred_scopes: Vec<String>,
    /// e.g. `read_write`.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub access_pattern: String,
}

/// Static plus dynamic profile.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Profile {
    /// Static part.
    #[serde(rename = "static")]
    pub static_info: StaticProfile,
    /// Dynamic part.
    pub dynamic: DynamicProfile,
}

fn language_of(text: &str) -> String {
    let lower = text.to_lowercase();
    [
        "english", "german", "french", "spanish", "chinese", "japanese",
    ]
    .iter()
    .find(|l| lower.contains(**l))
    .map(|l| (*l).to_owned())
    .unwrap_or_default()
}

fn timezone_of(text: &str) -> String {
    [
        "UTC",
        "Europe/Berlin",
        "America/New_York",
        "America/Los_Angeles",
        "Asia/Shanghai",
        "Asia/Tokyo",
    ]
    .iter()
    .find(|z| text.contains(**z))
    .map(|z| (*z).to_owned())
    .unwrap_or_default()
}

/// Builds the profile from stored preferences; a failing search leaves the
/// part empty. Everything quoted from memory is redacted first.
pub(crate) fn synthesize(source: Option<&dyn ContextSource>, session_id: &str) -> Profile {
    let mut profile = Profile::default();
    if let Some(source) = source {
        if let Ok(prefs) = source.keyword_search("user prefers language role timezone", 10) {
            for pref in prefs {
                let lower = pref.content.to_lowercase();
                if (lower.contains("language") || lower.contains("prefers"))
                    && profile.static_info.language.is_empty()
                    && pref.content.len() < 80
                {
                    profile.static_info.language = language_of(&pref.content);
                }
                if (lower.contains("timezone") || lower.contains("time zone"))
                    && profile.static_info.timezone.is_empty()
                {
                    profile.static_info.timezone = timezone_of(&pref.content);
                }
            }
        }
        if !session_id.is_empty()
            && let Ok(topics) = source.keyword_search("topic preference category", 5)
        {
            for topic in topics.into_iter().take(5) {
                profile
                    .dynamic
                    .recent_topics
                    .push(prefix(&redact(&topic.content), 60).to_owned());
            }
        }
    }
    "read_write".clone_into(&mut profile.dynamic.access_pattern);
    profile.dynamic.preferred_scopes = ["global", "project", "user", "agent"]
        .map(str::to_owned)
        .to_vec();
    profile
}

/// The text of the profile layer (empty when there is nothing to say).
pub(crate) fn format_profile(profile: &Profile, config: &ProfileLayerConfig) -> String {
    let mut parts = Vec::new();
    let mut fixed = Vec::new();
    let info = &profile.static_info;
    if !info.user_id.is_empty() {
        fixed.push(format!(
            "user: {}",
            truncate(&info.user_id, config.max_static_len)
        ));
    }
    if !info.display_name.is_empty() {
        fixed.push(format!(
            "display: {}",
            truncate(&info.display_name, config.max_static_len)
        ));
    }
    if !info.language.is_empty() {
        fixed.push(format!("lang: {}", info.language));
    }
    if !info.timezone.is_empty() {
        fixed.push(format!("tz: {}", info.timezone));
    }
    if !fixed.is_empty() {
        parts.push(format!("[static] {}", fixed.join(", ")));
    }
    let mut dynamic = Vec::new();
    let live = &profile.dynamic;
    if !live.recent_topics.is_empty() {
        let topics: Vec<_> = live.recent_topics.iter().take(5).cloned().collect();
        dynamic.push(format!("topics: {}", topics.join(", ")));
    }
    if !live.preferred_scopes.is_empty() {
        dynamic.push(format!("scopes: {}", live.preferred_scopes.join(", ")));
    }
    if !live.access_pattern.is_empty() {
        dynamic.push(format!("access: {}", live.access_pattern));
    }
    if !dynamic.is_empty() {
        parts.push(format!("[dynamic] {}", dynamic.join(", ")));
    }
    if parts.is_empty() {
        return String::new();
    }
    format!("Profile:\n{}", parts.join("\n"))
}
