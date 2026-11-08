//! Embedding columns of a synced memory (#855).
//!
//! The lite wire form a Go-era peer sends carries no vector. Applying such a
//! row must not wipe a valid stored embedding, so [`preserved`] hands back the
//! stored columns when the incoming row has no vector and its content is the
//! content the stored vector was computed for. A content change without a new
//! vector clears the columns instead: a vector of other text is not valid.

use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension};

/// The seven embedding-related columns as they are bound to INSERT/UPDATE.
pub(crate) struct EmbeddingColumns {
    pub embedding: Value,
    pub binary: Value,
    pub dimension: Value,
    pub source: Value,
    pub model: Value,
    pub quantization: Value,
    pub lsh: Value,
}

impl EmbeddingColumns {
    /// The columns of an incoming row.
    pub(crate) fn incoming(
        embedding: String,
        binary: Option<Vec<u8>>,
        dimension: i64,
        labels: [&str; 3],
        lsh: i64,
    ) -> Self {
        Self {
            embedding: Value::Text(embedding),
            binary: binary.map_or(Value::Null, Value::Blob),
            dimension: Value::Integer(dimension),
            source: Value::Text(labels[0].to_owned()),
            model: Value::Text(labels[1].to_owned()),
            quantization: Value::Text(labels[2].to_owned()),
            lsh: Value::Integer(lsh),
        }
    }
}

/// The stored columns of `id` when they must survive an update whose incoming
/// row has no vector and the content `incoming_content`.
///
/// # Errors
/// The SQLite error.
pub(crate) fn preserved(
    conn: &Connection,
    id: &str,
    incoming_content: &str,
) -> rusqlite::Result<Option<EmbeddingColumns>> {
    let stored = conn
        .query_row(
            "SELECT content, embedding, embedding_binary, embedding_dim, embedding_source, embedding_model, embedding_quantization, lsh_hash FROM memories WHERE id=?",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    EmbeddingColumns {
                        embedding: row.get(1)?,
                        binary: row.get(2)?,
                        dimension: row.get(3)?,
                        source: row.get(4)?,
                        model: row.get(5)?,
                        quantization: row.get(6)?,
                        lsh: row.get(7)?,
                    },
                ))
            },
        )
        .optional()?;
    Ok(stored.and_then(|(content, columns)| {
        let has_vector = matches!(columns.dimension, Value::Integer(dimension) if dimension > 0);
        (has_vector && content == incoming_content).then_some(columns)
    }))
}
