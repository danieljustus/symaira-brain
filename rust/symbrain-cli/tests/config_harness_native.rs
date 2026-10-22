//! Native replay of Go-recorded `config` / `harness` argv-grammar shapes (#769).
//!
//! `fixtures/config_harness_go_shapes.json` is written by
//! `scripts/config-harness-oracle/replay.py --record-fixture` from the real Go
//! CLI at the pinned oracle revision. Every Rust process runs with an absent Go
//! fallback, so a shape that still routes to Go fails here without needing a
//! Go toolchain.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::ffi::OsString;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

const FIXTURE: &str = include_str!("fixtures/config_harness_go_shapes.json");

fn from_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

fn os_arg(bytes: Vec<u8>) -> OsString {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        OsString::from_vec(bytes)
    }
    #[cfg(not(unix))]
    {
        OsString::from(String::from_utf8(bytes).expect("non-raw fixture argv is UTF-8"))
    }
}

fn expected(case: &Value, stream: &str) -> Vec<u8> {
    case.get(stream)
        .and_then(Value::as_str)
        .map(|text| text.as_bytes().to_vec())
        .or_else(|| {
            case.get(format!("{stream}_hex"))
                .and_then(Value::as_str)
                .map(from_hex)
        })
        .unwrap_or_else(|| panic!("{case}: missing {stream}"))
}

#[test]
fn go_recorded_shapes_are_native_without_a_go_fallback() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.len() >= 90, "fixture shrank to {} cases", cases.len());
    let mut executed = 0;
    for case in cases {
        let name = case["name"].as_str().unwrap();
        if cfg!(not(unix)) && case["raw_bytes"].as_bool().unwrap() {
            eprintln!("SKIP {name}: raw-byte argv is not representable on this platform");
            continue;
        }
        let root = TempDir::new().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        for dir in ["home", "config", "data", "cache", "project"] {
            std::fs::create_dir_all(root_path.join(dir)).unwrap();
        }
        let args: Vec<OsString> = case["argv_hex"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| os_arg(from_hex(arg.as_str().unwrap())))
            .collect();
        let output = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", root_path.join("home"))
            .env("USERPROFILE", root_path.join("home"))
            .env("XDG_CONFIG_HOME", root_path.join("config"))
            .env("XDG_DATA_HOME", root_path.join("data"))
            .env("XDG_CACHE_HOME", root_path.join("cache"))
            .env("PATH", "/usr/bin:/bin")
            .env("SYMBRAIN_GO_BINARY", root_path.join("absent-go-fallback"))
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .current_dir(root_path.join("project"))
            .args(&args)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(i32::try_from(case["exit"].as_i64().unwrap()).unwrap()),
            "{name}: exit"
        );
        assert_eq!(output.stdout, expected(case, "stdout"), "{name}: stdout");
        assert_eq!(output.stderr, expected(case, "stderr"), "{name}: stderr");
        let written = std::fs::read_dir(root_path.join("config")).unwrap().count();
        assert_eq!(
            written, 0,
            "{name}: flag-grammar shapes must not write config"
        );
        executed += 1;
    }
    let raw = cases
        .iter()
        .filter(|case| case["raw_bytes"].as_bool().unwrap())
        .count();
    let expected_executed = if cfg!(unix) {
        cases.len()
    } else {
        cases.len() - raw
    };
    assert_eq!(executed, expected_executed);
}

/// Runs the native CLI in a disposable root seeded like the oracle's
/// `setup_seeded_harness`: a stdio server `a` with a missing command and an
/// HTTP server `b` on a closed loopback port in `~/.claude.json`, plus a
/// project-local stdio server `p` in `project/.mcp.json`.
fn seeded_output(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_symbrain"))
        .env_clear()
        .envs(coverage::profile_environment())
        .env("HOME", root.join("home"))
        .env("USERPROFILE", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .env("SYMBRAIN_GO_BINARY", root.join("absent-go-fallback"))
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(root.join("project"))
        .args(args)
        .output()
        .unwrap();
    assert!(
        started.elapsed() < std::time::Duration::from_secs(8),
        "{args:?}: probe was not bounded"
    );
    output
}

fn seeded_root() -> (TempDir, std::path::PathBuf) {
    let root = TempDir::new().unwrap();
    let path = root.path().canonicalize().unwrap();
    for dir in ["home", "config", "data", "cache", "project"] {
        std::fs::create_dir_all(path.join(dir)).unwrap();
    }
    std::fs::write(
        path.join("home/.claude.json"),
        r#"{"mcpServers":{"a":{"command":"symaira-missing-mcp-fixture","args":["x"]},"b":{"type":"http","url":"http://127.0.0.1:1/mcp"}}}"#,
    )
    .unwrap();
    std::fs::write(
        path.join("project/.mcp.json"),
        r#"{"mcpServers":{"p":{"command":"symaira-missing-mcp-fixture"}}}"#,
    )
    .unwrap();
    (root, path)
}

fn server_rows(stdout: &[u8]) -> Vec<(String, String)> {
    String::from_utf8(stdout.to_vec())
        .unwrap()
        .lines()
        .map(|line| {
            let mut fields = line.split_whitespace();
            fields.next().expect("status glyph");
            let harness = fields.next().expect("harness").to_owned();
            (harness, fields.next().expect("server").to_owned())
        })
        .collect()
}

/// Go's probeHarnessHealth only filters on a non-empty `--harness` value, so an
/// empty value (like an empty `--project`) means "no filter" (#769 review F1).
#[test]
fn empty_harness_filter_probes_every_server_like_go() {
    let (_guard, root) = seeded_root();
    let all = seeded_output(&root, &["harness", "health", "-project", "."]);
    assert_eq!(all.status.code(), Some(0));
    let want = [("claude", "a"), ("claude", "b"), ("claude", "p")]
        .map(|(harness, server)| (harness.to_owned(), server.to_owned()));
    assert_eq!(server_rows(&all.stdout), want);

    for args in [
        &["-harness="][..],
        &["-harness", ""],
        &["--harness", ""],
        &["-harness", "claude", "-harness="],
        &["-harness=", "-harness", "claude"],
        &["-harness", "claude"],
    ] {
        let mut argv = vec!["harness", "health", "-project", "."];
        argv.extend_from_slice(args);
        let output = seeded_output(&root, &argv);
        assert_eq!(output.status.code(), Some(0), "{argv:?}");
        assert_eq!(output.stdout, all.stdout, "{argv:?}");
    }
    for args in [&["-harness", "nope"][..], &["-harness", "cursor"]] {
        let mut argv = vec!["harness", "health", "-project", "."];
        argv.extend_from_slice(args);
        let output = seeded_output(&root, &argv);
        assert_eq!(output.status.code(), Some(0), "{argv:?}");
        assert_eq!(output.stdout, b"no MCP servers found\n", "{argv:?}");
    }
}

/// Go's `filepath.Join` cleans the displayed project config path, so a relative
/// `--project` never prints `./.mcp.json` (#769 review F2).
#[test]
fn relative_project_config_path_is_cleaned_like_go() {
    let (_guard, root) = seeded_root();
    for project in [".", "./", "./x/../"] {
        let output = seeded_output(&root, &["harness", "list", "-project", project]);
        assert_eq!(output.status.code(), Some(0), "{project:?}");
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(
            text.contains("  project\t.mcp.json\tparsed\tservers=p[stdio]\n"),
            "{project:?}: {text}"
        );
        assert!(!text.contains("./.mcp.json"), "{project:?}: {text}");
    }
    let health = seeded_output(&root, &["harness", "health", "-project", "./"]);
    let text = String::from_utf8(health.stdout).unwrap();
    assert!(text.contains(" .mcp.json: "), "{text}");
    assert!(!text.contains("./.mcp.json"), "{text}");
}
