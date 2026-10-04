//! Apply the exact owned effects and record each verified ordered postcondition.

use super::{backfill, catalog, facts, fts, initial::Initial, sql};
use crate::{
    StoreError,
    schema::{COLUMN_PARITY, INDEXES, MIGRATIONS, SCHEMA},
};
use rusqlite::Connection;

pub(super) fn apply(conn: &Connection) -> Result<(), StoreError> {
    if !catalog::STEPS
        .iter()
        .map(|(version, _)| *version)
        .eq(MIGRATIONS.iter().copied())
    {
        return Err(StoreError::Invalid(
            "owned migration identifiers differ".into(),
        ));
    }
    let initial = Initial::capture(conn)?;
    // Validate preexisting FTS/trigger identities before even creating missing
    // current schema objects; unknown objects are never selected for dropping.
    fts::recognize(conn)?;
    facts::owned()?.verify_existing_defaults(conn)?;
    conn.execute_batch(SCHEMA)?;
    for (table, column, definition) in COLUMN_PARITY {
        if !sql::column_present(conn, table, column)? {
            conn.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN {column} {definition}"
            ))?;
        }
        facts::owned()?.verify_column(conn, table, column)?;
    }
    for (version, resource) in catalog::STEPS {
        let statements = sql::statements(resource)?;
        if *version == "027_fts5_porter" {
            fts::porter(conn, resource, !initial.applied.contains(*version))?;
        } else {
            for statement in &statements {
                apply_statement(conn, statement, version, &initial)?;
            }
        }
        // Every statement's named objects are checked before publishing its
        // entry; final constraints/indexes/triggers are checked before commit.
        verify_step(conn, &statements)?;
        conn.execute(
            "INSERT OR IGNORE INTO schema_migrations(version) VALUES (?)",
            [version],
        )?;
        let recorded: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version=?)",
            [version],
            |row| row.get(0),
        )?;
        if !recorded {
            return Err(StoreError::Invalid(format!(
                "incomplete owned migration marker {version}"
            )));
        }
    }
    // Retain the approved native extra index, with definition validation too.
    for statement in sql::statements(INDEXES)? {
        sql::ensure_index(conn, &statement)?;
    }
    facts::owned()?.verify_final(conn)
}

fn apply_statement(
    conn: &Connection,
    statement: &str,
    version: &str,
    initial: &Initial,
) -> Result<(), StoreError> {
    let start = statement.trim_start();
    if let Some((table, column)) = sql::alter(statement) {
        if !sql::column_present(conn, table, column)? {
            conn.execute_batch(statement)?;
        }
        return facts::owned()?.verify_column(conn, table, column);
    }
    if start.starts_with("CREATE INDEX") || start.starts_with("CREATE UNIQUE INDEX") {
        return sql::ensure_index(conn, statement);
    }
    if start.starts_with("CREATE TRIGGER") {
        return ensure_trigger(conn, statement);
    }
    if start.starts_with("DROP TRIGGER") && version == "034_activity_store" {
        // The current schema may already have installed the final definition.
        // Generic trigger creation accepts known versions; this transition
        // only drops the exact historical variant when replacement is needed.
        let name = sql::object_name(statement, "TRIGGER")
            .ok_or_else(|| StoreError::Invalid("owned trigger transition has no name".into()))?;
        if let Some((kind, current)) = sql::object(conn, name)? {
            let owned = facts::owned()?;
            if kind != "trigger"
                || !owned
                    .trigger_variants
                    .get(name)
                    .is_some_and(|variants| variants.contains(&sql::canonical(&current)))
            {
                return Err(StoreError::Invalid(format!(
                    "incompatible owned trigger {name}"
                )));
            }
            if owned
                .triggers
                .get(name)
                .is_none_or(|final_sql| sql::canonical(&current) != sql::canonical(final_sql))
            {
                conn.execute_batch(statement)?;
            }
        }
        return Ok(());
    }
    if start.starts_with("UPDATE rules") && version == "006_provenance" {
        if initial.rules_backfill() {
            conn.execute_batch(statement)?;
            let incomplete: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM rules WHERE updated_at IS NULL AND created_at IS NOT NULL)", [], |row| row.get(0))?;
            if incomplete {
                return Err(StoreError::Invalid(
                    "incomplete owned rule timestamp backfill".into(),
                ));
            }
        }
        return Ok(());
    }
    if start.starts_with("UPDATE entity_relations") && version == "022_entity_relation_provenance" {
        // Apply both ordered row effects once, at the first UUID statement.
        if start.starts_with("UPDATE entity_relations\nSET id") {
            return backfill::relations(
                conn,
                initial,
                &sql::statements(
                    catalog::STEPS
                        .iter()
                        .find(|(name, _)| *name == version)
                        .ok_or_else(|| StoreError::Invalid("owned relation step is absent".into()))?
                        .1,
                )?,
            );
        }
        return Ok(());
    }
    if start.starts_with("CREATE TABLE") || start.starts_with("CREATE VIRTUAL TABLE") {
        conn.execute_batch(statement)?;
        return Ok(());
    }
    Err(StoreError::Invalid(format!(
        "unsupported owned migration action in {version}"
    )))
}

fn ensure_trigger(conn: &Connection, statement: &str) -> Result<(), StoreError> {
    let name = sql::object_name(statement, "TRIGGER")
        .ok_or_else(|| StoreError::Invalid("owned trigger has no name".into()))?;
    if let Some((kind, current)) = sql::object(conn, name)? {
        if kind != "trigger"
            || !facts::owned()?
                .trigger_variants
                .get(name)
                .is_some_and(|variants| variants.contains(&sql::canonical(&current)))
        {
            return Err(StoreError::Invalid(format!(
                "incompatible owned trigger {name}"
            )));
        }
    } else {
        conn.execute_batch(statement)?;
    }
    Ok(())
}

fn verify_step(conn: &Connection, statements: &[String]) -> Result<(), StoreError> {
    for statement in statements {
        if let Some((table, column)) = sql::alter(statement) {
            facts::owned()?.verify_column(conn, table, column)?;
        }
        for kind in ["TABLE", "INDEX", "TRIGGER"] {
            if statement.trim_start().starts_with("CREATE") {
                if let Some(name) = sql::object_name(statement, kind) {
                    if !sql::object(conn, name)?
                        .is_some_and(|(actual, _)| actual.eq_ignore_ascii_case(kind))
                    {
                        return Err(StoreError::Invalid(format!("missing owned {kind} {name}")));
                    }
                }
            }
        }
    }
    Ok(())
}
