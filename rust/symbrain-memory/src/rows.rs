use chrono::{DateTime, Utc};
use rusqlite::Row;

use crate::Memory;

pub(crate) fn row_memory(row: &Row<'_>) -> rusqlite::Result<Memory> {
    let metadata: String = row.get(3)?;
    Ok(Memory {
        id: row.get(0)?,
        content: row.get(1)?,
        scope: row.get(2)?,
        metadata: serde_json::from_str(&metadata).unwrap_or_default(),
        created_at: parse_time(&row.get::<_, String>(4)?)?,
        updated_at: parse_time(&row.get::<_, String>(5)?)?,
        created_by: row.get(6)?,
        created_session: row.get(7)?,
        entities: serde_json::from_str(&row.get::<_, String>(11)?).unwrap_or_default(),
        consolidation_status: row.get(8)?,
        kind: row.get(9)?,
        importance: row.get(10)?,
    })
}

pub(crate) fn row_memory_score(row: &Row<'_>) -> rusqlite::Result<(Memory, f32)> {
    Ok((row_memory(row)?, 1.0))
}

fn parse_time(raw: &str) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                raw.len(),
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}
