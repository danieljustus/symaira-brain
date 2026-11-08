//! `ResolveEntityCandidates` (ported for #760 from
//! `internal/memory/db/entities.go` at v0.12.0): read-only ranked matches of
//! a query and optional alias hints against stored entities.
//!
//! Hints are comparison-only: they are never stored, and any hint that looks
//! like an email address or phone number is dropped before matching so
//! contact identifiers never become implicit memory data.

use crate::entity_resolve::{MAX_HINT_LENGTH, best_match, is_pii, normalize};
use crate::model::{Store, StoreError};
use serde::Serialize;

/// One ranked match.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntityCandidate {
    /// Entity id.
    pub entity_id: String,
    /// Entity name.
    pub name: String,
    /// Entity type.
    #[serde(rename = "type")]
    pub entity_type: String,
    /// Stored aliases.
    pub aliases: Vec<String>,
    /// Fixed per-kind score.
    pub score: f64,
    /// `exact_name`, `exact_alias`, `normalized_name`, `normalized_alias`.
    pub match_kind: String,
    /// Human-readable explanation.
    pub match_reason: String,
}

struct EntityRow {
    id: String,
    name: String,
    kind: String,
    aliases: Vec<String>,
}

impl Store {
    /// Entities matching `query` (or one of `alias_hints`), best first;
    /// `entity_type` (when non-empty) restricts strictly; `limit == 0` is
    /// unbounded.
    ///
    /// # Errors
    /// Empty or oversized `query`, or a SQLite/alias-JSON failure.
    pub fn resolve_entity_candidates(
        &self,
        query: &str,
        entity_type: &str,
        alias_hints: &[&str],
        limit: usize,
    ) -> Result<Vec<EntityCandidate>, StoreError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(StoreError::Invalid("query must not be empty".into()));
        }
        if query.len() > MAX_HINT_LENGTH {
            return Err(StoreError::Invalid(format!(
                "query exceeds maximum length of {MAX_HINT_LENGTH} characters"
            )));
        }
        let mut hints = vec![query];
        for hint in alias_hints {
            let hint = hint.trim();
            if hint.is_empty() || hint.len() > MAX_HINT_LENGTH || is_pii(hint) {
                continue;
            }
            hints.push(hint);
        }
        let mut candidates = Vec::new();
        for entity in self.entity_rows()? {
            if !entity_type.is_empty() && entity.kind != entity_type {
                continue;
            }
            let aliases: Vec<&str> = entity.aliases.iter().map(String::as_str).collect();
            let Some(found) = best_match(&hints, &entity.name, &aliases) else {
                continue;
            };
            candidates.push(EntityCandidate {
                entity_id: entity.id,
                name: entity.name,
                entity_type: entity.kind,
                aliases: entity.aliases,
                score: found.score,
                match_kind: found.kind.as_str().to_owned(),
                match_reason: found.reason,
            });
        }
        candidates.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.match_kind.cmp(&b.match_kind))
                .then_with(|| normalize(&a.name).cmp(&normalize(&b.name)))
                .then_with(|| a.entity_id.cmp(&b.entity_id))
        });
        if limit > 0 {
            candidates.truncate(limit);
        }
        Ok(candidates)
    }

    fn entity_rows(&self) -> Result<Vec<EntityRow>, StoreError> {
        let conn = self.lock()?;
        let mut statement = conn
            .prepare("SELECT id, name, type, aliases FROM entities ORDER BY name COLLATE NOCASE")
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let rows = statement
            .query_map([], |row| {
                let aliases: String = row.get(3)?;
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, aliases))
            })
            .map_err(|e| StoreError::Invalid(e.to_string()))?;
        let mut out = Vec::new();
        for row in rows {
            let (id, name, kind, aliases): (String, String, String, String) =
                row.map_err(|e| StoreError::Invalid(e.to_string()))?;
            let aliases =
                serde_json::from_str(&aliases).map_err(|e| StoreError::Invalid(e.to_string()))?;
            out.push(EntityRow {
                id,
                name,
                kind,
                aliases,
            });
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    fn seed(store: &Store) {
        let conn = store.lock().unwrap();
        for (id, name, kind, aliases) in [
            ("e1", "José Müller", "person", r#"["Joe M"]"#),
            ("e2", "jose muller", "person", "[]"),
            ("e3", "Acme", "org", r#"["ACME Corp","Jose Muller"]"#),
        ] {
            conn.execute(
                "INSERT INTO entities (id, name, type, aliases, description, created_by, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, '', '', '2026-01-01 00:00:00 +0000 UTC', '2026-01-01 00:00:00 +0000 UTC')",
                rusqlite::params![id, name, kind, aliases],
            )
            .unwrap();
        }
    }

    #[test]
    fn ranks_by_score_kind_name_and_id_and_filters() {
        let store = Store::open_in_memory().unwrap();
        seed(&store);
        let all = store
            .resolve_entity_candidates("Jose Muller", "", &[], 0)
            .unwrap();
        let got: Vec<_> = all
            .iter()
            .map(|c| (c.entity_id.as_str(), c.match_kind.as_str(), c.score))
            .collect();
        assert_eq!(
            got,
            [
                ("e2", "exact_name", 1.0),
                ("e3", "exact_alias", 0.9),
                ("e1", "normalized_name", 0.75)
            ]
        );
        let people = store
            .resolve_entity_candidates("Jose Muller", "person", &[], 1)
            .unwrap();
        assert_eq!(people.len(), 1);
        assert_eq!(people[0].entity_id, "e2");
        // Hints extend the search; PII-shaped hints are dropped.
        let hinted = store
            .resolve_entity_candidates("nobody", "", &["ACME Corp", "boss@example.com"], 0)
            .unwrap();
        assert_eq!(hinted.len(), 1);
        assert_eq!(hinted[0].entity_id, "e3");
        assert_eq!(hinted[0].match_kind, "exact_alias");
        let json = serde_json::to_value(&hinted[0]).unwrap();
        assert_eq!(json["type"], "org");
    }

    #[test]
    fn rejects_bad_queries() {
        let store = Store::open_in_memory().unwrap();
        assert!(
            store
                .resolve_entity_candidates("  ", "", &[], 0)
                .unwrap_err()
                .to_string()
                .contains("must not be empty")
        );
        let long = "x".repeat(501);
        assert!(
            store
                .resolve_entity_candidates(&long, "", &[], 0)
                .unwrap_err()
                .to_string()
                .contains("exceeds maximum length of 500")
        );
    }
}
