//! Applying the entity graph and per-memory extras of a full-fidelity sync
//! (#855) to a local store.
//!
//! Merging is additive and idempotent: rows are matched by their natural key
//! (entity name, relation `(from, to, type)`, evidence id, link), a newer
//! `updated_at` wins, and nothing here deletes. Unlinking or removing a single
//! relation is therefore not replicated; deleting a memory (which cascades to
//! its links and evidence) is.

use crate::entity::stable_id;
use crate::evidence_store::EvidenceSpan;
use crate::sync_graph::{EntityRecord, GraphDelta, MemoryExtras, RelationRecord, stored_time};
use rusqlite::{Connection, OptionalExtension, params};

/// What one application changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GraphApplied {
    pub entities: u32,
    pub relations: u32,
    pub links: u32,
    pub evidence: u32,
}

fn instant(raw: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    crate::gotime::parse(raw)
}

/// Whether `incoming` is strictly newer than `existing` (stored spelling).
fn newer(incoming: &str, existing: &str) -> bool {
    match (instant(incoming), instant(existing)) {
        (Some(new), Some(old)) => new > old,
        (Some(_), None) => true,
        _ => false,
    }
}

fn entity_id_by_name(conn: &Connection, name: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT id FROM entities WHERE name=? COLLATE NOCASE",
        [name],
        |row| row.get(0),
    )
    .optional()
}

/// The id of the entity called `name`, creating a minimal one when absent.
fn ensure_entity(conn: &Connection, name: &str) -> rusqlite::Result<String> {
    if let Some(id) = entity_id_by_name(conn, name)? {
        return Ok(id);
    }
    let id = stable_id(&["entity", &name.to_lowercase()]);
    let now = crate::gotime::format(chrono::Utc::now());
    conn.execute(
        "INSERT OR IGNORE INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
        params![id, name, "other", "[]", "", "sync", now, now],
    )?;
    Ok(entity_id_by_name(conn, name)?.unwrap_or(id))
}

fn write_aliases(conn: &Connection, id: &str, aliases: &serde_json::Value) {
    for alias in aliases
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a.as_str())
    {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO entities_aliases(entity_id,alias) VALUES(?,?)",
            params![id, alias],
        );
    }
}

fn apply_entity(conn: &Connection, entity: &EntityRecord) -> rusqlite::Result<bool> {
    if entity.name.trim().is_empty() {
        return Ok(false);
    }
    let aliases = entity.aliases.to_string();
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT id,updated_at FROM entities WHERE name=? COLLATE NOCASE",
            [&entity.name],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let incoming = stored_time(&entity.updated_at);
    match existing {
        None => {
            let created = if entity.created_at.is_empty() {
                incoming.clone()
            } else {
                stored_time(&entity.created_at)
            };
            let id = if entity.id.is_empty() {
                stable_id(&["entity", &entity.name.to_lowercase()])
            } else {
                entity.id.clone()
            };
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
                params![id, entity.name, entity.kind, aliases, entity.description, entity.created_by, created, incoming],
            )?;
            if inserted > 0 {
                write_aliases(conn, &id, &entity.aliases);
            }
            Ok(inserted > 0)
        }
        Some((id, stored)) if newer(&incoming, &stored) => {
            conn.execute(
                "UPDATE entities SET type=?,aliases=?,description=?,updated_at=? WHERE id=?",
                params![entity.kind, aliases, entity.description, incoming, id],
            )?;
            write_aliases(conn, &id, &entity.aliases);
            Ok(true)
        }
        Some(_) => Ok(false),
    }
}

fn apply_relation(conn: &Connection, relation: &RelationRecord) -> rusqlite::Result<bool> {
    if relation.from.trim().is_empty()
        || relation.to.trim().is_empty()
        || relation.relation_type.is_empty()
    {
        return Ok(false);
    }
    let from = ensure_entity(conn, &relation.from)?;
    let to = ensure_entity(conn, &relation.to)?;
    let created = stored_time(&relation.created_at);
    let updated = relation
        .updated_at
        .as_deref()
        .map_or_else(|| created.clone(), stored_time);
    let optional =
        |value: &Option<String>| value.as_deref().filter(|t| !t.is_empty()).map(stored_time);
    let existing: Option<Option<String>> = conn
        .query_row(
            "SELECT COALESCE(updated_at,created_at) FROM entity_relations WHERE from_entity_id=? AND to_entity_id=? AND relation_type=?",
            params![from, to, relation.relation_type],
            |row| row.get(0),
        )
        .optional()?;
    match existing {
        None => {
            let id = if relation.id.is_empty() {
                stable_id(&["relation", &from, &to, &relation.relation_type])
            } else {
                relation.id.clone()
            };
            let inserted = conn.execute(
                "INSERT OR IGNORE INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_by,created_at,id,source,source_ref,verification,evidence,updated_at,valid_from,valid_until) VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
                params![from, to, relation.relation_type, relation.created_by, created, id, relation.source, relation.source_ref, relation.verification, relation.evidence, updated, optional(&relation.valid_from), optional(&relation.valid_until)],
            )?;
            Ok(inserted > 0)
        }
        Some(stored) if newer(&updated, stored.as_deref().unwrap_or_default()) => {
            conn.execute(
                "UPDATE entity_relations SET source=?,source_ref=?,verification=?,evidence=?,updated_at=?,valid_from=?,valid_until=? WHERE from_entity_id=? AND to_entity_id=? AND relation_type=?",
                params![relation.source, relation.source_ref, relation.verification, relation.evidence, updated, optional(&relation.valid_from), optional(&relation.valid_until), from, to, relation.relation_type],
            )?;
            Ok(true)
        }
        Some(_) => Ok(false),
    }
}

/// Applies the graph envelope: entities first, then the relations that name
/// them.
///
/// # Errors
/// The SQLite error; nothing is kept on failure.
pub(crate) fn apply_graph(conn: &Connection, delta: &GraphDelta) -> rusqlite::Result<GraphApplied> {
    let mut applied = GraphApplied::default();
    if delta.is_empty() {
        return Ok(applied);
    }
    let transaction = conn.unchecked_transaction()?;
    for entity in &delta.entities {
        if apply_entity(&transaction, entity)? {
            applied.entities += 1;
        }
    }
    for relation in &delta.relations {
        if apply_relation(&transaction, relation)? {
            applied.relations += 1;
        }
    }
    transaction.commit()?;
    Ok(applied)
}

fn memory_exists(conn: &Connection, id: &str) -> rusqlite::Result<bool> {
    conn.query_row("SELECT 1 FROM memories WHERE id=?", [id], |_| Ok(()))
        .optional()
        .map(|found| found.is_some())
}

fn apply_evidence(conn: &Connection, span: &EvidenceSpan) -> rusqlite::Result<bool> {
    let created = stored_time(&span.created_at.to_rfc3339());
    conn.execute(
        "INSERT OR IGNORE INTO memory_evidence(id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at) VALUES(?,?,?,?,?,?,?,?,?,?)",
        params![span.id, span.memory_id, span.source_id, span.source_kind, span.text, span.evidence_text, span.char_start, span.char_end, span.alignment_status, created],
    )
    .map(|rows| rows > 0)
}

/// Links `memory_id` to its entities and adds its evidence spans. A memory
/// that is not present locally (skipped by a tombstone) takes nothing.
///
/// # Errors
/// The SQLite error; nothing is kept on failure.
pub(crate) fn apply_extras(
    conn: &Connection,
    memory_id: &str,
    extras: &MemoryExtras,
) -> rusqlite::Result<GraphApplied> {
    let mut applied = GraphApplied::default();
    if extras.is_empty() || !memory_exists(conn, memory_id)? {
        return Ok(applied);
    }
    let transaction = conn.unchecked_transaction()?;
    for name in extras
        .entities
        .iter()
        .filter(|name| !name.trim().is_empty())
    {
        let entity = ensure_entity(&transaction, name)?;
        let linked = transaction.execute(
            "INSERT OR IGNORE INTO memory_entities(memory_id,entity_id) VALUES(?,?)",
            params![memory_id, entity],
        )?;
        applied.links += u32::try_from(linked).unwrap_or(0);
    }
    for span in &extras.evidence {
        let mut span = span.clone();
        memory_id.clone_into(&mut span.memory_id);
        if apply_evidence(&transaction, &span)? {
            applied.evidence += 1;
        }
    }
    transaction.commit()?;
    Ok(applied)
}
