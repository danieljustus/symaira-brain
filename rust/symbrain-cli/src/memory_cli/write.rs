//! Native memory write contracts.

use super::{
    MEMORY_DELETE_USAGE, MEMORY_DELETE_USAGE_ERROR, MEMORY_SET_USAGE, MEMORY_SET_USAGE_ERROR,
    OsString, OutputFormat, Write, exit, flags, go_json, kind, open_store,
};

/// The write-path conflict configuration of the loaded memory config.
pub(super) fn conflict_config(config: &super::config::Config) -> symbrain_memory::ConflictConfig {
    symbrain_memory::ConflictConfig {
        enabled: config.conflict_enabled,
        contradiction_threshold: config.conflict_contradiction,
        near_dup_threshold: config.conflict_near_dup,
        max_candidates: config.conflict_max_candidates,
        llm: symbrain_memory::LlmSettings {
            provider: config.conflict_llm_provider.clone(),
            url: config.conflict_llm_url.clone(),
            model: config.conflict_llm_model.clone(),
        },
    }
}

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
    let content = flags::bytes(&parsed.positional[0]);
    let content = trim_space(&content);
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

    let mut metadata = std::collections::BTreeMap::new();
    let raw_metadata = parsed.bytes_of("metadata");
    if !flags::go_string(raw_metadata).trim().is_empty() {
        match symbrain_memory::gojson_decode::decode_string_map(raw_metadata) {
            Ok(decoded) => metadata = decoded.unwrap_or_default(),
            Err(error) => {
                let _ = writeln!(
                    stderr,
                    "symbrain memory set: --metadata must be a JSON object: {error}"
                );
                return exit::USAGE;
            }
        }
    }
    let entities = parsed
        .bytes_of("entities")
        .split(|byte| *byte == b',')
        .map(|name| trim_space(name).to_vec())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>();

    let store = match open_store(stderr, "set", Some(&parsed.raw("db"))) {
        Ok(s) => s,
        Err(code) => return code,
    };

    let config = super::config::load();
    let request = symbrain_memory::CliSet {
        content: content.to_vec(),
        scope: parsed.bytes_of("scope").to_vec(),
        kind: kind.to_owned(),
        metadata,
        author: parsed.bytes_of("author").to_vec(),
        entities,
        staged,
        quantize_binary: config.quantize_binary,
        prefilter: config.prefilter,
        conflict: conflict_config(config),
    };
    let generator =
        symbrain_memory::EmbeddingGenerator::new(&config.ollama_url, &config.ollama_model);
    let mut log = symbrain_memory::LogSink(stderr);
    let result = store.set_cli(&request, &generator, &mut log);
    let stderr = log.0;
    match result {
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

/// `strings.TrimSpace` over raw bytes: invalid bytes are not space, so they
/// stop the trim on either side.
pub(super) fn trim_space(bytes: &[u8]) -> &[u8] {
    let mut start = 0;
    while let Some(character) = bytes[start..]
        .utf8_chunks()
        .next()
        .and_then(|chunk| chunk.valid().chars().next())
    {
        if !character.is_whitespace() {
            break;
        }
        start += character.len_utf8();
    }
    let mut end = bytes.len();
    while end > start {
        let Some(chunk) = bytes[start..end].utf8_chunks().last() else {
            break;
        };
        if !chunk.invalid().is_empty() {
            break;
        }
        match chunk.valid().chars().next_back() {
            Some(character) if character.is_whitespace() => end -= character.len_utf8(),
            _ => break,
        }
    }
    &bytes[start..end]
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
    let store = match open_store(stderr, "delete", Some(&parsed.raw("db"))) {
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
