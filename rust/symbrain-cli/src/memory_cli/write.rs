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
    process_stdout: bool,
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

    // Routing admits the deterministic checker only. Staged writes bypass it;
    // provider verdicts, redaction and secondary extraction remain delegated.
    let config = super::config::load();
    let request = symbrain_memory::DirectWrite {
        content: content.to_owned(),
        scope: scope.clone(),
        kind: kind.to_owned(),
        metadata: parse_metadata(&parsed.text("metadata")).unwrap_or_default(),
        author: parsed.text("author"),
        staged,
        quantize_binary: config.quantize_binary,
        conflict_enabled: config.conflict_enabled,
        entities: parsed
            .text("entities")
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect(),
    };
    let generator =
        symbrain_memory::EmbeddingGenerator::new(&config.ollama_url, &config.ollama_model);
    match store.set_direct_cli_with_conflict(
        &request,
        &generator,
        config.conflict_contradiction.unwrap_or(0.80),
        config.conflict_near_dup.unwrap_or(0.95),
        config.conflict_max_candidates.unwrap_or(10),
        // run_stdio owns both real streams; embedded callers pass false.
        &mut |message, attributes| conflict_diagnostic(stderr, process_stdout, message, attributes),
    ) {
        Ok(id) => super::write_output::finish_set(
            super::write_output::SetReply {
                id: &id,
                scope: &scope,
                kind,
                staged,
            },
            stdout,
            stderr,
            format,
            process_stdout,
        ),
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
/// extraction, provider verdicts and invalid metadata JSON diagnostics.
pub(super) fn direct_write_supported(parsed: &flags::Args) -> bool {
    // Go can store raw non-UTF8 actor/entity bytes in SQLite. The native
    // string model cannot claim those states by silently replacing the bytes.
    if parsed.raw("author").to_str().is_none() || parsed.raw("entities").to_str().is_none() {
        return false;
    }
    let config = super::config::load();
    if !config.deterministic_conflict_supported(parsed.staged) {
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

fn conflict_diagnostic(stderr: &mut dyn Write, process_stdio: bool, message: &str, attributes: &[(&str, &str)]) {
    // The frozen executable uses slog's default handler and log.LstdFlags,
    // not logkit's separate configured logger. os.Stderr applies epipecheck
    // before slog can ignore the error, just as os.Stdout does.
    let mut line = format!("{} {message}", chrono::Local::now().format("%Y/%m/%d %H:%M:%S"));
    for (name, value) in attributes {
        line.push_str(&format!(" {name}={}", diagnostic_value(value)));
    }
    if let Err(error) = writeln!(stderr, "{line}") {
        crate::stdio_output::pipe_boundary(&error, process_stdio);
    }
}

fn diagnostic_value(value: &str) -> String {
    let quoted = symbrain_core::config::format_go_quoted_bytes(value.as_bytes());
    // Backslash alone does not require quoting in slog's text handler. Any
    // other Go quote escape, Unicode space, ASCII space or '=' does.
    let plain_quoted = format!("\"{}\"", value.replace('\\', "\\\\"));
    if value.is_empty() || quoted != plain_quoted
        || value.chars().any(|ch| ch.is_whitespace() || ch == '=' || ch == '\u{fffd}')
    {
        quoted
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn diagnostics_keep_go_quoting_and_ignore_failed_sink() {
        for (value, expected) in [("id", "id"), ("a\\b", "a\\b"), ("", "\"\""),
            ("a=b", "\"a=b\""), ("a\n", "\"a\\n\""), ("\u{a0}", "\"\\u00a0\""),
            ("\u{fffd}", "\"\u{fffd}\"")] {
            assert_eq!(diagnostic_value(value), expected);
        }
        struct Failed(Option<i32>);
        impl std::io::Write for Failed {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                Err(self.0.map_or_else(
                    || std::io::ErrorKind::BrokenPipe.into(),
                    std::io::Error::from_raw_os_error,
                ))
            }
            fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
        }
        // Injected writers never signal, even with a raw EPIPE. A synthetic
        // BrokenPipe and non-EPIPE OS errors also stay best-effort in stdio.
        for (code, process_stdio) in [(None, false), (None, true), (Some(32), false), (Some(28), true)] {
            conflict_diagnostic(&mut Failed(code), process_stdio,
                "INFO conflict: duplicate write deduplicated", &[("id", "existing")]);
        }
    }
}
