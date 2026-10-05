#![deny(unsafe_code)]
//! Executable public Core observation in a caller-owned disposable directory.
use std::{collections::BTreeMap, fs, path::PathBuf};
use symbrowse_core::{
    state::State,
    state_store::{KeyMaterial, Store},
};
use time::{Duration, OffsetDateTime};

fn snapshot() -> State {
    State {
        schema_version: 3,
        name: "existing".into(),
        saved_at: String::new(),
        expires_at: String::new(),
        key_source: String::new(),
        origins: BTreeMap::new(),
    }
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("owned directory required")?,
    );
    if root.exists() {
        return Err("probe requires a new owned directory".into());
    }
    let encrypted = Store::new(
        &root,
        Duration::days(30),
        Some(KeyMaterial::new([0xab; 32], "environment")?),
    )?;
    encrypted.save_at(&mut snapshot(), OffsetDateTime::now_utc())?;
    let before = fs::read(root.join("existing.json"))?;
    let plain = Store::new(&root, Duration::days(30), None)?;
    let saved = plain.save_at(&mut snapshot(), OffsetDateTime::now_utc());
    let after = fs::read(root.join("existing.json"))?;
    let loaded = plain.load("existing");
    println!(
        "{}",
        serde_json::json!({"fresh_snapshot_save_success":saved.is_ok(),
        "retained_bytes_unchanged":before==after,"no_key_load_success":loaded.is_ok(),
        "loaded_key_source":loaded.ok().map(|value|value.key_source).unwrap_or_default()})
    );
    Ok(())
}
