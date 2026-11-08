//! Metadata-only source discovery (ported for #760 from
//! `internal/memory/discovery/discovery.go` at v0.12.0).
//!
//! `scan` asks every provider for its sessions without importing anything and
//! returns one entry per logical source. A failing provider is skipped so a
//! missing source never hides the others.

use crate::importer::{ImportResult, SessionRef};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use std::collections::BTreeMap;

/// Version of the discovery envelope.
pub const SCHEMA_VERSION: u32 = 1;

/// One logical source that can be scanned.
pub struct Provider<'a> {
    /// Stable logical identity (not a path).
    pub id: String,
    /// Tool name.
    pub tool: String,
    /// Source kind.
    pub kind: String,
    /// Human-readable name.
    pub display_name: String,
    /// Location shown to clients.
    pub location: String,
    /// Privacy hint.
    pub privacy_hint: String,
    /// Lists the sessions currently present.
    pub discover: Option<Box<dyn Fn() -> ImportResult<Vec<SessionRef>> + 'a>>,
}

/// Metadata-only representation of a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Source {
    /// `symmemory:<provider id>`.
    pub source_id: String,
    /// Tool name.
    pub tool: String,
    /// Source kind.
    pub kind: String,
    /// Human-readable name.
    pub display_name: String,
    /// Location.
    pub location: String,
    /// Always `["import"]`.
    pub capabilities: Vec<String>,
    /// Distinct sessions found.
    #[serde(skip_serializing_if = "is_zero")]
    pub item_count: usize,
    /// Newest modification, RFC 3339 UTC.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub last_seen: String,
    /// Privacy hint.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub privacy_hint: String,
}

#[allow(clippy::trivially_copy_pass_by_ref)]
const fn is_zero(n: &usize) -> bool {
    *n == 0
}

/// The versioned envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Response {
    /// Envelope version.
    pub schema_version: u32,
    /// Sources sorted by tool, then id.
    pub sources: Vec<Source>,
}

struct Group<'p, 'a> {
    provider: &'p Provider<'a>,
    refs: BTreeMap<(String, String), SessionRef>,
}

/// Discovers the sources of all providers at `now`.
#[must_use]
pub fn scan(providers: &[Provider<'_>], now: DateTime<Utc>) -> Response {
    let mut groups: BTreeMap<String, Group<'_, '_>> = BTreeMap::new();
    for provider in providers {
        let Some(discover) = provider
            .discover
            .as_ref()
            .filter(|_| !provider.id.is_empty())
        else {
            continue;
        };
        let Ok(refs) = discover() else { continue };
        let group = groups
            .entry(format!("symmemory:{}", provider.id))
            .or_insert_with(|| Group {
                provider,
                refs: BTreeMap::new(),
            });
        for session in refs {
            let key = if session.session_id.is_empty() {
                session.path.clone()
            } else {
                session.session_id.clone()
            };
            group.refs.insert((session.tool.clone(), key), session);
        }
    }
    let mut sources: Vec<Source> = groups
        .into_iter()
        .filter(|(_, group)| !group.refs.is_empty())
        .map(|(id, group)| {
            let newest = group
                .refs
                .values()
                .map(|r| r.modified_at)
                .max()
                .unwrap_or_default();
            let last_seen = if newest == DateTime::<Utc>::default() {
                now
            } else {
                newest
            };
            let p = group.provider;
            Source {
                source_id: id,
                tool: p.tool.clone(),
                kind: p.kind.clone(),
                display_name: p.display_name.clone(),
                location: p.location.clone(),
                capabilities: vec!["import".into()],
                item_count: group.refs.len(),
                last_seen: last_seen.to_rfc3339_opts(SecondsFormat::Secs, true),
                privacy_hint: p.privacy_hint.clone(),
            }
        })
        .collect();
    sources.sort_by(|a, b| (&a.tool, &a.source_id).cmp(&(&b.tool, &b.source_id)));
    Response {
        schema_version: SCHEMA_VERSION,
        sources,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, day, 10, 0, 0).unwrap()
    }

    fn refs(tool: &str, ids: &[&str], day: u32) -> Vec<SessionRef> {
        ids.iter()
            .map(|id| SessionRef {
                tool: tool.into(),
                session_id: (*id).into(),
                path: format!("/fixture/{id}"),
                modified_at: at(day),
                ..SessionRef::default()
            })
            .collect()
    }

    fn provider<'a>(
        id: &str,
        tool: &str,
        discover: impl Fn() -> ImportResult<Vec<SessionRef>> + 'a,
    ) -> Provider<'a> {
        Provider {
            id: id.into(),
            tool: tool.into(),
            kind: "sessions".into(),
            display_name: format!("{tool} sessions"),
            location: "fixture".into(),
            privacy_hint: String::new(),
            discover: Some(Box::new(discover)),
        }
    }

    #[test]
    fn groups_dedupes_sorts_and_skips_failures() {
        let providers = [
            provider("zeta", "zed", || Ok(refs("zed", &["a"], 1))),
            provider("alpha", "codex", || Ok(refs("codex", &["a", "b", "b"], 5))),
            provider("alpha", "codex", || Ok(refs("codex", &["c"], 9))),
            provider("broken", "git", || Err("boom".into())),
            provider("empty", "aider", || Ok(Vec::new())),
            provider("", "x", || Ok(refs("x", &["a"], 1))),
            Provider {
                discover: None,
                ..provider("nodisc", "y", || Ok(Vec::new()))
            },
        ];
        let response = scan(&providers, at(20));
        assert_eq!(response.schema_version, 1);
        let got: Vec<_> = response
            .sources
            .iter()
            .map(|s| {
                (
                    s.source_id.as_str(),
                    s.tool.as_str(),
                    s.item_count,
                    s.last_seen.as_str(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("symmemory:alpha", "codex", 3, "2026-03-09T10:00:00Z"),
                ("symmemory:zeta", "zed", 1, "2026-03-01T10:00:00Z"),
            ]
        );
        assert_eq!(response.sources[0].capabilities, ["import"]);
    }

    #[test]
    fn zero_mtime_falls_back_to_now_and_empty_session_id_uses_path() {
        let providers = [provider("p", "t", || {
            Ok(vec![
                SessionRef {
                    tool: "t".into(),
                    path: "/one".into(),
                    ..SessionRef::default()
                },
                SessionRef {
                    tool: "t".into(),
                    path: "/two".into(),
                    ..SessionRef::default()
                },
            ])
        })];
        let source = &scan(&providers, at(20)).sources[0];
        assert_eq!(
            (source.item_count, source.last_seen.as_str()),
            (2, "2026-03-20T10:00:00Z")
        );
        let json = serde_json::to_string(&scan(&providers, at(20))).unwrap();
        assert!(
            json.starts_with("{\"schema_version\":1,\"sources\":[{\"source_id\":\"symmemory:p\"")
        );
        assert!(!json.contains("privacy_hint"), "{json}");
    }
}
