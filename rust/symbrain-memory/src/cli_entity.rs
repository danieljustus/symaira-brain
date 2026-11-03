//! CLI entity linking preserves Go's name/alias precedence and best-effort writes.

use crate::set_pipeline::Bytes;
use rusqlite::{Connection, OptionalExtension, params};

/// `memory.Store`'s entity loop over byte-exact names: resolve (name, then
/// alias), create an `other` entity when unknown, link. Any failure skips
/// that entity without failing the write.
pub(crate) fn link_bytes(
    conn: &Connection,
    audit_enabled: bool,
    session: &str,
    memory: &str,
    names: &[Vec<u8>],
    author: &[u8],
) {
    for name in names {
        let name = crate::gotext::trim_space(name);
        if name.is_empty() {
            continue;
        }
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
            if audit_enabled {
                let _ = conn.execute("INSERT INTO audit_log(id,action,target_type,target_id,actor,detail,created_at) VALUES(?,'entity_create','entity',?,CAST(? AS TEXT),?,?)",
                    params![crate::write::new_uuid(), id, crate::set_pipeline::NullBytes(author), detail, crate::gotime::format(chrono::Utc::now())]);
            }
            id
        };
        let _ = session;
        let _ = conn.execute(
            "INSERT OR IGNORE INTO memory_entities(memory_id,entity_id) VALUES(?,?)",
            params![memory, id],
        );
    }
}

/// `ResolveEntity` plus the `Scan` of the whole row: an unreadable row is an
/// error, which the caller treats as "skip this entity".
pub(crate) fn resolve(conn: &Connection, name: &[u8]) -> rusqlite::Result<Option<String>> {
    for query in [
        "SELECT id, name, type, aliases, description, created_by, created_at, updated_at FROM entities WHERE name=CAST(? AS TEXT) COLLATE NOCASE",
        "SELECT e.id, e.name, e.type, e.aliases, e.description, e.created_by, e.created_at, e.updated_at FROM entities e JOIN entities_aliases ea ON ea.entity_id=e.id WHERE ea.alias=CAST(? AS TEXT) COLLATE NOCASE",
    ] {
        let found: Option<String> = conn
            .query_row(query, [Bytes(name)], |row| {
                use crate::goscan::{raw, text, time};
                let id = text(row, 0, "id")?;
                text(row, 1, "name")?;
                text(row, 2, "type")?;
                let aliases = raw(row, 3, "aliases")?;
                text(row, 4, "description")?;
                text(row, 5, "created_by")?;
                time(row, 6, "created_at")?;
                time(row, 7, "updated_at")?;
                crate::goscan::check_string_list(&aliases)?;
                Ok(id)
            })
            .optional()?;
        if found.is_some() {
            return Ok(found);
        }
    }
    Ok(None)
}
