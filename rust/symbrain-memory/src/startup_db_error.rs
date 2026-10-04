//! Preserve startup failure phase while sharing the Store's atomic migration.
use symbrain_core::GoText;
use crate::{StoreError, migration::ConfigurePhase};

pub(super) fn format(phase: ConfigurePhase, error: &StoreError) -> GoText {
    let prefix = match phase {
        ConfigurePhase::Connection => "failed to open sqlite database: failed to open sqlite database: ",
        ConfigurePhase::SecureDelete => "failed to enable secure sqlite deletes: ",
        ConfigurePhase::Migration => "failed to run migrations: ",
    };
    let detail = match error {
        StoreError::Sql(rusqlite::Error::SqliteFailure(code, message)) => {
            if code.extended_code == 14 {
                "unable to open database file: out of memory (14)".to_owned()
            } else {
                // Frozen modernc preserves the SQLite detail alongside its
                // result-code label. Actual paired failures remain required.
                if code.extended_code & 255 == 1 {
                    message.as_ref().map_or_else(|| error.to_string(),
                        |message| format!("SQL logic error: {message} ({})", code.extended_code))
                } else { error.to_string() }
            }
        }
        _ => error.to_string(),
    };
    format!("{prefix}{detail}").into()
}
