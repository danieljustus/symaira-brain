//! Hermes Agent importer (ported for #761 from `importer/hermes/hermes.go` at
//! v0.12.0): reads a Hermes SQLite state database whose message table and
//! columns are discovered by naming pattern. The database is opened
//! read-only; the Go code opened it read-write.

use super::util::home_dir;
use super::{ImportResult, ImportedFact, SessionImporter, SessionRef};
use crate::gotime;
use chrono::{DateTime, Utc};
use rusqlite::types::Value;
use rusqlite::{Connection, OpenFlags};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Imports sessions from the Hermes state database.
pub struct Hermes {
    custom_path: String,
}

impl Hermes {
    /// `custom_path` empty means `$HOME/.hermes/state.db`.
    #[must_use]
    pub fn new(custom_path: &str) -> Self {
        Self {
            custom_path: custom_path.to_owned(),
        }
    }
}

#[derive(Default)]
struct Schema {
    session_id_col: String,
    messages_table: String,
    role_col: String,
    content_col: String,
    timestamp_col: String,
}

/// Quotes an identifier discovered in the database before it is spliced into
/// SQL (the Go code used the raw name).
fn ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn open(path: &Path) -> Result<Connection, String> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("failed to open hermes database: {e}"))
}

fn discover_schema(conn: &Connection) -> Result<Schema, String> {
    let tables: Vec<String> = {
        let mut statement = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            )
            .map_err(|e| e.to_string())?;
        statement
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect()
    };
    let mut schema = Schema::default();
    if let Some(table) = tables.iter().find(|t| {
        let lower = t.to_lowercase();
        lower.contains("message") || lower.contains("turn") || lower.contains("chat")
    }) {
        schema.messages_table.clone_from(table);
    } else if let Some(table) = tables
        .iter()
        .find(|t| *t != "sqlite_sequence" && *t != "sqlite_master")
    {
        schema.messages_table.clone_from(table);
    }
    if schema.messages_table.is_empty() {
        return Ok(schema);
    }
    let columns: Vec<String> = {
        let mut statement = conn
            .prepare(&format!(
                "PRAGMA table_info({})",
                ident(&schema.messages_table)
            ))
            .map_err(|e| e.to_string())?;
        statement
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .collect()
    };
    for col in columns {
        let lower = col.to_lowercase();
        if (lower.contains("session") || lower.contains("conversation") || lower.contains("thread"))
            && schema.session_id_col.is_empty()
        {
            schema.session_id_col.clone_from(&col);
        }
        if lower.contains("role") || lower == "sender" {
            schema.role_col.clone_from(&col);
        }
        if matches!(lower.as_str(), "content" | "text" | "body" | "message") {
            schema.content_col.clone_from(&col);
        }
        if matches!(
            lower.as_str(),
            "timestamp" | "created_at" | "time" | "ts" | "created"
        ) {
            schema.timestamp_col.clone_from(&col);
        }
    }
    for (slot, default) in [
        (&mut schema.role_col, "role"),
        (&mut schema.content_col, "content"),
        (&mut schema.timestamp_col, "timestamp"),
        (&mut schema.session_id_col, "session_id"),
    ] {
        if slot.is_empty() {
            *slot = default.into();
        }
    }
    Ok(schema)
}

impl SessionImporter for Hermes {
    fn name(&self) -> &'static str {
        "hermes"
    }
    fn is_transcript(&self) -> bool {
        true
    }
    fn discover_sessions(&self, since: DateTime<Utc>) -> ImportResult<Vec<SessionRef>> {
        let path: PathBuf = if self.custom_path.is_empty() {
            home_dir()?.join(".hermes").join("state.db")
        } else {
            PathBuf::from(&self.custom_path)
        };
        if !path.exists() {
            return Ok(Vec::new());
        }
        let conn = open(&path)?;
        let schema =
            discover_schema(&conn).map_err(|e| format!("failed to discover hermes schema: {e}"))?;
        if schema.messages_table.is_empty() || schema.session_id_col.is_empty() {
            return Ok(Vec::new());
        }
        let query = format!(
            "SELECT DISTINCT {s} FROM {t} WHERE {ts} > ? ORDER BY {ts}",
            s = ident(&schema.session_id_col),
            t = ident(&schema.messages_table),
            ts = ident(&schema.timestamp_col),
        );
        let mut statement = conn
            .prepare(&query)
            .map_err(|e| format!("failed to query sessions: {e}"))?;
        let rows = statement
            .query_map([gotime::format(since)], |r| r.get::<_, String>(0))
            .map_err(|e| format!("failed to query sessions: {e}"))?;
        Ok(rows
            .filter_map(Result::ok)
            .map(|session_id| SessionRef {
                tool: "hermes".into(),
                session_id,
                path: path.to_string_lossy().into_owned(),
                modified_at: Utc::now(),
                metadata: BTreeMap::new(),
            })
            .collect())
    }
    fn import_session(&self, session: &SessionRef) -> ImportResult<Vec<ImportedFact>> {
        let conn = open(Path::new(&session.path))?;
        let schema =
            discover_schema(&conn).map_err(|e| format!("failed to discover hermes schema: {e}"))?;
        if schema.messages_table.is_empty() {
            return Err("no messages table found in hermes database".into());
        }
        let query = format!(
            "SELECT {r}, {c}, {ts} FROM {t} WHERE {s} = ? ORDER BY {ts}",
            r = ident(&schema.role_col),
            c = ident(&schema.content_col),
            ts = ident(&schema.timestamp_col),
            t = ident(&schema.messages_table),
            s = ident(&schema.session_id_col),
        );
        let mut statement = conn
            .prepare(&query)
            .map_err(|e| format!("failed to query messages: {e}"))?;
        let rows = statement
            .query_map([&session.session_id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Value>(2)?,
                ))
            })
            .map_err(|e| format!("failed to query messages: {e}"))?;
        let mut facts = Vec::new();
        for (role, content, ts) in rows.filter_map(Result::ok) {
            // database/sql only scans a stored time.Time (text) into time.Time.
            let Value::Text(ts) = ts else { continue };
            let Some(timestamp) = gotime::parse(&ts) else {
                continue;
            };
            if role == "assistant" && content.len() > 50 {
                facts.push(ImportedFact {
                    content,
                    source: "hermes".into(),
                    session_id: session.session_id.clone(),
                    timestamp,
                    ..ImportedFact::default()
                });
            }
        }
        Ok(facts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::importer::test_support::TestDir;
    use chrono::TimeZone;

    const LONG: &str = "A long assistant reply that is comfortably above fifty bytes in size.";

    fn at(day: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, day, 8, 0, 0).unwrap()
    }

    fn fixture(dir: &TestDir, ddl: &str, rows: &[(&str, &str, &str, DateTime<Utc>)]) -> String {
        let path = dir.path().join("state.db");
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(ddl).unwrap();
        let table = if ddl.contains("chat_turns") {
            "chat_turns"
        } else {
            "messages"
        };
        let (sid, role, text, ts) = if table == "messages" {
            ("session_id", "role", "content", "timestamp")
        } else {
            ("conversation", "sender", "body", "created_at")
        };
        for (s, r, c, t) in rows {
            conn.execute(
                &format!("INSERT INTO {table}({sid},{role},{text},{ts}) VALUES(?,?,?,?)"),
                rusqlite::params![s, r, c, gotime::format(*t)],
            )
            .unwrap();
        }
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn standard_schema() {
        let dir = TestDir::new("hermes");
        let path = fixture(
            &dir,
            "CREATE TABLE messages(session_id TEXT, role TEXT, content TEXT, timestamp TEXT);",
            &[
                ("s1", "user", "hello", at(2)),
                ("s1", "assistant", LONG, at(3)),
                ("s1", "assistant", "short", at(4)),
                ("s2", "assistant", LONG, at(1)),
                ("s3", "assistant", LONG, at(10)),
            ],
        );
        let importer = Hermes::new(&path);
        let sessions = importer.discover_sessions(at(2)).unwrap();
        let ids: Vec<_> = sessions.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, ["s1", "s3"], "ordered by timestamp, s2 is too old");
        let facts = importer.import_session(&sessions[0]).unwrap();
        assert_eq!(facts.len(), 1);
        assert_eq!(
            (facts[0].content.as_str(), facts[0].timestamp),
            (LONG, at(3))
        );
        assert!(importer.is_transcript());
    }

    #[test]
    fn schema_is_discovered_by_name() {
        let dir = TestDir::new("hermes-alt");
        let path = fixture(
            &dir,
            "CREATE TABLE misc(x); CREATE TABLE chat_turns(conversation TEXT, sender TEXT, body TEXT, created_at TEXT);",
            &[("c1", "assistant", LONG, at(5))],
        );
        let importer = Hermes::new(&path);
        let sessions = importer.discover_sessions(at(1)).unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(importer.import_session(&sessions[0]).unwrap().len(), 1);
    }

    #[test]
    fn absent_or_empty_databases() {
        let dir = TestDir::new("hermes-none");
        let missing = dir.path().join("none.db").to_string_lossy().into_owned();
        assert!(
            Hermes::new(&missing)
                .discover_sessions(at(1))
                .unwrap()
                .is_empty()
        );
        let empty = dir.path().join("empty.db");
        Connection::open(&empty).unwrap();
        let empty = empty.to_string_lossy().into_owned();
        assert!(
            Hermes::new(&empty)
                .discover_sessions(at(1))
                .unwrap()
                .is_empty()
        );
        let session = SessionRef {
            path: empty,
            session_id: "x".into(),
            ..SessionRef::default()
        };
        assert_eq!(
            Hermes::new("").import_session(&session).unwrap_err(),
            "no messages table found in hermes database"
        );
    }
}
