use chrono::{DateTime, Utc};
use rusqlite::Row;

use crate::Memory;

pub(crate) fn row_memory(row: &Row<'_>) -> rusqlite::Result<Memory> {
    Ok(Memory {
        id: row.get(0)?,
        content: row.get(1)?,
        scope: row.get(2)?,
        metadata: json(row, 3)?,
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

/// Stored decode errors are data errors, never successful empty defaults.
pub(crate) fn json<T: serde::de::DeserializeOwned>(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<T> {
    serde_json::from_str(&row.get::<_, String>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

/// A stored metadata column: a JSON object, or JSON `null` when the writer
/// marshalled a nil map (the shipped decoder yields a nil map, which renders
/// as `null` again). The flag keeps that distinction.
pub(crate) fn metadata(
    row: &Row<'_>,
    index: usize,
) -> rusqlite::Result<(serde_json::Map<String, serde_json::Value>, bool)> {
    match json::<serde_json::Value>(row, index)? {
        serde_json::Value::Null => Ok((serde_json::Map::new(), true)),
        serde_json::Value::Object(map) => Ok((map, false)),
        other => Err(rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("metadata is not an object: {other}"),
            )),
        )),
    }
}

fn parse_time(raw: &str) -> rusqlite::Result<DateTime<Utc>> {
    crate::gotime::parse(raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("unrecognized timestamp: {raw}"),
            )),
        )
    })
}
