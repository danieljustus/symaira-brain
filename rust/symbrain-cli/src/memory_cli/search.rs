//! Native memory search contracts.

use super::{
    MEMORY_SEARCH_USAGE, MEMORY_SEARCH_USAGE_ERROR, OsString, OutputFormat, Write, exit, flags,
    open_store, table_content,
};

pub(super) fn run_search(
    args: &[OsString],
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    if flags::has_help(args) {
        let _ = write!(stdout, "{MEMORY_SEARCH_USAGE}");
        return exit::OK;
    }
    let Ok(parsed) = flags::parse(args, "search", stderr) else {
        return exit::USAGE;
    };
    let mut limit = parsed.limit;
    let scope = parsed.text("scope");
    if parsed.positional.len() != 1 {
        let _ = write!(stderr, "{MEMORY_SEARCH_USAGE_ERROR}");
        return exit::USAGE;
    }
    let query = flags::go_string(&flags::bytes(&parsed.positional[0]));
    if query.is_empty() {
        let _ = writeln!(stderr, "symbrain memory search: query is required");
        return exit::USAGE;
    }
    if limit <= 0 {
        limit = 5;
    }

    let store = match open_store(stderr, Some(&parsed.raw("db"))) {
        Ok(store) => store,
        Err(code) => return code,
    };

    let config = super::config::load();
    let embedding =
        symbrain_memory::EmbeddingGenerator::new(&config.ollama_url, &config.ollama_model)
            .generate(&query);
    let mut hits = match store.search_ranked(
        &embedding.vector,
        &embedding.source,
        scope.as_str(),
        usize::try_from(limit).unwrap_or(usize::MAX),
    ) {
        Ok(hits) => hits,
        Err(err) => {
            let _ = writeln!(stderr, "symbrain memory search: search memories: {err}");
            return exit::GENERIC;
        }
    };
    // The shipped CLI redacts at the response boundary, on top of the
    // write-time redaction.
    for hit in &mut hits {
        hit.redact();
    }

    match format {
        OutputFormat::Json => {
            let rendered = hits
                .iter()
                .map(symbrain_memory::SearchHit::to_go_json)
                .collect::<Vec<_>>()
                .join(",");
            let _ = writeln!(stdout, "{}", escape_html(&format!("[{rendered}]")));
        }
        OutputFormat::Table => {
            if hits.is_empty() {
                let _ = writeln!(stdout, "No relevant memories found.");
            } else {
                let _ = writeln!(stdout, "SCORE\tID\tSCOPE\tCREATED\tCONTENT");
                for hit in &hits {
                    let _ = writeln!(
                        stdout,
                        "{:.6}\t{}\t{}\t{}\t{}",
                        f64::from(hit.score),
                        hit.memory.id,
                        hit.memory.scope,
                        hit.memory.created_at_seconds(),
                        table_content(&hit.memory.content)
                    );
                }
            }
        }
    }

    exit::OK
}

/// Escapes `&`, `<`, `>` and the two JavaScript line separators the way Go's
/// `json.Encoder` does by default.
pub(super) fn escape_html(json: &str) -> String {
    json.replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}
