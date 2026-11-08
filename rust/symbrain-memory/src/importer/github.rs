//! GitHub importer through the `gh` CLI (ported for #761 from
//! `importer/github/github.go` at v0.12.0): pull requests and issues of one
//! repository. The command runner is injected so tests never reach the
//! network.
//!
//! Like Go, a failed PR or issue listing only prints a warning and the other
//! listing still counts. `author`/`labels` are decoded as the Go structs did
//! (plain string / string list), so a payload with other shapes is skipped
//! with the same "failed to parse" message. Bodies are cut at 2000 bytes on a
//! character boundary.

use super::runner::{Runner, SystemRunner, truncate_bytes};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Imports PRs and issues of `owner/repo`.
pub struct GitHub<'a> {
    owner: String,
    repo: String,
    runner: Box<dyn Runner + 'a>,
}

#[derive(Deserialize, Default)]
struct Item {
    #[serde(default)]
    number: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    body: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    author: String,
    #[serde(default, rename = "createdAt")]
    created_at: String,
    #[serde(default)]
    url: String,
}

impl<'a> GitHub<'a> {
    /// Imports from the real `gh` binary.
    #[must_use]
    pub fn new(owner: &str, repo: &str) -> Self {
        Self::with_runner(owner, repo, SystemRunner)
    }

    /// As [`GitHub::new`] with an injected runner.
    #[must_use]
    pub fn with_runner(owner: &str, repo: &str, runner: impl Runner + 'a) -> Self {
        Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            runner: Box::new(runner),
        }
    }

    fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    fn list(&self, kind: &str, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let (noun, fields) = if kind == "pr" {
            (
                "pr",
                "number,title,body,state,author,createdAt,mergedAt,url,labels",
            )
        } else {
            (
                "issue",
                "number,title,body,state,author,createdAt,closedAt,url,labels",
            )
        };
        let query = format!(
            "repo:{} is:{noun} created:>={}",
            self.slug(),
            since.format("%Y-%m-%dT%H:%M:%SZ")
        );
        let args: Vec<String> = [
            noun,
            "list",
            "--repo",
            &self.slug(),
            "--search",
            &query,
            "--state",
            "all",
            "--limit",
            "200",
            "--json",
            fields,
        ]
        .map(String::from)
        .into();
        let out = self
            .runner
            .run("gh", &args)
            .map_err(|e| format!("gh {noun} list failed: {e}"))?;
        let items: Vec<Item> = serde_json::from_slice(&out).map_err(|e| {
            format!(
                "failed to parse {} JSON: {e}",
                if kind == "pr" { "PR" } else { "issue" }
            )
        })?;
        Ok(items
            .into_iter()
            .map(|item| SessionRef {
                tool: "github".into(),
                session_id: format!("{kind}/{}", item.number),
                path: item.url,
                modified_at: parse(&item.created_at),
                metadata: BTreeMap::from([
                    ("type".to_owned(), kind.to_owned()),
                    ("state".to_owned(), item.state),
                    ("title".to_owned(), item.title),
                    ("author".to_owned(), item.author),
                ]),
            })
            .collect())
    }
}

fn parse(raw: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(raw).map_or_else(
        |_| crate::sync_state::zero_time(),
        |t| t.with_timezone(&Utc),
    )
}

impl SessionImporter for GitHub<'_> {
    fn name(&self) -> &'static str {
        "github"
    }
    fn category(&self) -> Option<&str> {
        Some("code")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Internal)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let mut sessions = Vec::new();
        for (kind, label) in [("pr", "PRs"), ("issue", "issues")] {
            match self.list(kind, since) {
                Ok(found) => sessions.extend(found),
                Err(e) => eprintln!("Warning: failed to discover {label}: {e}"),
            }
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let Some((kind, number)) = session.session_id.split_once('/') else {
            return Err(format!("invalid session ID format: {}", session.session_id));
        };
        let (noun, fields, title, label) = match kind {
            "pr" => (
                "pr",
                "number,title,body,state,author,createdAt,mergedAt,url,labels,files",
                "PR",
                "PR",
            ),
            "issue" => (
                "issue",
                "number,title,body,state,author,createdAt,closedAt,url,labels,comments",
                "Issue",
                "issue",
            ),
            other => return Err(format!("unknown item type: {other}")),
        };
        let args: Vec<String> = [
            noun,
            "view",
            number,
            "--repo",
            &self.slug(),
            "--json",
            fields,
        ]
        .map(String::from)
        .into();
        let out = self
            .runner
            .run("gh", &args)
            .map_err(|e| format!("gh view failed for {}: {e}", session.session_id))?;
        let item: Item =
            super::parse_object(&out).map_err(|e| format!("failed to parse {label} JSON: {e}"))?;
        let mut content = format!("{title} #{}: {}", item.number, item.title);
        if !item.body.is_empty() {
            let body = if item.body.len() > 2000 {
                format!("{}...", truncate_bytes(&item.body, 2000))
            } else {
                item.body.clone()
            };
            content.push_str("\n\n");
            content.push_str(&body);
        }
        Ok(vec![ImportedFact {
            content,
            source: "github".into(),
            session_id: session.session_id.clone(),
            timestamp: parse(&item.created_at),
            metadata: BTreeMap::from([
                ("type".to_owned(), kind.to_owned()),
                ("state".to_owned(), item.state),
                ("url".to_owned(), item.url),
            ]),
            evidence: Vec::new(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::runner::fake::Fake;
    use chrono::TimeZone;

    const PRS: &str = r#"[{"number":7,"title":"Add flag","state":"MERGED","author":"ann","createdAt":"2026-05-01T08:00:00Z","url":"https://example.test/pr/7"}]"#;
    const ISSUES: &str = r#"[{"number":3,"title":"Crash","state":"OPEN","author":"bob","createdAt":"2026-05-02T09:30:00.5Z","url":"https://example.test/issues/3"}]"#;

    fn since() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn lists_prs_and_issues_with_go_arguments() {
        let fake = Fake::new(vec![("pr list", Ok(PRS)), ("issue list", Ok(ISSUES))]);
        let importer = GitHub::with_runner("acme", "widgets", &fake);
        let sessions = importer.discover_sessions(since()).unwrap();
        let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, ["pr/7", "issue/3"]);
        assert_eq!(sessions[0].metadata["state"], "MERGED");
        assert_eq!(sessions[0].path, "https://example.test/pr/7");
        assert_eq!(
            sessions[1].modified_at.timestamp(),
            Utc.with_ymd_and_hms(2026, 5, 2, 9, 30, 0)
                .unwrap()
                .timestamp()
        );
        let calls = fake.calls.borrow();
        assert_eq!(
            calls[0],
            "gh pr list --repo acme/widgets --search repo:acme/widgets is:pr created:>=2026-04-01T00:00:00Z --state all --limit 200 --json number,title,body,state,author,createdAt,mergedAt,url,labels"
        );
        assert!(calls[1].contains("issue list") && calls[1].contains("closedAt"));
    }

    #[test]
    fn one_failing_listing_does_not_hide_the_other() {
        let fake = Fake::new(vec![
            ("pr list", Err("exit status 1")),
            ("issue list", Ok(ISSUES)),
        ]);
        let sessions = GitHub::with_runner("a", "b", &fake)
            .discover_sessions(since())
            .unwrap();
        assert_eq!(sessions.len(), 1);
        let broken = Fake::new(vec![("pr list", Ok("{")), ("issue list", Ok("[]"))]);
        assert!(
            GitHub::with_runner("a", "b", &broken)
                .discover_sessions(since())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn import_session_cases() {
        let long = "x".repeat(2100);
        let issue_json: &'static str = Box::leak(
            format!(r#"{{"number":3,"title":"Crash","body":"{long}","state":"OPEN","createdAt":"2026-05-02T09:30:00Z","url":"u"}}"#).into_boxed_str(),
        );
        let fake = Fake::new(vec![
            (
                "pr view 7",
                Ok(PRS.trim_start_matches('[').trim_end_matches(']')),
            ),
            ("issue view 3", Ok(issue_json)),
        ]);
        let importer = GitHub::with_runner("o", "r", &fake);
        let pr = SessionRef {
            session_id: "pr/7".into(),
            ..SessionRef::default()
        };
        let facts = importer.import_session(&pr).unwrap();
        assert_eq!(facts[0].content, "PR #7: Add flag");
        assert_eq!(facts[0].metadata["type"], "pr");
        let issue = SessionRef {
            session_id: "issue/3".into(),
            ..SessionRef::default()
        };
        let content = &importer.import_session(&issue).unwrap()[0].content;
        assert!(content.starts_with("Issue #3: Crash\n\n") && content.ends_with("..."));
        assert_eq!(content.len(), "Issue #3: Crash\n\n".len() + 2000 + 3);
        for (id, want) in [
            ("nope", "invalid session ID format: nope"),
            ("tag/1", "unknown item type: tag"),
        ] {
            let bad = SessionRef {
                session_id: id.into(),
                ..SessionRef::default()
            };
            assert_eq!(importer.import_session(&bad).unwrap_err(), want);
        }
        let failing = Fake::new(vec![("pr view", Err("exit status 4"))]);
        assert_eq!(
            GitHub::with_runner("o", "r", &failing)
                .import_session(&pr)
                .unwrap_err(),
            "gh view failed for pr/7: exit status 4"
        );
    }
}
