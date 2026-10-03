//! Checked Set rendering occurs after the store's committed governance writes.

use super::{OutputFormat, Write, exit, write::go_json_string};

pub(super) fn finish_set(
    id: &str,
    scope: &str,
    kind: &str,
    staged: bool,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
) -> u8 {
    let result = match format {
        OutputFormat::Json => writeln!(
            stdout,
            "{{\"id\":{},\"scope\":{},\"kind\":{},\"staged\":{}}}",
            go_json_string(id),
            go_json_string(scope),
            go_json_string(kind),
            staged
        ),
        OutputFormat::Table => {
            let state = if staged {
                "staged for review"
            } else {
                "stored"
            };
            writeln!(stdout, "Memory {id} ({scope}, {kind}, {state}).")
        }
    };
    match result {
        Ok(()) => exit::OK,
        Err(error) => {
            let _ = writeln!(
                stderr,
                "symbrain memory set: format output: {}",
                go_write_error(&error)
            );
            exit::GENERIC
        }
    }
}

fn go_write_error(error: &std::io::Error) -> String {
    let Some(code) = error.raw_os_error() else {
        return error.to_string();
    };
    let literal = error.to_string();
    let suffix = format!(" (os error {code})");
    let message = literal.strip_suffix(&suffix).unwrap_or(&literal);
    // Go's Unix errno strings are lowercase; Windows retains FormatMessage.
    #[cfg(unix)]
    let message = message.to_lowercase();
    format!("write /dev/stdout: {message}")
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("owned writer failure"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn output_failure_reports_error_after_committed_set() {
        for format in [OutputFormat::Json, OutputFormat::Table] {
            let store = symbrain_memory::Store::open_in_memory().expect("owned store");
            let request = symbrain_memory::DirectWrite {
                content: "hello world".into(),
                scope: "global".into(),
                kind: "reference".into(),
                metadata: [("fixture".into(), "value".into())].into(),
                author: "cli:symbrain".into(),
                entities: vec![],
                staged: false,
                quantize_binary: false,
                conflict_enabled: false,
            };
            // No URL scheme: hash fallback without contacting any endpoint.
            let generator =
                symbrain_memory::EmbeddingGenerator::new("owned-invalid-endpoint", "owned-model");
            let id = store
                .set_direct_cli(&request, &generator)
                .expect("committed set");
            let mut stderr = Vec::new();
            assert_eq!(
                finish_set(
                    &id,
                    "global",
                    "reference",
                    false,
                    &mut FailingWriter,
                    &mut stderr,
                    format
                ),
                exit::GENERIC
            );
            assert_eq!(
                stderr,
                b"symbrain memory set: format output: owned writer failure\n"
            );
            let rows = store.list("", 10).expect("committed row remains");
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].id, id);
            assert_eq!(rows[0].kind, "reference");
        }
    }
}
