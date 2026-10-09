//! Full transport → gateway → ordered Skills decoder regressions, prepared.
use serde_json::Value;
use std::io::Write;
use std::process::{Command, Stdio};

fn run(enabled: bool, tool: &str, arguments: &str, framed: bool) -> Vec<Value> {
    let root = tempfile::tempdir().unwrap();
    for directory in ["home", "config", "data", "cache", "project", "tmp"] {
        std::fs::create_dir(root.path().join(directory)).unwrap();
    }
    let profile = root.path().join("profile.toml");
    std::fs::write(&profile, format!("[profile]\nname = \"raw-skills\"\n[servers.vault]\nenabled = false\n[servers.memory]\nenabled = false\n[servers.usage]\nenabled = false\n[servers.skills]\nenabled = {enabled}\n[audit]\nenabled = false\n")).unwrap();
    let list = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let call = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{{"name":"{tool}","arguments":{arguments}}}}}"#
    );
    let mut input = Vec::new();
    for body in [list, &call] {
        if framed {
            write!(&mut input, "Content-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        } else {
            writeln!(&mut input, "{body}").unwrap();
        }
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
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
                assert_eq!(names, symbrain_mcp::SKILLS_TOOL_NAMES);
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
        for arguments in [
            r#"{"name":"\ud800"}"#,
            r#"{"\ud800":1e9999,"name":"not-versioned"}"#,
        ] {
            let replies = run(true, "skills_history", arguments, framed);
            assert_eq!(replies.len(), 2);
            assert_eq!(replies[1]["result"]["isError"], true);
            let text = replies[1]["result"]["content"][0]["text"].as_str().unwrap();
            assert!(text.starts_with("history: skill "), "{text}");
            assert!(text.contains("not versioned: no git repository"), "{text}");
        }
        let replies = run(true, "skills_history", r#"{"limit":1e9999}"#, framed);
        assert!(
            replies[1]["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("parse arguments:")
        );
    }
}
