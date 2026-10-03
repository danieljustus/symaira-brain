//! Checked Set rendering occurs after the store's committed governance writes.

use super::{OutputFormat, Write, exit, write::go_json_string};

#[derive(Clone, Copy)]
pub(super) struct SetReply<'a> {
    pub id: &'a str,
    pub scope: &'a str,
    pub kind: &'a str,
    pub staged: bool,
}

pub(super) fn finish_set(
    reply: SetReply<'_>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
    format: OutputFormat,
    process_stdout: bool,
) -> u8 {
    let SetReply {
        id,
        scope,
        kind,
        staged,
    } = reply;
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
            #[cfg(unix)]
            if process_stdout
                && error.raw_os_error().is_some()
                && error.kind() == std::io::ErrorKind::BrokenPipe
            {
                // Rust ignores SIGPIPE at startup. Emulate Go's actual fd1
                // completion only after EPIPE, after the write is committed.
                let _ =
                    signal_hook::low_level::emulate_default_handler(signal_hook::consts::SIGPIPE);
            }
            #[cfg(not(unix))]
            let _ = process_stdout;
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

    struct BrokenPipeWriter {
        os_error: bool,
    }

    impl Write for BrokenPipeWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            if self.os_error {
                #[cfg(unix)]
                let code = 32;
                #[cfg(not(unix))]
                let code = 109;
                Err(std::io::Error::from_raw_os_error(code))
            } else {
                Err(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "owned injected broken pipe",
                ))
            }
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn injected_broken_pipe_does_not_terminate_the_library_caller() {
        for format in [OutputFormat::Json, OutputFormat::Table] {
            for os_error in [false, true] {
                let mut stderr = Vec::new();
                assert_eq!(
                    finish_set(
                        SetReply {
                            id: "owned",
                            scope: "global",
                            kind: "reference",
                            staged: false
                        },
                        &mut BrokenPipeWriter { os_error },
                        &mut stderr,
                        format,
                        false,
                    ),
                    exit::GENERIC
                );
                assert!(stderr.starts_with(b"symbrain memory set: format output: "));
                #[cfg(unix)]
                if os_error {
                    assert_eq!(
                        stderr,
                        b"symbrain memory set: format output: write /dev/stdout: broken pipe\n"
                    );
                }
                if !os_error {
                    assert_eq!(
                        stderr,
                        b"symbrain memory set: format output: owned injected broken pipe\n"
                    );
                    let mut stderr = Vec::new();
                    // Even the process boundary only terminates for real OS
                    // EPIPE, not a custom error that happens to share its kind.
                    assert_eq!(
                        finish_set(
                            SetReply {
                                id: "owned",
                                scope: "global",
                                kind: "reference",
                                staged: false
                            },
                            &mut BrokenPipeWriter { os_error },
                            &mut stderr,
                            format,
                            true,
                        ),
                        exit::GENERIC
                    );
                    assert_eq!(
                        stderr,
                        b"symbrain memory set: format output: owned injected broken pipe\n"
                    );
                }
            }
        }
    }

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
                    SetReply {
                        id: &id,
                        scope: "global",
                        kind: "reference",
                        staged: false
                    },
                    &mut FailingWriter,
                    &mut stderr,
                    format,
                    false,
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
