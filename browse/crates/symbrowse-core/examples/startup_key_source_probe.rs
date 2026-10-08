//! Actual resolver probe: public owned provider fixtures, never a state store.
#![deny(unsafe_code)]
use std::{path::PathBuf, process::ExitCode, time::Duration};
use symbrowse_core::{key_resolver::KeyResolver, key_sources::SystemKeySources};
#[derive(serde::Serialize)]
struct Observation {
    configured: bool,
    key_source: String,
    error: String,
}
fn main() -> ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let [provider, owner] = args.as_slice() else {
        return ExitCode::from(2);
    };
    let sources = SystemKeySources::with_programs(
        PathBuf::from(provider),
        PathBuf::from(provider)
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("security"),
        Duration::from_secs(1),
    )
    .with_startup_owner(PathBuf::from(owner));
    let result = match KeyResolver::new(sources).resolve() {
        Ok(key) => Observation {
            configured: key.is_some(),
            key_source: key.map_or_else(String::new, |key| key.source().to_owned()),
            error: String::new(),
        },
        Err(error) => Observation {
            configured: false,
            key_source: String::new(),
            error: error.to_string(),
        },
    };
    match serde_json::to_writer(std::io::stdout().lock(), &result) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::from(1),
    }
}
