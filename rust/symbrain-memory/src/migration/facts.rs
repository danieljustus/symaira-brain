//! Cached facts from executing the exact owned migration SQL in an empty store.

use super::{catalog, sql};
use crate::StoreError;
use rusqlite::Connection;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct Column {
    pub name: String,
    pub kind: String,
    pub not_null: bool,
    pub default: Option<String>,
    pub pk: i64,
}

type ForeignKey = (String, String, String, String, String, String);
type UniqueKey = Vec<(Option<String>, String, bool)>;

pub(super) struct Facts {
    pub columns: BTreeMap<String, Vec<Column>>,
    pub indexes: BTreeMap<String, String>,
    pub triggers: BTreeMap<String, String>,
    pub trigger_variants: BTreeMap<String, BTreeSet<String>>,
    pub fts_variants: BTreeSet<String>,
    pub added: BTreeSet<(String, String)>,
    foreign_keys: BTreeMap<String, BTreeSet<ForeignKey>>,
    unique_keys: BTreeMap<String, BTreeSet<UniqueKey>>,
    checks: BTreeMap<String, BTreeSet<String>>,
    without_rowid: BTreeMap<String, bool>,
}

pub(super) fn owned() -> Result<&'static Facts, StoreError> {
    static FACTS: OnceLock<Result<Facts, String>> = OnceLock::new();
    FACTS
        .get_or_init(|| load().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| StoreError::Invalid(format!("owned migration facts: {error}")))
}

fn load() -> Result<Facts, StoreError> {
    let conn = Connection::open_in_memory()?;
    conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version TEXT PRIMARY KEY, applied_at DATETIME DEFAULT CURRENT_TIMESTAMP);")?;
    let mut variants: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut fts = BTreeSet::new();
    let mut added = BTreeSet::new();
    for (_, resource) in catalog::STEPS {
        for statement in sql::statements(resource)? {
            if let Some((table, column)) = sql::alter(&statement) {
                added.insert((table.to_owned(), column.to_owned()));
            }
        }
        conn.execute_batch(resource)?;
        for (name, definition) in definitions(&conn, "trigger")? {
            variants
                .entry(name)
                .or_default()
                .insert(sql::canonical(&definition));
        }
        if let Some((_, definition)) = sql::object(&conn, "memories_fts")? {
            let normalized = sql::canonical(&definition);
            // Explicit unicode61 is the known spelling of FTS5's default.
            if normalized.ends_with("content_rowid=rowid)") {
                fts.insert(normalized.replace(
                    "content_rowid=rowid)",
                    "content_rowid=rowid,tokenize='unicode61')",
                ));
            }
            fts.insert(normalized);
        }
    }
    let mut tables = conn.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")?;
    let names = tables
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut columns_by_table = BTreeMap::new();
    let mut foreign_by_table = BTreeMap::new();
    let mut unique_by_table = BTreeMap::new();
    let mut checks_by_table = BTreeMap::new();
    let mut rowid_by_table = BTreeMap::new();
    for table in names {
        columns_by_table.insert(table.clone(), columns(&conn, &table)?);
        foreign_by_table.insert(table.clone(), foreign_keys(&conn, &table)?);
        unique_by_table.insert(table.clone(), unique_keys(&conn, &table)?);
        let definition = sql::object(&conn, &table)?.unwrap().1;
        checks_by_table.insert(table.clone(), sql::checks(&definition));
        rowid_by_table.insert(table.clone(), without_rowid(&conn, &table)?);
    }
    Ok(Facts {
        columns: columns_by_table,
        indexes: definitions(&conn, "index")?,
        triggers: definitions(&conn, "trigger")?,
        trigger_variants: variants,
        fts_variants: fts,
        added,
        foreign_keys: foreign_by_table,
        unique_keys: unique_by_table,
        checks: checks_by_table,
        without_rowid: rowid_by_table,
    })
}

pub(super) fn columns(conn: &Connection, table: &str) -> Result<Vec<Column>, StoreError> {
    Ok(conn
        .prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |row| {
            Ok(Column {
                name: row.get(1)?,
                kind: row.get(2)?,
                not_null: row.get::<_, i64>(3)? != 0,
                default: row.get(4)?,
                pk: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?)
}

fn definitions(conn: &Connection, kind: &str) -> Result<BTreeMap<String, String>, StoreError> {
    Ok(conn
        .prepare(
            "SELECT name, sql FROM sqlite_schema WHERE type=? AND sql IS NOT NULL ORDER BY name",
        )?
        .query_map([kind], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<BTreeMap<_, _>, _>>()?)
}

fn foreign_keys(conn: &Connection, table: &str) -> Result<BTreeSet<ForeignKey>, StoreError> {
    Ok(conn
        .prepare(&format!("PRAGMA foreign_key_list({table})"))?
        .query_map([], |row| {
            Ok((
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })?
        .collect::<Result<BTreeSet<_>, _>>()?)
}

fn unique_keys(conn: &Connection, table: &str) -> Result<BTreeSet<UniqueKey>, StoreError> {
    let names = conn
        .prepare(&format!("PRAGMA index_list({table})"))?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(1)?,
                row.get::<_, bool>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut result = BTreeSet::new();
    for (name, unique, origin) in names {
        if unique && origin != "c" {
            let key = conn
                .prepare(&format!("PRAGMA index_xinfo({name})"))?
                .query_map([], |row| {
                    Ok((
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, bool>(3)?,
                        row.get::<_, bool>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .filter(|row| row.3)
                .map(|(column, collation, descending, _)| (column, collation, descending))
                .collect();
            result.insert(key);
        }
    }
    Ok(result)
}

fn compatible_default(actual: &Option<String>, expected: &Option<String>) -> bool {
    if actual == expected {
        return true;
    }
    matches!(
        (actual.as_deref(), expected.as_deref()),
        (Some("CURRENT_TIMESTAMP"), Some("datetime('now')"))
    )
}

fn without_rowid(conn: &Connection, table: &str) -> Result<bool, StoreError> {
    Ok(conn.query_row(
        "SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?",
        [table],
        |row| row.get(0),
    )?)
}

impl Facts {
    pub fn verify_column(
        &self,
        conn: &Connection,
        table: &str,
        column: &str,
    ) -> Result<(), StoreError> {
        let expected = self
            .columns
            .get(table)
            .and_then(|columns| columns.iter().find(|entry| entry.name == column))
            .ok_or_else(|| StoreError::Invalid(format!("unknown owned column {table}.{column}")))?;
        let actual = columns(conn, table)?
            .into_iter()
            .find(|entry| entry.name == column)
            .ok_or_else(|| StoreError::Invalid(format!("missing owned column {table}.{column}")))?;
        let additive = self.added.contains(&(table.to_owned(), column.to_owned()));
        if !actual.kind.eq_ignore_ascii_case(&expected.kind)
            || actual.pk != expected.pk
            || (additive
                && (actual.not_null != expected.not_null
                    || !compatible_default(&actual.default, &expected.default)))
        {
            return Err(StoreError::Invalid(format!(
                "incompatible owned column {table}.{column}"
            )));
        }
        Ok(())
    }

    pub fn verify_final(&self, conn: &Connection) -> Result<(), StoreError> {
        for (table, expected) in &self.columns {
            if !sql::object(conn, table)?.is_some_and(|(kind, _)| kind == "table") {
                return Err(StoreError::Invalid(format!("missing owned table {table}")));
            }
            for column in expected {
                self.verify_column(conn, table, &column.name)?;
            }
            if !self.foreign_keys[table].is_subset(&foreign_keys(conn, table)?)
                || !self.unique_keys[table].is_subset(&unique_keys(conn, table)?)
                || !self.checks[table]
                    .is_subset(&sql::checks(&sql::object(conn, table)?.unwrap().1))
                || self.without_rowid[table] != without_rowid(conn, table)?
            {
                return Err(StoreError::Invalid(format!(
                    "incompatible owned constraints {table}"
                )));
            }
        }
        for (name, expected) in self.indexes.iter().chain(self.triggers.iter()) {
            let Some((_, actual)) = sql::object(conn, name)? else {
                return Err(StoreError::Invalid(format!("missing owned object {name}")));
            };
            if sql::canonical(&actual) != sql::canonical(expected) {
                return Err(StoreError::Invalid(format!(
                    "incompatible owned object {name}"
                )));
            }
        }
        Ok(())
    }
}
