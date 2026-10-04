//! Owned omission contracts: exact defaults with one proven historical spelling.

use super::facts::Column;
use crate::StoreError;

pub(super) fn verify(table: &str, actual: &Column, expected: &Column) -> Result<(), StoreError> {
    if actual.default == expected.default
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
