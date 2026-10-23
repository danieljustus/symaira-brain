//! Full transport → gateway → ordered Skills outer-decoder regressions.
//!
//! Slice (a) of #764 wires the raw layer into the six Skills tools that main's
//! gateway implements; the other five of Go's eleven are not exposed yet.
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

const IMPLEMENTED: [&str; 6] = [
    "skills_list",
    "skills_inspect",
    "skills_validate",
    "skills_render_plan",
    "skills_install",
    "skills_targets_status",
];

fn run(enabled: bool, tool: &str, arguments: &str, framed: bool) -> Vec<Value> {
    let call = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#
    );
    run_raw(enabled, &call, framed, false)
}

fn run_raw(enabled: bool, call: &str, framed: bool, unchanged: bool) -> Vec<Value> {
    let root = tempfile::tempdir().unwrap();
    for directory in ["home", "config", "data", "cache", "project", "tmp"] {
        std::fs::create_dir(root.path().join(directory)).unwrap();
    }
    let profile = root.path().join("profile.toml");
    std::fs::write(&profile, format!("[profile]\nname = \"raw-skills\"\n[servers.vault]\nenabled = false\n[servers.memory]\nenabled = false\n[servers.usage]\nenabled = false\n[servers.skills]\nenabled = {enabled}\n[audit]\nenabled = false\n")).unwrap();
    let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    if unchanged {
        let library = root.path().join("data/symbrain/skills/library/demo");
        std::fs::create_dir_all(&library).unwrap();
        std::fs::write(
            library.join("SKILL.md"),
            b"---\nname: demo\ndescription: installable fixture\n---\nbody\n",
        )
        .unwrap();
    }
    let before = unchanged.then(|| tree(root.path()));
    let mut input = Vec::new();
    for body in [list, call] {
        if framed {
            write!(&mut input, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        } else {
            writeln!(&mut input, "{body}").unwrap();
        }
    }
    // An instrumented child (`cargo llvm-cov`) writes its profile to the cwd,
    // which is inside the tree whose immutability this test asserts, unless
    // `LLVM_PROFILE_FILE` survives `env_clear`. Forward the collector's
    // absolute path; otherwise direct any profile outside the isolated root.
    let outside = tempfile::tempdir().unwrap();
    let profile_data = std::env::var_os("LLVM_PROFILE_FILE")
        .unwrap_or_else(|| outside.path().join("child-%m-%p.profraw").into());
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .env("LLVM_PROFILE_FILE", profile_data)
        .env("PATH", "")
        .env("HOME", root.path().join("home"))
        .env("USERPROFILE", root.path().join("home"))
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("TMPDIR", root.path().join("tmp"))
        .env("TMP", root.path().join("tmp"))
        .env("TEMP", root.path().join("tmp"))
        .current_dir(root.path().join("project"))
        .args(["mcp", "--profile-file"])
        .arg(profile)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for key in ["SystemRoot", "windir"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = command.spawn().unwrap();
    child.stdin.take().unwrap().write_all(&input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    if let Some(before) = before {
        assert_eq!(
            tree(root.path()),
            before,
            "malformed request mutated isolated state"
        );
    }
    if !framed {
        return output
            .stdout
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
    }
    let mut remaining = output.stdout.as_slice();
    let mut replies = Vec::new();
    while !remaining.is_empty() {
        let split = remaining
            .windows(4)
            .position(|bytes| bytes == b"\r\n\r\n")
            .unwrap();
        let header = std::str::from_utf8(&remaining[..split]).unwrap();
        let size: usize = header
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        remaining = &remaining[split + 4..];
        replies.push(serde_json::from_slice(&remaining[..size]).unwrap());
        remaining = &remaining[size..];
    }
    replies
}

#[test]
fn enabled_and_disabled_catalog_retain_full_go_order_and_exposure() {
    for enabled in [false, true] {
        for framed in [false, true] {
            let replies = run(enabled, "skills_list", r#"{"ignored":1e9999}"#, framed);
            assert_eq!(replies.len(), 2);
            let names: Vec<_> = replies[0]["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|tool| {
                    tool["name"]
                        .as_str()
                        .filter(|name| name.starts_with("skills_"))
                })
                .collect();
            if enabled {
                // Go registers eleven; main exposes the implemented six, in Go's
                // relative order (the remaining five are slice (b)).
                let implemented: Vec<_> = symbrain_mcp::SKILLS_TOOL_NAMES
                    .iter()
                    .copied()
                    .filter(|name| IMPLEMENTED.contains(name))
                    .collect();
                assert_eq!(names, implemented);
                assert_eq!(replies[1]["result"]["isError"], false);
            } else {
                assert!(names.is_empty());
                assert_eq!(replies[1]["error"]["code"], -32601);
            }
        }
    }
}

#[test]
fn surrogate_values_keys_and_ignored_numbers_reach_real_skills_handlers() {
    for framed in [false, true] {
        // A lone surrogate becomes U+FFFD (as in Go) and reaches the handler,
        // which then reports the unknown library entry; it is not rejected by
        // the outer decoder. An ignored out-of-range number in a key/value pair
        // is skipped, not an error.
        for arguments in [
            r#"{"name":"\ud800"}"#,
            r#"{"\ud800":1e9999,"name":"\ud800"}"#,
        ] {
            let replies = run(true, "skills_inspect", arguments, framed);
            assert_eq!(replies.len(), 2);
            assert_eq!(replies[1]["result"]["isError"], true, "{arguments}");
            let text = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
            assert!(
                text.ends_with("skill '\u{fffd}' not found in library"),
                "{arguments}: {text}"
            );
        }
        let replies = run(true, "skills_list", r#"{"ignored":1e9999}"#, framed);
        assert_eq!(replies[1]["result"]["isError"], false);
        // A typed envelope error is reported before any handler runs.
        let replies = run_raw(
            true,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"skills_list","_meta":{"n":1e9999}}}"#,
            framed,
            false,
        );
        assert_eq!(replies[1]["error"]["code"], -32602);
        assert_eq!(
            replies[1]["error"]["message"],
            "Invalid params: json: cannot unmarshal number 1e9999 into Go struct field ._meta of type float64"
        );
    }
}

fn tree(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Option<Vec<u8>>> {
    let mut result = std::collections::BTreeMap::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let contents = if entry.file_type().unwrap().is_dir() {
                pending.push(path.clone());
                None
            } else {
                Some(std::fs::read(&path).unwrap())
            };
            result.insert(path.strip_prefix(root).unwrap().to_path_buf(), contents);
        }
    }
    result
}

#[test]
fn malformed_skills_never_admit_handlers_and_keep_null_id_parse_errors() {
    for enabled in [false, true] {
        for framed in [false, true] {
            for (call, message) in [
                (
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"skills_install","arguments":{"name":"demo","target":"opencode","dry_run":false},"_meta":-"#,
                    "Parse error: invalid character ' ' in numeric literal",
                ),
                (
                    r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"skills_install","arguments":{"name":"demo","target":"opencode","dry_run":false},"x":01}}"#,
                    "Parse error: invalid character '1' after object key:value pair",
                ),
            ] {
                let replies = run_raw(enabled, call, framed, true);
                assert_eq!(replies.len(), 2);
                assert!(replies[0]["result"]["tools"].is_array());
                assert_eq!(
                    replies[1],
                    serde_json::json!({
                        "jsonrpc": "2.0", "id": null,
                        "error": {"code": -32700, "message": message}
                    })
                );
            }
        }
    }
}
