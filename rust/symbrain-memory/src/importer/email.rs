//! Email importer through the `himalaya` CLI (ported for #761 from
//! `importer/email/email.go` at v0.12.0).
//!
//! Deviation from Go: discovery also records the message `date` so facts carry
//! the message time (Go looked up a `date` key that was never set and stored
//! the zero time). Bodies are cut on a character boundary.

use super::runner::{Runner, SystemRunner, truncate_bytes};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Imports mail listed by `himalaya`.
pub struct Email<'a> {
    folder: String,
    importance: String,
    max_body: usize,
    include_senders: Vec<String>,
    exclude_senders: Vec<String>,
    runner: Box<dyn Runner + 'a>,
}

#[derive(Deserialize, Default)]
struct Message {
    #[serde(default)]
    id: String,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    from: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    flags: Vec<String>,
}

#[derive(Deserialize, Default)]
struct Body {
    #[serde(default)]
    text: String,
}

impl<'a> Email<'a> {
    /// `importance` (`high`, `low` or empty) filters on a message flag;
    /// `max_body <= 0` means 2000 bytes.
    #[must_use]
    pub fn new(folder: &str, importance: &str, max_body: usize) -> Self {
        Self::with_runner(folder, importance, max_body, SystemRunner)
    }

    /// As [`Email::new`] with an injected command runner.
    #[must_use]
    pub fn with_runner(
        folder: &str,
        importance: &str,
        max_body: usize,
        runner: impl Runner + 'a,
    ) -> Self {
        Self {
            folder: folder.to_owned(),
            importance: importance.to_owned(),
            max_body: if max_body == 0 { 2000 } else { max_body },
            include_senders: Vec::new(),
            exclude_senders: Vec::new(),
            runner: Box::new(runner),
        }
    }

    /// Sender substring filters: when `include` is non-empty only matching
    /// senders pass, otherwise matching `exclude` senders are dropped.
    #[must_use]
    pub fn with_sender_filters(mut self, include: Vec<String>, exclude: Vec<String>) -> Self {
        self.include_senders = include;
        self.exclude_senders = exclude;
        self
    }

    fn matches_sender(&self, from: &str) -> bool {
        if !self.include_senders.is_empty() {
            return self
                .include_senders
                .iter()
                .any(|p| from.contains(p.as_str()));
        }
        !self
            .exclude_senders
            .iter()
            .any(|p| from.contains(p.as_str()))
    }
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| (*s).to_owned()).collect()
}

impl SessionImporter for Email<'_> {
    fn name(&self) -> &'static str {
        "email"
    }
    fn category(&self) -> Option<&str> {
        Some("communication")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Confidential)
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let folder = if self.folder.is_empty() {
            "INBOX"
        } else {
            &self.folder
        };
        let out = self
            .runner
            .run(
                "himalaya",
                &args(&["message", "list", "--folder", folder, "--json"]),
            )
            .map_err(|e| format!("himalaya list failed: {e}"))?;
        let messages: Vec<Message> = serde_json::from_slice(&out)
            .map_err(|e| format!("failed to parse himalaya output: {e}"))?;
        let mut sessions = Vec::new();
        for message in messages {
            let Ok(time) = DateTime::parse_from_rfc3339(&message.date) else {
                continue;
            };
            let time = time.with_timezone(&Utc);
            if time < since {
                continue;
            }
            if !self.importance.is_empty()
                && !message
                    .flags
                    .iter()
                    .any(|f| f.eq_ignore_ascii_case(&self.importance))
            {
                continue;
            }
            if !self.matches_sender(&message.from) {
                continue;
            }
            sessions.push(SessionRef {
                tool: "email".into(),
                path: format!("email://{folder}/{}", message.id),
                session_id: message.id,
                modified_at: time,
                metadata: BTreeMap::from([
                    ("subject".to_owned(), message.subject),
                    ("from".to_owned(), message.from),
                    ("folder".to_owned(), folder.to_owned()),
                    ("date".to_owned(), message.date),
                ]),
            });
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let meta = |key: &str| session.metadata.get(key).cloned().unwrap_or_default();
        let folder = Some(meta("folder"))
            .filter(|f| !f.is_empty())
            .unwrap_or_else(|| "INBOX".into());
        let out = self
            .runner
            .run(
                "himalaya",
                &args(&[
                    "message",
                    "get",
                    "--folder",
                    &folder,
                    "--json",
                    &session.session_id,
                ]),
            )
            .map_err(|e| format!("himalaya get failed: {e}"))?;
        let body: Body =
            super::parse_object(&out).map_err(|e| format!("failed to parse himalaya body: {e}"))?;
        let content = if body.text.len() > self.max_body {
            format!("{}...", truncate_bytes(&body.text, self.max_body))
        } else {
            body.text
        };
        let (subject, from) = (meta("subject"), meta("from"));
        let mut text = format!("Email from {from}: {subject}");
        if !content.is_empty() {
            text.push_str("\n\n");
            text.push_str(&content);
        }
        let timestamp = DateTime::parse_from_rfc3339(&meta("date")).map_or_else(
            |_| crate::sync_state::zero_time(),
            |t| t.with_timezone(&Utc),
        );
        Ok(vec![ImportedFact {
            content: text,
            source: "email".into(),
            session_id: session.session_id.clone(),
            timestamp,
            metadata: BTreeMap::from([
                ("message_id".to_owned(), session.session_id.clone()),
                ("from".to_owned(), from),
                ("subject".to_owned(), subject),
                ("folder".to_owned(), folder),
                ("source".to_owned(), "email".to_owned()),
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

    const LIST: &str = r#"[
      {"id":"1","subject":"Plan","from":"Ann <ann@example.test>","date":"2026-07-02T10:00:00Z","flags":["Seen","High"]},
      {"id":"2","subject":"Old","from":"bob@example.test","date":"2026-01-02T10:00:00Z","flags":["high"]},
      {"id":"3","subject":"Spam","from":"promo@spam.test","date":"2026-07-03T10:00:00Z","flags":["high"]},
      {"id":"4","subject":"No flag","from":"ann@example.test","date":"2026-07-04T10:00:00Z","flags":[]},
      {"id":"5","subject":"Bad date","from":"ann@example.test","date":"yesterday","flags":["high"]}
    ]"#;

    fn since() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 6, 1, 0, 0, 0).unwrap()
    }

    #[test]
    fn discovery_filters_by_date_flag_and_sender() {
        let fake = Fake::new(vec![("message list", Ok(LIST))]);
        let importer = Email::with_runner("", "high", 0, &fake)
            .with_sender_filters(Vec::new(), vec!["spam.test".into()]);
        let sessions = importer.discover_sessions(since()).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].session_id, "1");
        assert_eq!(sessions[0].path, "email://INBOX/1");
        assert_eq!(sessions[0].metadata["from"], "Ann <ann@example.test>");
        assert_eq!(
            fake.calls.borrow()[0],
            "himalaya message list --folder INBOX --json"
        );
        let only = Email::with_runner("Work", "", 0, &fake)
            .with_sender_filters(vec!["promo".into()], vec!["promo".into()]);
        let ids: Vec<_> = only
            .discover_sessions(since())
            .unwrap()
            .into_iter()
            .map(|s| s.session_id)
            .collect();
        assert_eq!(ids, ["3"], "include list wins over exclude");
        assert_eq!(
            fake.calls.borrow()[1],
            "himalaya message list --folder Work --json"
        );
    }

    #[test]
    fn import_builds_the_fact_and_truncates() {
        let fake = Fake::new(vec![
            ("message list", Ok(LIST)),
            ("message get", Ok(r#"{"text":"0123456789abcdef"}"#)),
        ]);
        let importer = Email::with_runner("", "", 10, &fake);
        let session = &importer.discover_sessions(since()).unwrap()[0];
        let fact = &importer.import_session(session).unwrap()[0];
        assert_eq!(
            fact.content,
            "Email from Ann <ann@example.test>: Plan\n\n0123456789..."
        );
        assert_eq!(
            fact.timestamp,
            Utc.with_ymd_and_hms(2026, 7, 2, 10, 0, 0).unwrap()
        );
        assert_eq!(fact.metadata["message_id"], "1");
        assert_eq!(fact.metadata["folder"], "INBOX");
        assert_eq!(
            fake.calls.borrow()[1],
            "himalaya message get --folder INBOX --json 1"
        );
        assert_eq!(
            (importer.category(), importer.privacy_level()),
            (Some("communication"), Some(PrivacyLevel::Confidential))
        );
    }

    #[test]
    fn failures_are_wrapped() {
        let failing = Fake::new(vec![("message list", Err("exit status 1"))]);
        let error = Email::with_runner("", "", 0, &failing)
            .discover_sessions(since())
            .unwrap_err();
        assert_eq!(error, "himalaya list failed: exit status 1");
        let garbage = Fake::new(vec![
            ("message list", Ok("nope")),
            ("message get", Ok("[]")),
        ]);
        let importer = Email::with_runner("", "", 0, &garbage);
        assert!(
            importer
                .discover_sessions(since())
                .unwrap_err()
                .starts_with("failed to parse himalaya output: ")
        );
        let session = SessionRef {
            session_id: "9".into(),
            ..SessionRef::default()
        };
        assert!(
            importer
                .import_session(&session)
                .unwrap_err()
                .starts_with("failed to parse himalaya body: ")
        );
    }
}
