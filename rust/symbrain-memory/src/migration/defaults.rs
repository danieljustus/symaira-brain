//! Owned omission contracts: exact defaults with one proven historical spelling.

use super::facts::Column;
use crate::StoreError;

/// Original-table columns that older Go releases declared with an explicit
/// zero-value default (#649). Every native write names these columns, so the
/// extra default is never observable; any other spelling stays incompatible.
const LEGACY_ZERO_DEFAULTS: &[(&str, &str, &str)] = &[
    ("memories", "scope", "''"),
    ("memories", "metadata", "'{}'"),
    ("memories", "embedding", "''"),
    ("rules", "scope", "''"),
    ("rules", "metadata", "'{}'"),
];

pub(super) fn verify(table: &str, actual: &Column, expected: &Column) -> Result<(), StoreError> {
    if actual.default == expected.default
        || (expected.default.is_none()
            && actual.default.as_deref().is_some_and(|default| {
                LEGACY_ZERO_DEFAULTS.contains(&(table, expected.name.as_str(), default))
            }))
        || (table == "query_log"
            && expected.name == "created_at"
            && actual.default.as_deref() == Some("CURRENT_TIMESTAMP")
            && expected.default.as_deref() == Some("datetime('now')"))
    {
        return Ok(());
    }
    // Do not evaluate caller expressions to guess equivalence, or normalize
    // all SQL. Only this declared old-native footprint has supporting evidence.
    Err(StoreError::Invalid(format!(
        "incompatible owned default {table}.{}",
        expected.name
    )))
}
