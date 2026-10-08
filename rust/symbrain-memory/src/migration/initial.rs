//! Capture eligibility before any repair can erase the historical evidence.

use super::sql;
use crate::{StoreError, schema::MIGRATIONS};
use rusqlite::Connection;
use std::collections::BTreeSet;

pub(super) struct Initial {
    pub applied: BTreeSet<String>,
    pub rules_updated_missing: bool,
    pub relation_updated_missing: bool,
    pub old_native_completion: bool,
}

impl Initial {
    pub fn capture(conn: &Connection) -> Result<Self, StoreError> {
        let applied = match sql::object(conn, "schema_migrations")? {
            None => BTreeSet::new(),
            Some((kind, _)) if kind == "table" => conn
                .prepare("SELECT version FROM schema_migrations")?
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<BTreeSet<_>, _>>()?,
            Some(_) => return Err(StoreError::Invalid("incompatible migration ledger".into())),
        };
        let all_claimed = MIGRATIONS.iter().all(|version| applied.contains(*version));
        let old_index =
            sql::object(conn, "idx_entity_relations_id")?.is_some_and(|(kind, definition)| {
                kind == "index"
                    && sql::canonical(&definition)
                        == sql::canonical(
                            "CREATE INDEX idx_entity_relations_id ON entity_relations(id)",
                        )
            });
        let missing_unique = sql::object(conn, "idx_entity_relations_relation_id")?.is_none();
        Ok(Self {
            applied,
            rules_updated_missing: !sql::column_present(conn, "rules", "updated_at")?,
            relation_updated_missing: !sql::column_present(conn, "entity_relations", "updated_at")?,
            old_native_completion: all_claimed && old_index && missing_unique,
        })
    }

    pub fn rules_backfill(&self) -> bool {
        !self.applied.contains("006_provenance")
            || self.rules_updated_missing
            || self.old_native_completion
    }

    pub fn relation_timestamps(&self) -> bool {
        !self.applied.contains("022_entity_relation_provenance")
            || self.relation_updated_missing
            || self.old_native_completion
    }
}
