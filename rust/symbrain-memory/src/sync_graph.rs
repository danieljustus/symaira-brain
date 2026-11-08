//! The entity graph and evidence that travel with memories in a full-fidelity
//! sync replica (#855).
//!
//! The wire is additive: Go-era peers ignore the members below. A memory
//! carries `entities` (linked entity names, the member `db.Memory` always had)
//! and `evidence`; the `entity_graph` envelope carries entities and relations
//! changed since the sync cursor. Relations reference their endpoints by
//! entity *name* because the name is the unique key (`NOCASE`) while ids can
//! differ between independently created stores.

use crate::evidence_store::EvidenceSpan;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// One `entities` row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityRecord {
    pub id: String,
    pub name: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(default = "empty_list")]
    pub aliases: serde_json::Value,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

fn empty_list() -> serde_json::Value {
    serde_json::Value::Array(Vec::new())
}

/// One `entity_relations` row, endpoints by entity name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationRecord {
    pub id: String,
    pub from: String,
    pub to: String,
    pub relation_type: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_ref: String,
    #[serde(default)]
    pub verification: String,
    #[serde(default)]
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
}

/// Entities and relations changed since a cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphDelta {
    #[serde(default)]
    pub entities: Vec<EntityRecord>,
    #[serde(default)]
    pub relations: Vec<RelationRecord>,
}

impl GraphDelta {
    /// Redacts free text the same way memory content is redacted on ingest
    /// and egress of the serving peer.
    pub fn redact_pii(&mut self) {
        for entity in &mut self.entities {
            entity.description = crate::pii::redact(&entity.description);
        }
        for relation in &mut self.relations {
            relation.evidence = crate::pii::redact(&relation.evidence);
        }
    }

    /// Whether nothing needs to be sent.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.relations.is_empty()
    }

    /// The JSON value of the `entity_graph` member.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Reads an `entity_graph` member; anything malformed is an empty delta.
    #[must_use]
    pub fn from_value(value: Option<&serde_json::Value>) -> Self {
        value
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default()
    }
}

/// What a single memory carries besides its row: linked entity names and
/// grounded evidence spans.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryExtras {
    pub entities: Vec<String>,
    pub evidence: Vec<EvidenceSpan>,
}

impl MemoryExtras {
    /// Redacts evidence text like memory content.
    pub fn redact_pii(&mut self) {
        for span in &mut self.evidence {
            span.text = crate::pii::redact(&span.text);
            span.evidence_text = crate::pii::redact(&span.evidence_text);
        }
    }

    /// Whether the memory carries nothing extra.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty() && self.evidence.is_empty()
    }

    /// JSON members (`"entities":[…]`, `"evidence":[…]`) for a memory object;
    /// empty parts are omitted, exactly like the Go `omitempty` fields.
    #[must_use]
    pub fn members(&self) -> Vec<String> {
        let mut members = Vec::new();
        if !self.entities.is_empty() {
            members.push(format!(
                "\"entities\":{}",
                serde_json::to_string(&self.entities).unwrap_or_else(|_| "[]".to_owned())
            ));
        }
        if !self.evidence.is_empty() {
            members.push(format!(
                "\"evidence\":{}",
                serde_json::to_string(&self.evidence).unwrap_or_else(|_| "[]".to_owned())
            ));
        }
        members
    }

    /// Reads the `entities` and `evidence` members of a memory object.
    #[must_use]
    pub fn from_object(object: &serde_json::Map<String, serde_json::Value>) -> Self {
        let entities = object
            .get("entities")
            .and_then(|value| serde_json::from_value::<Vec<String>>(value.clone()).ok())
            .unwrap_or_default();
        let evidence = object
            .get("evidence")
            .and_then(|value| serde_json::from_value::<Vec<EvidenceSpan>>(value.clone()).ok())
            .unwrap_or_default();
        Self { entities, evidence }
    }

    /// Appends the extras to a rendered memory object.
    #[must_use]
    pub fn splice(&self, object_json: String) -> String {
        let members = self.members();
        match (members.is_empty(), object_json.strip_suffix('}')) {
            (false, Some(head)) => format!("{head},{}}}", members.join(",")),
            _ => object_json,
        }
    }
}

/// An instant as RFC 3339 UTC text, or the raw text when it is not a time.
pub(crate) fn wire_time(raw: &str) -> String {
    crate::gotime::parse(raw).map_or_else(
        || raw.to_owned(),
        |time| time.to_rfc3339_opts(SecondsFormat::AutoSi, true),
    )
}

/// The stored spelling of a wire instant.
pub(crate) fn stored_time(raw: &str) -> String {
    crate::gotime::parse(raw).map_or_else(|| raw.to_owned(), crate::gotime::format)
}

fn optional_wire(value: Option<String>) -> Option<String> {
    value
        .filter(|text| !text.is_empty())
        .map(|text| wire_time(&text))
}

/// Entities and relations whose own timestamp is after `since`, plus the
/// endpoint entities of every relation so the receiver can resolve names.
///
/// # Errors
/// The SQLite error.
pub(crate) fn graph_since(conn: &Connection, since: &str) -> rusqlite::Result<GraphDelta> {
    let mut delta = GraphDelta::default();
    let mut seen = BTreeSet::new();
    let mut statement = conn.prepare(
        "SELECT id,name,type,aliases,description,created_by,created_at,updated_at FROM entities WHERE symbrain_sync_time(updated_at) > symbrain_sync_time(?) ORDER BY name COLLATE NOCASE, id",
    )?;
    let rows = statement.query_map([since], entity_row)?;
    for row in rows {
        let entity = row?;
        seen.insert(entity.name.to_lowercase());
        delta.entities.push(entity);
    }
    let mut statement = conn.prepare(
        "SELECT r.id,e1.name,e2.name,r.relation_type,r.created_by,r.created_at,r.source,r.source_ref,r.verification,r.evidence,r.updated_at,r.valid_from,r.valid_until FROM entity_relations r JOIN entities e1 ON e1.id=r.from_entity_id JOIN entities e2 ON e2.id=r.to_entity_id WHERE symbrain_sync_time(COALESCE(r.updated_at,r.created_at)) > symbrain_sync_time(?) ORDER BY r.id",
    )?;
    let rows = statement.query_map([since], |row| {
        Ok(RelationRecord {
            id: row.get(0)?,
            from: row.get(1)?,
            to: row.get(2)?,
            relation_type: row.get(3)?,
            created_by: row.get(4)?,
            created_at: wire_time(&row.get::<_, String>(5)?),
            source: row.get(6)?,
            source_ref: row.get(7)?,
            verification: row.get(8)?,
            evidence: row.get(9)?,
            updated_at: optional_wire(row.get(10)?),
            valid_from: optional_wire(row.get(11)?),
            valid_until: optional_wire(row.get(12)?),
        })
    })?;
    for row in rows {
        delta.relations.push(row?);
    }
    let mut missing = BTreeSet::new();
    for relation in &delta.relations {
        for name in [&relation.from, &relation.to] {
            if !seen.contains(&name.to_lowercase()) {
                missing.insert(name.clone());
            }
        }
    }
    for name in missing {
        if !seen.insert(name.to_lowercase()) {
            continue;
        }
        let mut statement = conn.prepare(
            "SELECT id,name,type,aliases,description,created_by,created_at,updated_at FROM entities WHERE name=? COLLATE NOCASE",
        )?;
        for row in statement.query_map([&name], entity_row)? {
            delta.entities.push(row?);
        }
    }
    Ok(delta)
}

fn entity_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<EntityRecord> {
    let aliases: String = row.get(3)?;
    Ok(EntityRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        kind: row.get(2)?,
        aliases: serde_json::from_str(&aliases).unwrap_or_else(|_| empty_list()),
        description: row.get(4)?,
        created_by: row.get(5)?,
        created_at: wire_time(&row.get::<_, String>(6)?),
        updated_at: wire_time(&row.get::<_, String>(7)?),
    })
}

/// Linked entity names and evidence spans of each memory in `ids`.
///
/// # Errors
/// The SQLite error.
pub(crate) fn extras_for(
    conn: &Connection,
    ids: &[String],
) -> rusqlite::Result<BTreeMap<String, MemoryExtras>> {
    let mut result = BTreeMap::new();
    let mut names = conn.prepare(
        "SELECT e.name FROM memory_entities me JOIN entities e ON e.id=me.entity_id WHERE me.memory_id=? ORDER BY e.name COLLATE NOCASE",
    )?;
    let mut spans = conn.prepare(
        "SELECT id,memory_id,source_id,source_kind,text,evidence_text,char_start,char_end,alignment_status,created_at FROM memory_evidence WHERE memory_id=? ORDER BY created_at ASC, id ASC",
    )?;
    for id in ids {
        let entities = names
            .query_map([id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let evidence = spans
            .query_map(params![id], |row| {
                let created: String = row.get(9)?;
                Ok(EvidenceSpan {
                    id: row.get(0)?,
                    memory_id: row.get(1)?,
                    source_id: row.get(2)?,
                    source_kind: row.get(3)?,
                    text: row.get(4)?,
                    evidence_text: row.get(5)?,
                    char_start: row.get(6)?,
                    char_end: row.get(7)?,
                    alignment_status: row.get(8)?,
                    created_at: crate::gotime::parse(&created)
                        .unwrap_or(DateTime::<Utc>::UNIX_EPOCH),
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let extras = MemoryExtras { entities, evidence };
        if !extras.is_empty() {
            result.insert(id.clone(), extras);
        }
    }
    Ok(result)
}
