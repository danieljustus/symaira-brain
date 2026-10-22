//! Replays the recorded real-Go replies (`skills_raw_go_oracle.json`, produced by
//! `scripts/skills-native-oracle/raw_layer_oracle.py record` from the pinned
//! Go sources) through the native `symbrain mcp` in both transports and
//! requires identical JSON-RPC outcomes: exact parse/params error codes and
//! messages, and the same success/tool-error class for valid calls.
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io::Write;
use std::process::{Command, Stdio};

const FIXTURE: &str = include_str!("fixtures/skills_raw_go_oracle.json");

fn body(rle: &Value) -> Vec<u8> {
    let mut bytes = Vec::new();
    for pair in rle.as_array().unwrap() {
        let chunk = hex_bytes(pair[0].as_str().unwrap());
        for _ in 0..pair[1].as_u64().unwrap() {
            bytes.extend_from_slice(&chunk);
        }
    }
    bytes
}

fn hex_bytes(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn stdin(boot: &[Vec<u8>], body: &[u8], framed: bool) -> Vec<u8> {
    let mut input = Vec::new();
    for line in boot.iter().map(Vec::as_slice).chain([body]) {
        if framed {
            write!(&mut input, "Content-Length: {}\r\n\r\n", line.len()).unwrap();
            input.extend_from_slice(line);
        } else {
            input.extend_from_slice(line);
            input.push(b'\n');
        }
    }
    input
}

fn replies(output: &[u8], framed: bool) -> Vec<Value> {
    if !framed {
        return output
            .split(|&byte| byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
    }
    let mut rest = output;
    let mut result = Vec::new();
    while !rest.is_empty() {
        let split = rest.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let size: usize = std::str::from_utf8(&rest[..split])
            .unwrap()
            .strip_prefix("Content-Length: ")
            .unwrap()
            .parse()
            .unwrap();
        result.push(serde_json::from_slice(&rest[split + 4..split + 4 + size]).unwrap());
        rest = &rest[split + 4 + size..];
    }
    result
}

fn run(input: &[u8], framed: bool) -> Vec<Value> {
    let root = tempfile::tempdir().unwrap();
    for directory in ["home", "config", "data", "cache", "project", "tmp"] {
        std::fs::create_dir(root.path().join(directory)).unwrap();
    }
    let profile = root.path().join("profile.toml");
    std::fs::write(
        &profile,
        "[profile]\nname = \"raw-skills\"\n[servers.vault]\nenabled = false\n[servers.memory]\n\
         enabled = false\n[servers.usage]\nenabled = false\n[servers.skills]\nenabled = true\n\
         [audit]\nenabled = false\n",
    )
    .unwrap();
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
    child.stdin.take().unwrap().write_all(input).unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    replies(&output.stdout, framed)
}

fn view(replies: &[Value]) -> Value {
    assert_eq!(replies.len(), 3, "{replies:?}");
    let reply = &replies[2];
    match reply.get("error") {
        Some(error) => json!({"id": reply["id"], "error": error}),
        None => json!({"id": reply["id"], "isError": reply["result"]["isError"]}),
    }
}

#[test]
fn native_replies_match_every_recorded_go_reply() {
    let oracle: Value = serde_json::from_str(FIXTURE).unwrap();
    assert!(
        oracle["go_version"]
            .as_str()
            .unwrap()
            .starts_with("go version go1.26")
    );
    let boot: Vec<Vec<u8>> = oracle["bootstrap"]
        .as_array()
        .unwrap()
        .iter()
        .map(|line| line.as_str().unwrap().as_bytes().to_vec())
        .collect();
    let cases = oracle["cases"].as_array().unwrap();
    let (mut mismatches, mut classes, mut residual) = (Vec::new(), BTreeSet::new(), 0);
    for case in cases {
        let framed = case["mode"] == "framed";
        let body = body(&case["body_rle"]);
        let native = view(&run(&stdin(&boot, &body, framed), framed));
        if case.get("known_divergence").is_some() {
            // Inherited generic owner (not Skills): parity is out of this
            // layer's scope. Fail when it starts to match so the marker is
            // removed rather than left to hide a fixed gap.
            assert_ne!(native, case["go"], "{} now matches Go", case["id"]);
            residual += 1;
        } else if native != case["go"] {
            mismatches.push(format!("{}: go={} native={native}", case["id"], case["go"]));
        }
        classes.insert(match case["go"].get("error") {
            Some(error) => format!("error {}", error["code"]),
            None => format!("result isError={}", case["go"]["isError"]),
        });
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ:\n{}",
        mismatches.len(),
        cases.len(),
        mismatches.join("\n")
    );
    assert_eq!(residual, 4, "documented non-Skills-owner residual cases");
    // Anti-vacuity: the corpus must exercise parse errors, params errors,
    // invalid requests, unknown tools, and both tool outcomes.
    assert!(cases.len() >= 300, "{}", cases.len());
    for class in [
        "error -32700",
        "error -32602",
        "error -32600",
        "error -32601",
        "result isError=false",
        "result isError=true",
    ] {
        assert!(classes.contains(class), "no case for {class}: {classes:?}");
    }
}
