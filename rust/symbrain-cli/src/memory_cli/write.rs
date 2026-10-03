//! Native memory write contracts.

use super::{
    MEMORY_DELETE_USAGE, MEMORY_DELETE_USAGE_ERROR, MEMORY_SET_USAGE, MEMORY_SET_USAGE_ERROR,
    OsString, OutputFormat, Write, exit, flags, go_json, kind, open_store,
};

pub(super) fn run_set(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if flags::has_help(args) {
        let _ = write!(stdout, "{MEMORY_SET_USAGE}");
        return exit::OK;
    }
    let Ok(parsed) = flags::parse(args, "set", stderr) else {
        return exit::USAGE;
    };
    if parsed.positional.len() != 1 {
        let _ = write!(stderr, "{MEMORY_SET_USAGE_ERROR}");
        return exit::USAGE;
    }
    let content = flags::go_string(&flags::bytes(&parsed.positional[0]));
    let content = content.trim();
    if content.is_empty() {
        let _ = writeln!(stderr, "symbrain memory set: content is required");
        return exit::USAGE;
    }
    let raw_kind = parsed.text("kind");
    if raw_kind.is_empty() {
        let _ = writeln!(
            stderr,
            "symbrain memory set: --kind is required (one of: {})",
            kind::VALID
        );
        return exit::USAGE;
    }
    let Some(kind) = kind::normalize(&raw_kind) else {
        let _ = writeln!(
            stderr,
            "symbrain memory set: invalid kind {} (valid: {})",
            symbrain_core::config::format_go_quoted(&parsed.raw("kind")),
            kind::VALID
        );
        return exit::USAGE;
    };
    let scope = parsed.text("scope");
    let staged = parsed.staged;
    let store = match open_store(stderr, Some(&parsed.raw("db"))) {
        Ok(s) => s,
        Err(code) => return code,
    };

    // The shipped CLI records `cli:symbrain` as the actor; the MCP surface
    // records `mcp`.
    let options = symbrain_memory::SetOptions {
        actor: parsed.text("author"),
        staged,
        ..symbrain_memory::SetOptions::default()
    };
    match store.set_with_options(content, &scope, kind, serde_json::Map::new(), &options) {
        Ok(mem) => {
            match format {
                OutputFormat::Json => {
                    // Shipped shape: compact, `id`, `scope`, `kind`, `staged`.
                    let _ = writeln!(
                        stdout,
                        "{{\"id\":{},\"scope\":{},\"kind\":{},\"staged\":{}}}",
                        go_json_string(&mem.id),
                        go_json_string(&mem.scope),
                        go_json_string(&mem.kind),
                        staged
                    );
                }
                OutputFormat::Table => {
                    let state = if staged {
                        "staged for review"
                    } else {
                        "stored"
                    };
                    let _ = writeln!(
                        stdout,
                        "Memory {} ({}, {}, {}).",
                        mem.id, mem.scope, mem.kind, state
                    );
                }
            }
            exit::OK
        }
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory set: {err}");
            exit::GENERIC
        }
    }
}

pub(super) fn run_delete(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if flags::has_help(args) {
        let _ = write!(stdout, "{MEMORY_DELETE_USAGE}");
        return exit::OK;
    }
    let Ok(parsed) = flags::parse(args, "delete", stderr) else {
        return exit::USAGE;
    };
    if parsed.positional.len() != 1 {
        let _ = write!(stderr, "{MEMORY_DELETE_USAGE_ERROR}");
        return exit::USAGE;
    }
    let id = flags::go_string(&flags::bytes(&parsed.positional[0]));
    let id = id.trim();
    if id.is_empty() {
        let _ = writeln!(stderr, "symbrain memory delete: id is required");
        return exit::USAGE;
    }
    let store = match open_store(stderr, Some(&parsed.raw("db"))) {
        Ok(store) => store,
        Err(code) => return code,
    };

    match store.delete(id) {
        Ok(true) => {
            match format {
                OutputFormat::Json => {
                    // Shipped shape: compact, `id` before `deleted`.
                    let _ = writeln!(stdout, "{{\"id\":{},\"deleted\":true}}", go_json_string(id));
                }
                OutputFormat::Table => {
                    let _ = writeln!(stdout, "Deleted memory {id}.");
                }
            }
            exit::OK
        }
        Ok(false) => {
            let _ = writeln!(stderr, "symbrain memory delete: memory not found: {id}");
            exit::GENERIC
        }
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory delete: {err}");
            exit::GENERIC
        }
    }
}

/// Renders a string as the shipped encoder does, including its HTML escaping.
pub(super) fn go_json_string(value: &str) -> String {
    go_json(&value.to_owned())
}
