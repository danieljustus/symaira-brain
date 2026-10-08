use std::{path::PathBuf, time::Duration};
use symbrowse_core::{key_resolver::KeyResolver, key_sources::SystemKeySources};
fn main() {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let sources = SystemKeySources::with_programs(PathBuf::from(&args[0]), PathBuf::from(&args[0]), Duration::from_secs(1));
    let sources = sources.with_startup_owner(PathBuf::from(&args[1]));
    let outcome = KeyResolver::new(sources).resolve().map(|key| key.map(|key| key.source().to_string()));
    println!("{outcome:?}");
}
