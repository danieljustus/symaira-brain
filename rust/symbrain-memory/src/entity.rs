use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

use crate::model::{Store, StoreError};

impl Store {
    /// Lists entities in deterministic case-insensitive name order.
    ///
    /// # Errors
    /// Returns a `StoreError` when the SQLite query fails.
    pub fn entities(&self) -> Result<Vec<Value>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(
            "SELECT id,name,type,aliases,description,created_by,created_at,updated_at FROM entities ORDER BY name COLLATE NOCASE,id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?,
                "type": row.get::<_, String>(2)?,
                "aliases": serde_json::from_str::<Value>(&row.get::<_, String>(3)?).unwrap_or_else(|_| json!([])),
                "description": row.get::<_, String>(4)?, "created_by": row.get::<_, String>(5)?,
                "created_at": row.get::<_, String>(6)?, "updated_at": row.get::<_, String>(7)?
            }))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Returns recent query-log rows in deterministic reverse insertion order.
    ///
    /// # Errors
    /// Returns a `StoreError` when the SQLite query fails.
    pub fn query_log(&self, limit: usize) -> Result<Vec<Value>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn.prepare(
            "SELECT id,tool,query_text,params,duration_ms,created_at,actor,scope,session FROM query_log ORDER BY created_at DESC,id DESC LIMIT ?",
        )?;
        let rows = statement.query_map([i64::try_from(limit).unwrap_or(i64::MAX)], |row| {
            Ok(json!({
                "id": row.get::<_, String>(0)?, "tool": row.get::<_, String>(1)?,
                "query_text": row.get::<_, Option<String>>(2)?, "params": row.get::<_, Option<String>>(3)?,
                "duration_ms": row.get::<_, i64>(4)?, "created_at": row.get::<_, String>(5)?,
                "actor": row.get::<_, Option<String>>(6)?, "scope": row.get::<_, Option<String>>(7)?,
                "session": row.get::<_, Option<String>>(8)?
            }))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Creates or deletes a relation between two named entities.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation or SQLite access fails.
    pub fn relate(
        &self,
        from: &str,
        to: &str,
        relation: &str,
        action: &str,
    ) -> Result<Value, StoreError> {
        if from.trim().is_empty() || to.trim().is_empty() || relation.trim().is_empty() {
            return Err(StoreError::Invalid(
                "invalid arguments for 'entity_relate': 'from', 'to', and 'relation' are required"
                    .into(),
            ));
        }
        let conn = self.lock()?;
        let now = chrono::Utc::now().to_rfc3339();
        let entity_id = |name: &str| stable_id(&["entity", &name.to_lowercase()]);
        let from_id = entity_id(from.trim());
        let to_id = entity_id(to.trim());
        for (id, name) in [(&from_id, from.trim()), (&to_id, to.trim())] {
            conn.execute(
                "INSERT OR IGNORE INTO entities(id,name,type,aliases,description,created_by,created_at,updated_at) VALUES(?,?,?,?,?,?,?,?)",
                params![id, name, "other", "[]", "", "mcp", now, now],
            )?;
        }
        match action {
            "create" | "" => {
                let id = stable_id(&["relation", &from_id, &to_id, relation.trim()]);
                conn.execute(
                    "INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_by,created_at,id,updated_at) VALUES(?,?,?,?,?,?,?) ON CONFLICT(from_entity_id,to_entity_id,relation_type) DO UPDATE SET updated_at=excluded.updated_at",
                    params![from_id, to_id, relation.trim(), "mcp", now, id, now],
                )?;
                Ok(
                    json!({"from": from.trim(), "to": to.trim(), "relation": relation.trim(), "action": "create"}),
                )
            }
            "delete" => {
                let deleted = conn.execute(
                    "DELETE FROM entity_relations WHERE from_entity_id=? AND to_entity_id=? AND relation_type=?",
                    params![from_id, to_id, relation.trim()],
                )?;
                Ok(
                    json!({"from": from.trim(), "to": to.trim(), "relation": relation.trim(), "action": "delete", "deleted": deleted == 1}),
                )
            }
            other => Err(StoreError::Invalid(format!(
                "invalid arguments for 'entity_relate': 'action' must be 'create' or 'delete', got {other:?}"
            ))),
        }
    }

    /// Returns entities and relations reachable from a named entity.
    ///
    /// # Errors
    /// Returns a `StoreError` when validation or SQLite access fails.
    pub fn graph_neighbors(&self, entity: &str, depth: usize) -> Result<Value, StoreError> {
        if entity.trim().is_empty() {
            return Err(StoreError::Invalid(
                "invalid arguments for 'graph_neighbors': 'entity' is required".into(),
            ));
        }
        let depth = depth.clamp(1, 3);
        let conn = self.lock()?;
        let start: Option<String> = conn
            .query_row(
                "SELECT id FROM entities WHERE name=? COLLATE NOCASE OR aliases LIKE ? LIMIT 1",
                params![entity.trim(), format!("%{}%", entity.trim())],
                |row| row.get(0),
            )
            .optional()?;
        let Some(start) = start else {
            return Ok(json!({"nodes": [], "edges": []}));
        };
        let mut ids = std::collections::BTreeSet::from([start.clone()]);
        let mut frontier = vec![start];
        let mut edges = Vec::new();
        for _ in 0..depth {
            let mut next = Vec::new();
            for node in frontier {
                let mut statement = conn.prepare(
                    "SELECT r.id,r.from_entity_id,r.to_entity_id,r.relation_type,e1.name,e2.name FROM entity_relations r JOIN entities e1 ON e1.id=r.from_entity_id JOIN entities e2 ON e2.id=r.to_entity_id WHERE r.from_entity_id=? OR r.to_entity_id=? ORDER BY r.id",
                )?;
                let rows = statement.query_map(params![node, node], |row| {
                    Ok(json!({"id": row.get::<_, String>(0)?, "from_entity_id": row.get::<_, String>(1)?, "to_entity_id": row.get::<_, String>(2)?, "relation_type": row.get::<_, String>(3)?, "from": row.get::<_, String>(4)?, "to": row.get::<_, String>(5)?}))
                })?;
                for row in rows {
                    let edge = row?;
                    let other = if edge["from_entity_id"] == node {
                        edge["to_entity_id"].as_str().unwrap_or_default()
                    } else {
                        edge["from_entity_id"].as_str().unwrap_or_default()
                    };
                    if ids.insert(other.to_string()) {
                        next.push(other.to_string());
                    }
                    if !edges.iter().any(|existing: &Value| existing == &edge) {
                        edges.push(edge);
                    }
                }
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
        let mut nodes = Vec::new();
        for id in ids {
            if let Some(node) = conn
                .query_row(
                    "SELECT id,name,type,aliases,description,created_by,created_at,updated_at FROM entities WHERE id=?",
                    [&id],
                    |row| Ok(json!({"id": row.get::<_, String>(0)?, "name": row.get::<_, String>(1)?, "type": row.get::<_, String>(2)?, "aliases": serde_json::from_str::<Value>(&row.get::<_, String>(3)?).unwrap_or_else(|_| json!([])), "description": row.get::<_, String>(4)?, "created_by": row.get::<_, String>(5)?, "created_at": row.get::<_, String>(6)?, "updated_at": row.get::<_, String>(7)?})),
                )
                .optional()?
            {
                nodes.push(node);
            }
        }
        Ok(json!({"nodes": nodes, "edges": edges}))
    }
}

fn stable_id(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    let hex = format!("{:x}", hasher.finalize());
    format!("memory-{}", &hex[..32])
}
