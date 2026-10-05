//! CLI entity linking preserves Go's name/alias precedence and best-effort writes.

use rusqlite::{Connection, OptionalExtension, params};

pub(crate) fn link(conn: &Connection, memory: &str, names: &[String], author: &str) {
    for name in names
        .iter()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty())
    {
        let Ok(found) = resolve(conn, name) else {
            continue;
        };
        let id = if let Some(id) = found {
            id
        } else {
            let id = crate::write::new_uuid();
            let created_at = crate::gotime::format(chrono::Utc::now());
            let updated_at = crate::gotime::format(chrono::Utc::now());
            if conn.execute("INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,'other','[]','',?,?,?)",
                    params![id, name, author, created_at, updated_at]).is_err() { continue; }
            let detail =
                crate::cli_write::go_json(&serde_json::json!({"name": name, "type": "other"}))
                    .unwrap_or_default();
            let actor = (!author.is_empty()).then_some(author);
            let _ = conn.execute("INSERT INTO audit_log(id,action,target_type,target_id,actor,detail,created_at) VALUES(?,'entity_create','entity',?,?,?,?)",
                    params![crate::write::new_uuid(), id, actor, detail, crate::gotime::format(chrono::Utc::now())]);
            id
        };
        let _ = conn.execute(
            "INSERT OR IGNORE INTO memory_entities(memory_id,entity_id) VALUES(?,?)",
            params![memory, id],
        );
    }
}

fn resolve(conn: &Connection, name: &str) -> rusqlite::Result<Option<String>> {
    for query in [
        "SELECT id,aliases FROM entities WHERE name=? COLLATE NOCASE",
        "SELECT e.id,e.aliases FROM entities e JOIN entities_aliases ea ON ea.entity_id=e.id WHERE ea.alias=? COLLATE NOCASE",
    ] {
        let found: Option<(String, String)> = conn
            .query_row(query, [name], |row| Ok((row.get(0)?, row.get(1)?)))
            .optional()?;
        if let Some((id, aliases)) = found {
            // A malformed alias payload makes Go skip this entity entirely.
            if serde_json::from_str::<Option<Vec<Option<String>>>>(&aliases).is_err() {
                return Err(rusqlite::Error::InvalidQuery);
            }
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Checks that existing name/alias matches have canonical Go timestamp/UTF8
/// hydration. Other representations retain Go until their decoder is ported.
#[must_use]
pub fn direct_entities_supported(path: &std::path::Path, names: &[String]) -> bool {
    if names.is_empty() {
        return true;
    }
    let Ok(conn) = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
    else {
        return false;
    };
    for name in names {
        for query in [
            "SELECT e.* FROM entities e WHERE e.name=? COLLATE NOCASE",
            "SELECT e.* FROM entities e JOIN entities_aliases ea ON ea.entity_id=e.id WHERE ea.alias=? COLLATE NOCASE",
        ] {
            let found = conn
                .query_row(query, [name], |row| {
                    for field in ["id", "name", "type", "aliases", "description", "created_by"] {
                        let _: String = row.get(field)?;
                    }
                    Ok(["created_at", "updated_at"].into_iter().all(|field| {
                        row.get::<_, String>(field)
                            .is_ok_and(|value| crate::cli_delete_admission::canonical_time(&value))
                    }))
                })
                .optional();
            match found {
                Ok(None) => {}
                Ok(Some(true)) => break,
                _ => return false,
            }
        }
    }
    true
}
