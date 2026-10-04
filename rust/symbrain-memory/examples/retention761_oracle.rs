//! PREPARED actual public API caller on a private clone of a real Go-seeded DB.
use std::{error::Error, path::Path};

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::json;
use symbrain_memory::Store;

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Packet {
    #[serde(rename = "ID")]
    id: String,
    operation: String,
    #[serde(default)]
    at: String,
    #[serde(default)]
    start: String,
    #[serde(default)]
    end: String,
}

fn instant(raw: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(raw).map(|t| t.with_timezone(&Utc))
}

fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<_> = std::env::args_os().skip(1).collect();
    if arguments.len() != 2 {
        return Err("usage: retention761_oracle INPUT.json DATABASE".into());
    }
    let packet: Packet = serde_json::from_slice(&std::fs::read(&arguments[0])?)?;
    let store = Store::open(Path::new(&arguments[1]))?;
    let result = match packet.operation.as_str() {
        "expire" => store.activity_expire(Some(instant(&packet.at)?)),
        "range" => store.activity_clear_time_range(instant(&packet.start)?, instant(&packet.end)?),
        _ => return Err("unsupported retention operation".into()),
    };
    println!(
        "{}",
        json!({"id":packet.id,"segments":result.deleted.segments,
        "episodes":result.deleted.episodes,"error":result.error.map_or_else(String::new, |e| e.to_string())})
    );
    Ok(())
}
