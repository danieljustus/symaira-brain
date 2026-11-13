//! Actual process stdout EPIPE terminates Set only after committed persistence.

#[test]
fn actual_stdout_closed_reader_preserves_committed_set_and_sigpipe() {
    use std::os::unix::process::ExitStatusExt;
    use std::process::{Command, Stdio};

    for format in ["json", "table"] {
        let root = tempfile::tempdir().expect("owned root");
        let path = root.path().join("memory.db");
        drop(symbrain_memory::Store::open(&path).expect("existing owned store"));
        // Close the read end before the child starts: dropping a piped
        // stdout after spawn races the short reply into the pipe buffer.
        let (reader, writer) = std::io::pipe().expect("stdout pipe");
        drop(reader);
        let result = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .args(["memory", "set", "hello world", "--db"])
            .arg(&path)
            .args([
                "--kind",
                "fact",
                "--staged",
                "--metadata",
                "{\"fixture\":\"value\"}",
                "--output",
                format,
            ])
            .env_clear()
            .env("HOME", root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .env("XDG_DATA_HOME", root.path().join("data"))
            .env("XDG_CACHE_HOME", root.path().join("cache"))
            .env("PATH", "")
            .env("SYMBRAIN_GO_BINARY", root.path().join("absent-go"))
            // The typed string is allowed, but lacks a URL scheme: hash
            // fallback performs no request to an operator-owned endpoint.
            .env("SYMMEMORY_OLLAMA_URL", "owned-invalid-endpoint")
            .current_dir(root.path())
            .stdin(Stdio::null())
            .stdout(writer)
            .stderr(Stdio::piped())
            .output()
            .expect("actual CLI child");
        assert_eq!(result.status.signal(), Some(signal_hook::consts::SIGPIPE));
        assert!(result.stderr.is_empty(), "{:?}", result.stderr);

        let store = symbrain_memory::Store::open(&path).expect("committed database");
        let candidates = store.candidates(10).expect("committed staged row");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].kind, "reference");
        assert_eq!(candidates[0].metadata["fixture"], "value");
        assert_eq!(candidates[0].content, "hello world");
        assert!(store.list("", 10).expect("approved projection").is_empty());
    }
}
