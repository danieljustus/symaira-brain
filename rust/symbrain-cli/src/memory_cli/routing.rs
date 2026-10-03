//! Native memory routing contracts.

use super::{OsString, PathBuf, flags, kind, resolve_db_path};

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
    if verb == "set" {
        let raw_kind = parsed.text("kind");
        // Parsing aliases does not prove governed write state. Preserve the
        // previous canonical-kind/scope cutover boundary for valid writes.
        if kind::normalize(&raw_kind) != Some(raw_kind.as_str())
            || !["global", "project", "agent", "user", "session"]
                .contains(&parsed.text("scope").as_str())
        {
            return true;
        }
    }
    // Valid operations still retain configuration/DB and governed-write gates
    // until their source-bound process contracts are completed.
    if verb == "search" && super::config::load().prefilter {
        return true;
    }
    if verb == "set" && memory_config_is_dynamic() {
        return true;
    }
    if !database_path_is_usable(&resolve_db_path(Some(&parsed.raw("db")))) {
        return true;
    }
    if verb == "set"
        && (!parsed.text("metadata").is_empty()
            || !parsed.text("entities").is_empty()
            || parsed.text("author") != "cli:symbrain")
    {
        return true;
    }
    false
}

/// Reads the shipped memory configuration lookups that change the database or
/// the retrieval behaviour.
pub(super) fn memory_config_is_dynamic() -> bool {
    if std::env::vars_os().any(|(name, _)| name.to_string_lossy().starts_with("SYMMEMORY_")) {
        return true;
    }
    let config_root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| symbrain_core::xdg::home_dir().map(|home| home.join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    config_root.join("symmemory/config.toml").is_file()
        || std::env::current_dir().is_ok_and(|dir| dir.join(".symmemory.toml").is_file())
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
