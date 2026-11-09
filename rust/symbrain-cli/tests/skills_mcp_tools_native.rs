//! Every skills MCP tool is served natively by `symbrain mcp` (#764): the tool
//! set and order are stable, each tool answers a happy-path call against a real
//! library, and stdout carries nothing but JSON-RPC.

#[path = "support/skills_sandbox.rs"]
mod skills_sandbox;

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::{Value, json};
use skills_sandbox::{Sandbox, git_available};

const TOOLS: [&str; 11] = [
    "skills_list",
    "skills_inspect",
    "skills_validate",
    "skills_profile_list",
    "skills_profile_resolve",
    "skills_render_plan",
    "skills_install",
    "skills_discover_sources",
    "skills_history",
    "skills_restore",
    "skills_targets_status",
];

/// Replies keyed by request id: the gateway may answer concurrent calls in any order.
fn serve(sandbox: &Sandbox, calls: &[(&str, Value)]) -> (BTreeMap<u64, Value>, String) {
    std::fs::write(
        sandbox.path("profile.toml"),
        "[profile]\nname = \"native-skills\"\n[servers.vault]\nenabled = false\n[servers.memory]\nenabled = false\n[servers.usage]\nenabled = false\n[servers.skills]\nenabled = true\n[audit]\nenabled = false\n",
    )
    .unwrap();
    let mut input = vec![
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
    ];
    for (index, (name, arguments)) in calls.iter().enumerate() {
        input.push(json!({"jsonrpc":"2.0","id":10 + index,"method":"tools/call","params":{"name":name,"arguments":arguments}}));
    }
    let text = input
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let mut child = Command::new(env!("CARGO_BIN_EXE_symbrain"))
        .env_clear()
        .envs(std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)))
        .envs(skills_sandbox::coverage::profile_environment())
        .env("HOME", sandbox.path("home"))
        .env("XDG_CONFIG_HOME", sandbox.path("config"))
        .env("XDG_DATA_HOME", sandbox.path("data"))
        .env("XDG_CACHE_HOME", sandbox.path("cache"))
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("PATH", "/usr/bin:/bin:/opt/homebrew/bin")
        .current_dir(sandbox.path("project"))
        .args(["mcp", "--profile-file"])
        .arg(sandbox.path("profile.toml"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(text.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let replies = stdout
        .lines()
        .map(|line| {
            let reply = serde_json::from_str::<Value>(line)
                .unwrap_or_else(|e| panic!("stdout pollution {line:?}: {e}"));
            (reply["id"].as_u64().unwrap(), reply)
        })
        .collect();
    (replies, String::from_utf8(output.stderr).unwrap())
}

fn payload(reply: &Value) -> Value {
    assert_ne!(reply["result"]["isError"], true, "{reply}");
    serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
}

#[test]
fn all_skills_tools_are_listed_and_answer_natively() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    std::fs::create_dir_all(sandbox.path("config/symskills/profiles")).unwrap();
    std::fs::write(
        sandbox.path("config/symskills/profiles/dev.toml"),
        "[links.first]\nskill = \"alpha\"\n",
    )
    .unwrap();
    let calls: Vec<(&str, Value)> = vec![
        ("skills_list", json!({})),
        ("skills_inspect", json!({"name":"alpha"})),
        ("skills_validate", json!({"name":"alpha"})),
        ("skills_profile_list", json!({})),
        ("skills_profile_resolve", json!({"name":"dev"})),
        (
            "skills_render_plan",
            json!({"name":"alpha","target":"claude"}),
        ),
        (
            "skills_install",
            json!({"name":"alpha","target":"claude","mode":"copy","dry_run":true}),
        ),
        ("skills_discover_sources", json!({})),
        ("skills_history", json!({"name":"alpha"})),
        (
            "skills_restore",
            json!({"name":"alpha","rev":"HEAD","dry_run":true}),
        ),
        ("skills_targets_status", json!({})),
    ];
    let (replies, stderr) = serve(&sandbox, &calls);
    assert!(stderr.is_empty(), "stderr: {stderr}");
    let listed: Vec<&str> = replies[&2]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|tool| tool["name"].as_str())
        .filter(|name| name.starts_with("skills_"))
        .collect();
    for name in TOOLS {
        assert!(listed.contains(&name), "{name} missing from {listed:?}");
    }
    assert_eq!(listed.len(), TOOLS.len(), "{listed:?}");

    let skills = payload(&replies[&10]);
    assert_eq!(skills["skills"][0]["name"], "alpha");
    assert_eq!(payload(&replies[&11])["name"], "alpha");
    assert_eq!(payload(&replies[&12])["valid"], true);
    assert_eq!(payload(&replies[&13])["profiles"][0]["name"], "dev");
    assert_eq!(payload(&replies[&14])["skills"][0]["skill"], "alpha");
    assert_eq!(payload(&replies[&15])["action"], "planned");
    assert_eq!(payload(&replies[&16])["action"], "planned");
    assert!(payload(&replies[&17]).get("candidates").is_some());
    if git_available() {
        assert_eq!(payload(&replies[&18])["history"][0]["operation"], "import");
        assert_eq!(payload(&replies[&19])["action"], "planned");
    }
    assert_eq!(
        payload(&replies[&20])["targets"].as_array().unwrap().len(),
        6
    );

    let (again, _) = serve(&sandbox, &calls[..1]);
    assert_eq!(again[&2], replies[&2], "tool catalog must be deterministic");
}
