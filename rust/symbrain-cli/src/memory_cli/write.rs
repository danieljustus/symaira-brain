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

    // This route is gated to non-redacting, extraction-free writes with no
    // conflict checker (staged writes bypass it in Go regardless of config).
    let request = symbrain_memory::DirectWrite {
        content: content.to_owned(),
        scope: scope.clone(),
        kind: kind.to_owned(),
        metadata: parse_metadata(&parsed.text("metadata")).unwrap_or_default(),
        author: parsed.text("author"),
        staged,
        quantize_binary: super::config::load().quantize_binary,
        conflict_enabled: super::config::load().conflict_enabled,
        entities: parsed
            .text("entities")
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
    };
    let config = super::config::load();
    let generator =
        symbrain_memory::EmbeddingGenerator::new(&config.ollama_url, &config.ollama_model);
    match store.set_direct_cli(&request, &generator) {
        Ok(id) => {
            match format {
                OutputFormat::Json => {
                    // Shipped shape: compact, `id`, `scope`, `kind`, `staged`.
                    let _ = writeln!(
                        stdout,
                        "{{\"id\":{},\"scope\":{},\"kind\":{},\"staged\":{}}}",
                        go_json_string(&id),
                        go_json_string(&scope),
                        go_json_string(kind),
                        staged
                    );
                }
                OutputFormat::Table => {
                    let state = if staged {
                        "staged for review"
                    } else {
                        "stored"
                    };
                    let _ = writeln!(stdout, "Memory {id} ({scope}, {kind}, {state}).");
                }
            }
            exit::OK
        }
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory set: store memory: {err}");
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

    match store.delete_cli(id) {
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

/// Admission is conservative: untouched Go remains the oracle for redaction,
/// extraction, conflict resolution and invalid metadata JSON diagnostics.
pub(super) fn direct_write_supported(parsed: &flags::Args) -> bool {
    // Go can store raw non-UTF8 actor/entity bytes in SQLite. The native
    // string model cannot claim those states by silently replacing the bytes.
    if parsed.raw("author").to_str().is_none() || parsed.raw("entities").to_str().is_none() {
        return false;
    }
    let config = super::config::load();
    if !parsed.staged && config.conflict_enabled {
        return false;
    }
    let content = flags::go_string(&flags::bytes(&parsed.positional[0]));
    if !symbrain_memory::direct_content_supported(content.trim()) {
        return false;
    }
    if parsed.text("scope") == "project" && !symbrain_memory::direct_project_supported() {
        return false;
    }
    let Some(meta) = parse_metadata(&parsed.text("metadata")) else {
        return false;
    };
    meta.values()
        .all(|value| symbrain_memory::direct_text_supported(value))
        && ["", "global", "project", "agent", "user", "session"]
            .contains(&parsed.text("scope").as_str())
}

fn parse_metadata(raw: &str) -> Option<std::collections::BTreeMap<String, String>> {
    if raw.trim().is_empty() {
        return Some(std::collections::BTreeMap::new());
    }
    // Typed decoding validates every duplicate occurrence before replacement.
    // A wrong value followed by a string is still an error in Go Unmarshal.
    let values =
        serde_json::from_str::<Option<std::collections::BTreeMap<String, Option<String>>>>(raw)
            .ok()?;
    Some(
        values
            .unwrap_or_default()
            .into_iter()
            .map(|(key, value)| (key, value.unwrap_or_default()))
            .collect(),
    )
}
