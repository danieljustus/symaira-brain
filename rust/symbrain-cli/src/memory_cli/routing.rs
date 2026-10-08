//! Native memory routing contracts.

use super::{OsString, flags, kind, resolve_db_path};

pub(crate) fn requires_go_fallback(args: &[OsString]) -> bool {
    let Some(verb) = args.first().and_then(|value| value.to_str()) else {
        return false;
    };
    let rest = &args[1..];
    if matches!(verb, "-h" | "--help") {
        return false;
    }
    if !matches!(
        verb,
        "list" | "rules" | "query-log" | "search" | "set" | "delete"
    ) {
        return verb == "serve" || (verb == "sync" && !rest.is_empty());
    }
    if flags::has_help(rest) {
        return false;
    }
    let Ok(parsed) = flags::parse(rest, verb, &mut Vec::new()) else {
        return false;
    };
    let invalid_positionals = match verb {
        "list" | "rules" | "query-log" => !parsed.positional.is_empty(),
        _ => parsed.positional.len() != 1,
    };
    if invalid_positionals {
        return false;
    }
    if matches!(verb, "search" | "set" | "delete") {
        let content = flags::go_string(&flags::bytes(&parsed.positional[0]));
        if content.is_empty() || (verb != "search" && content.trim().is_empty()) {
            return false;
        }
    }
    if verb == "set" && kind::normalize(&parsed.text("kind")).is_none() {
        return false;
    }
    if verb == "set" && !super::write::direct_write_supported(&parsed) {
        return true;
    }
    // Valid operations still retain configuration/DB and governed-write gates
    // until their source-bound process contracts are completed.
    if verb == "search" && super::config::load().prefilter {
        return true;
    }
    let path = resolve_db_path(Some(&parsed.raw("db")));
    if !database_path_is_usable(&path) {
        return true;
    }
    if matches!(verb, "list" | "rules" | "search")
        && path.is_file()
        && !symbrain_memory::direct_reads_supported(&path, verb)
    {
        return true;
    }
    // File/directory modes for newly created stores are still a release gate.
    if matches!(verb, "set" | "delete") && !path.is_file() {
        return true;
    }
    if verb == "set" {
        let entities = parsed
            .text("entities")
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !symbrain_memory::direct_entities_supported(&path, &entities) {
            return true;
        }
    }
    if verb == "delete" {
        let Some(id) = parsed.positional[0].to_str() else {
            return true;
        };
        if !symbrain_memory::direct_delete_supported(&path, id.trim()) {
            return true;
        }
    }
    false
}

/// Reports whether the resolved database path can be opened.
///
/// A missing database is fine as long as its directory (or the nearest
/// existing ancestor) is writable; otherwise the shipped implementation's
/// directory-creation error is the contract, and the command stays on Go.
pub(super) fn database_path_is_usable(path: &std::path::Path) -> bool {
    if path.is_file() {
        return true;
    }
    let mut current = path.parent();
    while let Some(directory) = current {
        match std::fs::metadata(directory) {
            Ok(metadata) => return is_writable(&metadata),
            Err(_) => current = directory.parent(),
        }
    }
    false
}

#[cfg(unix)]
pub(super) fn is_writable(metadata: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;

    metadata.permissions().mode() & 0o200 != 0
}

#[cfg(not(unix))]
pub(super) fn is_writable(metadata: &std::fs::Metadata) -> bool {
    !metadata.permissions().readonly()
}
