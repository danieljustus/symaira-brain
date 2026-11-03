//! CLI entity linking preserves Go's name/alias precedence and best-effort writes.

use crate::set_pipeline::Bytes;
use rusqlite::{Connection, OptionalExtension, params};

/// Links `names` to `memory` for the service paths that hold valid text.
pub(crate) fn link(conn: &Connection, memory: &str, names: &[String], author: &str) {
    let names: Vec<Vec<u8>> = names
        .iter()
        .map(|name| name.trim().as_bytes().to_vec())
        .filter(|name| !name.is_empty())
        .collect();
    link_bytes(conn, memory, &names, author.as_bytes());
}

/// `memory.Store`'s entity loop over byte-exact names: resolve (name, then
/// alias), create an `other` entity when unknown, link. Any failure skips
/// that entity without failing the write.
pub(crate) fn link_bytes(conn: &Connection, memory: &str, names: &[Vec<u8>], author: &[u8]) {
    for name in names.iter().filter(|name| !name.is_empty()) {
        let Ok(found) = resolve(conn, name) else {
            continue;
        };
        let id = if let Some(id) = found {
            id
        } else {
            let id = crate::write::new_uuid();
            let created_at = crate::gotime::format(chrono::Utc::now());
            let updated_at = crate::gotime::format(chrono::Utc::now());
            if conn.execute("INSERT INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,CAST(? AS TEXT),'other','[]','',CAST(? AS TEXT),?,?)",
                    params![id, Bytes(name), Bytes(author), created_at, updated_at]).is_err() { continue; }
            let detail = format!(
                "{{\"name\":{},\"type\":\"other\"}}",
                crate::gotext::json_string(name)
            );
            let _ = conn.execute("INSERT INTO audit_log(id,action,target_type,target_id,actor,detail,created_at) VALUES(?,'entity_create','entity',?,CAST(? AS TEXT),?,?)",
                    params![crate::write::new_uuid(), id, crate::set_pipeline::NullBytes(author), detail, crate::gotime::format(chrono::Utc::now())]);
            id
        };
        let _ = conn.execute(
            "INSERT OR IGNORE INTO memory_entities(memory_id,entity_id) VALUES(?,?)",
            params![memory, id],
        );
    }
}

/// Go scans a column into a `string` from text, blob and numbers, not NULL.
fn scannable(row: &rusqlite::Row<'_>, field: &str) -> rusqlite::Result<()> {
    match row.get_ref(field)? {
        rusqlite::types::ValueRef::Null => Err(rusqlite::Error::InvalidQuery),
        _ => Ok(()),
    }
}

/// `ResolveEntity` plus the `Scan` of the whole row: an unreadable row is an
/// error, which the caller treats as "skip this entity".
fn resolve(conn: &Connection, name: &[u8]) -> rusqlite::Result<Option<String>> {
    for query in [
        "SELECT * FROM entities WHERE name=CAST(? AS TEXT) COLLATE NOCASE",
        "SELECT e.* FROM entities e JOIN entities_aliases ea ON ea.entity_id=e.id WHERE ea.alias=CAST(? AS TEXT) COLLATE NOCASE",
    ] {
        let found: Option<String> = conn
            .query_row(query, [Bytes(name)], |row| {
                for field in ["id", "name", "type", "aliases", "description", "created_by"] {
                    scannable(row, field)?;
                }
                for field in ["created_at", "updated_at"] {
                    if !crate::cli_delete_admission::canonical_time(&row.get::<_, String>(field)?) {
                        return Err(rusqlite::Error::InvalidQuery);
                    }
                }
                let aliases: Vec<u8> = row
                    .get_ref("aliases")?
                    .as_bytes()
                    .map(<[u8]>::to_vec)
                    .unwrap_or_default();
                if serde_json::from_slice::<Option<Vec<Option<String>>>>(&aliases).is_err() {
                    return Err(rusqlite::Error::InvalidQuery);
                }
                row.get::<_, String>("id")
            })
            .optional()?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}

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
