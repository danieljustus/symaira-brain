//! `OpenCode` importer (ported for #761 from `importer/opencode/opencode.go` at
//! v0.12.0): reads the `OpenCode` SQLite database (read-only here) and imports
//! assistant text parts. Sessions are tagged `prompt_mode=code` so
//! consolidation uses the code prompt family (#483).

use super::util::{from_millis, home_dir};
use super::{ImportResult, ImportedFact, PrivacyLevel, SessionImporter, SessionRef};
use crate::consolidation_prompts::{PROMPT_FAMILY_CODE, PROMPT_MODE_KEY};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Imports sessions from the `OpenCode` database.
pub struct OpenCode {
    custom_path: String,
}

impl OpenCode {
    /// `custom_path` empty means `$HOME/.local/share/opencode/opencode.db`.
    #[must_use]
    pub fn new(custom_path: &str) -> Self {
        Self {
            custom_path: custom_path.to_owned(),
        }
    }

    /// `dbPath`: empty when `$HOME` cannot be determined.
    fn db_path(&self) -> String {
        if !self.custom_path.is_empty() {
            return self.custom_path.clone();
        }
        home_dir().map_or_else(
            |_| String::new(),
            |h| {
                h.join(".local/share/opencode/opencode.db")
                    .to_string_lossy()
                    .into_owned()
            },
        )
    }
}

fn open(path: &str) -> Result<Connection, String> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("failed to open opencode database: {e}"))
}

#[derive(Deserialize, Default)]
struct MessageTime {
    #[serde(default)]
    created: i64,
}

#[derive(Deserialize, Default)]
struct MessageData {
    #[serde(default)]
    role: String,
    #[serde(default)]
    time: MessageTime,
    #[serde(default, rename = "modelID")]
    model_id: String,
    #[serde(default, rename = "providerID")]
    provider_id: String,
}

#[derive(Deserialize, Default)]
struct PartData {
    #[serde(default)]
    text: String,
}

fn message_text(conn: &Connection, message_id: &str) -> Result<String, rusqlite::Error> {
    let mut statement = conn.prepare(
        "SELECT data FROM part WHERE message_id = ? AND json_extract(data, '$.type') = 'text' ORDER BY time_created ASC",
    )?;
    let rows = statement.query_map([message_id], |r| r.get::<_, String>(0))?;
    let texts: Vec<String> = rows
        .filter_map(Result::ok)
        .filter_map(|json| super::parse_object::<PartData>(json.as_bytes()).ok())
        .map(|p| p.text.trim().to_owned())
        .filter(|t| !t.is_empty())
        .collect();
    Ok(texts.join("\n"))
}

impl SessionImporter for OpenCode {
    fn name(&self) -> &'static str {
        "opencode"
    }
    fn category(&self) -> Option<&str> {
        Some("code")
    }
    fn privacy_level(&self) -> Option<PrivacyLevel> {
        Some(PrivacyLevel::Internal)
    }
    fn is_transcript(&self) -> bool {
        true
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let path = self.db_path();
        if path.is_empty() || !std::path::Path::new(&path).exists() {
            return Ok(Vec::new());
        }
        let conn = open(&path)?;
        let wrap = |e: rusqlite::Error| format!("failed to query sessions: {e}");
        let mut statement = conn
            .prepare("SELECT id, title, directory, time_created, time_updated, agent, model FROM session WHERE time_updated > ? ORDER BY time_updated DESC")
            .map_err(wrap)?;
        let rows = statement
            .query_map([since.timestamp_millis()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                    r.get::<_, String>(6)?,
                ))
            })
            .map_err(wrap)?;
        let mut sessions = Vec::new();
        for (id, title, directory, updated, agent, model) in rows.filter_map(Result::ok) {
            let mut metadata = BTreeMap::from([
                ("title".to_owned(), title),
                ("directory".to_owned(), directory),
                ("agent".to_owned(), agent),
                (PROMPT_MODE_KEY.to_owned(), PROMPT_FAMILY_CODE.to_owned()),
            ]);
            if !model.is_empty() {
                metadata.insert("model".into(), model);
            }
            sessions.push(SessionRef {
                tool: "opencode".into(),
                session_id: id,
                path: path.clone(),
                modified_at: from_millis(updated),
                metadata,
            });
        }
        Ok(sessions)
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let conn = open(&session.path)?;
        let (title, directory, agent): (String, String, String) = conn
            .query_row(
                "SELECT title, directory, agent FROM session WHERE id = ?",
                [&session.session_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|e| format!("failed to query session: {e}"))?;
        let wrap = |e: rusqlite::Error| format!("failed to query messages: {e}");
        let mut statement = conn
            .prepare("SELECT id, data FROM message WHERE session_id = ? ORDER BY time_created ASC")
            .map_err(wrap)?;
        let rows = statement
            .query_map([&session.session_id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(wrap)?;
        let mut facts = Vec::new();
        for (id, data) in rows.filter_map(Result::ok) {
            let Ok(data) = super::parse_object::<MessageData>(data.as_bytes()) else {
                continue;
            };
            if data.role != "assistant" {
                continue;
            }
            let Ok(text) = message_text(&conn, &id) else {
                continue;
            };
            if text.len() < 50 {
                continue;
            }
            let timestamp = if data.time.created > 0 {
                from_millis(data.time.created)
            } else {
                crate::sync_state::zero_time()
            };
            let model = match (data.model_id.is_empty(), data.provider_id.is_empty()) {
                (true, _) => String::new(),
                (false, true) => data.model_id,
                (false, false) => format!("{}/{}", data.provider_id, data.model_id),
            };
            facts.push(ImportedFact {
                content: text,
                source: "opencode".into(),
                session_id: session.session_id.clone(),
                timestamp,
                metadata: BTreeMap::from([
                    ("title".to_owned(), title.clone()),
                    ("directory".to_owned(), directory.clone()),
                    ("agent".to_owned(), agent.clone()),
                    ("model".to_owned(), model),
                    ("message_id".to_owned(), id),
                ]),
                evidence: Vec::new(),
            });
        }
        Ok(facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::test_support::TestDir;
    use chrono::TimeZone;
    use rusqlite::params;

    fn ms(day: u32) -> i64 {
        Utc.with_ymd_and_hms(2026, 6, day, 12, 0, 0)
            .unwrap()
            .timestamp_millis()
    }

    fn fixture(dir: &TestDir) -> String {
        let path = dir.path().join("opencode.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE session(id TEXT, title TEXT, directory TEXT, time_created INTEGER, time_updated INTEGER, agent TEXT, model TEXT);
             CREATE TABLE message(id TEXT, session_id TEXT, time_created INTEGER, data TEXT);
             CREATE TABLE part(id TEXT, message_id TEXT, time_created INTEGER, data TEXT);",
        )
        .unwrap();
        for (id, title, updated, model) in [
            ("ses_old", "Old", ms(1), ""),
            ("ses_new", "Refactor", ms(20), "gpt-x"),
        ] {
            conn.execute(
                "INSERT INTO session VALUES(?,?,?,?,?,?,?)",
                params![id, title, "/work/p", ms(1), updated, "build", model],
            )
            .unwrap();
        }
        let parts = [
            ("m1", "user", 0, vec!["hello there"]),
            (
                "m2",
                "assistant",
                ms(20),
                vec![
                    "First paragraph of a fairly long answer",
                    "  second paragraph that adds even more text  ",
                    "",
                ],
            ),
            ("m3", "assistant", 0, vec!["too short"]),
        ];
        for (i, (mid, role, created, texts)) in parts.iter().enumerate() {
            let data = format!(
                "{{\"role\":\"{role}\",\"time\":{{\"created\":{created}}},\"modelID\":\"sonnet\",\"providerID\":\"anthropic\"}}"
            );
            conn.execute(
                "INSERT INTO message VALUES(?,?,?,?)",
                params![mid, "ses_new", i64::try_from(i).unwrap(), data],
            )
            .unwrap();
            for (j, text) in texts.iter().enumerate() {
                let part = format!("{{\"type\":\"text\",\"text\":\"{text}\"}}");
                conn.execute(
                    "INSERT INTO part VALUES(?,?,?,?)",
                    params![format!("{mid}-{j}"), mid, i64::try_from(j).unwrap(), part],
                )
                .unwrap();
            }
            conn.execute("INSERT INTO part VALUES(?,?,?,?)", params![format!("{mid}-tool"), mid, 99, "{\"type\":\"tool\",\"text\":\"ignored tool output that is long enough to count\"}"]).unwrap();
        }
        conn.execute(
            "INSERT INTO message VALUES('m4','ses_new',9,'not json')",
            [],
        )
        .unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn discovers_and_imports() {
        let dir = TestDir::new("opencode");
        let path = fixture(&dir);
        let importer = OpenCode::new(&path);
        let sessions = importer
            .discover_sessions(Utc.with_ymd_and_hms(2026, 6, 10, 0, 0, 0).unwrap())
            .unwrap();
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(
            (s.session_id.as_str(), s.modified_at.timestamp_millis()),
            ("ses_new", ms(20))
        );
        assert_eq!(s.metadata["prompt_mode"], "code");
        assert_eq!(s.metadata["model"], "gpt-x");
        let facts = importer.import_session(s).unwrap();
        assert_eq!(
            facts.len(),
            1,
            "user, short and unparsable messages are skipped"
        );
        assert_eq!(
            facts[0].content,
            "First paragraph of a fairly long answer\nsecond paragraph that adds even more text"
        );
        assert_eq!(facts[0].metadata["model"], "anthropic/sonnet");
        assert_eq!(facts[0].metadata["message_id"], "m2");
        assert_eq!(facts[0].timestamp.timestamp_millis(), ms(20));
        assert_eq!(
            (
                importer.category(),
                importer.privacy_level(),
                importer.is_transcript()
            ),
            (Some("code"), Some(PrivacyLevel::Internal), true)
        );
        let all = importer
            .discover_sessions(crate::sync_state::zero_time())
            .unwrap();
        assert_eq!(all.len(), 2);
        assert!(
            !all[1].metadata.contains_key("model"),
            "empty model omitted"
        );
    }

    #[test]
    fn missing_database_and_unknown_session() {
        let dir = TestDir::new("opencode-none");
        let missing = dir.path().join("x.db").to_string_lossy().into_owned();
        assert!(
            OpenCode::new(&missing)
                .discover_sessions(crate::sync_state::zero_time())
                .unwrap()
                .is_empty()
        );
        let path = fixture(&dir);
        let session = SessionRef {
            path,
            session_id: "nope".into(),
            ..SessionRef::default()
        };
        assert!(
            OpenCode::new("")
                .import_session(&session)
                .unwrap_err()
                .starts_with("failed to query session: ")
        );
    }
}
