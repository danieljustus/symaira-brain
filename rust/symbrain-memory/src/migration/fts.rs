//! Non-destructive recognition and repair of the known external-content owner.

use super::{facts, sql};
use crate::StoreError;
use rusqlite::{Connection, ErrorCode};

pub(super) fn recognize(conn: &Connection) -> Result<(), StoreError> {
    if let Some((kind, definition)) = sql::object(conn, "memories_fts")?
        && (kind != "table"
            || !facts::owned()?
                .fts_variants
                .contains(&sql::canonical(&definition)))
    {
        return Err(StoreError::Invalid(
            "incompatible owned memories_fts definition".into(),
        ));
    }
    for name in ["memories_ai", "memories_ad", "memories_au"] {
        if let Some((kind, definition)) = sql::object(conn, name)?
            && (kind != "trigger"
                || !facts::owned()?
                    .trigger_variants
                    .get(name)
                    .is_some_and(|variants| variants.contains(&sql::canonical(&definition))))
        {
            return Err(StoreError::Invalid(format!(
                "incompatible owned trigger {name}"
            )));
        }
    }
    Ok(())
}

pub(super) fn porter(
    conn: &Connection,
    resource: &str,
    pending: bool,
    check_only: bool,
) -> Result<(), StoreError> {
    recognize(conn)?;
    let current = sql::object(conn, "memories_fts")?.is_some_and(|(_, definition)| {
        sql::canonical(&definition).contains("tokenize='porter unicode61'")
    });
    if check_only {
        // FTS5's integrity-check is itself a write, so the read-only admission
        // proves the index covers exactly the content rows through its
        // docsize shadow instead. Anything else is repaired under the writer.
        let covered: bool = conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM memories WHERE rowid NOT IN (SELECT id FROM memories_fts_docsize)) AND NOT EXISTS(SELECT 1 FROM memories_fts_docsize WHERE id NOT IN (SELECT rowid FROM memories))",
            [],
            |row| row.get(0),
        )?;
        return if !pending && current && covered {
            Ok(())
        } else {
            Err(StoreError::Invalid("memories_fts needs repair".into()))
        };
    }
    if pending || !current {
        conn.execute_batch(resource)?;
    }
    match conn.execute(
        "INSERT INTO memories_fts(memories_fts,rank) VALUES('integrity-check',1)",
        [],
    ) {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(code, _)) if code.code == ErrorCode::DatabaseCorrupt => {
            // Shape and maintenance triggers were proven owned before this
            // typed corruption result. Disk/I/O/permission errors propagate.
            conn.execute(
                "INSERT INTO memories_fts(memories_fts) VALUES('rebuild')",
                [],
            )?;
            conn.execute(
                "INSERT INTO memories_fts(memories_fts,rank) VALUES('integrity-check',1)",
                [],
            )?;
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}
