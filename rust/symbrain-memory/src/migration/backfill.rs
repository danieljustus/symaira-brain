//! Only proven rule/relation data repairs; nullable temporal fields stay intact.

use super::initial::Initial;
use crate::StoreError;
use rusqlite::{Connection, params};

pub(super) fn relations(
    conn: &Connection,
    initial: &Initial,
    statements: &[String],
) -> Result<(), StoreError> {
    let blank = conn.prepare("SELECT from_entity_id,to_entity_id,relation_type FROM entity_relations WHERE id='' ORDER BY from_entity_id,to_entity_id,relation_type")?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    // Preserve Go's operation order and exact UUIDv4 SQL before timestamps.
    let identity = statements
        .iter()
        .find(|statement| {
            statement
                .trim_start()
                .starts_with("UPDATE entity_relations\nSET id")
        })
        .ok_or_else(|| StoreError::Invalid("owned relation UUID backfill is absent".into()))?;
    conn.execute_batch(identity)?;
    let incomplete: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM entity_relations WHERE id='')",
        [],
        |row| row.get(0),
    )?;
    if incomplete {
        return Err(StoreError::Invalid(
            "incomplete owned relation identity backfill".into(),
        ));
    }
    if initial.relation_timestamps() {
        conn.execute(
            "UPDATE entity_relations SET updated_at=created_at WHERE updated_at IS NULL",
            [],
        )?;
        let incomplete: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM entity_relations WHERE updated_at IS NULL AND created_at IS NOT NULL)", [], |row| row.get(0))?;
        if incomplete {
            return Err(StoreError::Invalid(
                "incomplete owned relation timestamp backfill".into(),
            ));
        }
    } else {
        // An already-claimed migration only repairs timestamps on the specific
        // unchanged triples whose blank identity proved incomplete provenance.
        for (from, to, relation) in blank {
            conn.execute("UPDATE entity_relations SET updated_at=created_at WHERE updated_at IS NULL AND from_entity_id=? AND to_entity_id=? AND relation_type=?",
                params![&from, &to, &relation])?;
            let incomplete: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM entity_relations WHERE updated_at IS NULL AND created_at IS NOT NULL AND from_entity_id=? AND to_entity_id=? AND relation_type=?)", params![from,to,relation], |row| row.get(0))?;
            if incomplete {
                return Err(StoreError::Invalid(
                    "incomplete owned relation timestamp backfill".into(),
                ));
            }
        }
    }
    Ok(())
}
