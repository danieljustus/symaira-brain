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
        return verb == "serve" || (verb == "sync" && super::sync::needs_go(rest));
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
    // A database that does not exist yet (or is not a regular file) has no
    // rows to hydrate: the native opener creates it with the shipped modes or
    // reports the shipped open error, so no Go admission applies to it.
    if !path.is_file() {
        return false;
    }
    if matches!(verb, "list" | "rules" | "search")
        && !symbrain_memory::direct_reads_supported(&path, verb)
    {
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
