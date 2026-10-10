//! Profile and global audit settings produce effective startup effects.
use std::fs;
use std::io::Write;
use std::process::Output;

use serde_json::Value;
use tempfile::TempDir;

#[test]
fn disabled_memory_and_history_do_not_create_startup_state() {
    let cases = [
        ("profile-opt-out", false, true, None, false),
        ("explicit-audit-opt-in", true, true, None, true),
        ("global-audit-disabled", true, false, None, false),
        ("environment-audit-opt-in", true, false, Some("true"), true),
        (
            "environment-audit-opt-out",
            true,
            true,
            Some("false"),
            false,
        ),
        (
            "profile-vetoes-environment",
            false,
            true,
            Some("true"),
            false,
        ),
    ];
    for (case, profile_audit, global_audit, audit_override, expect_audit) in cases {
        let root = TempDir::new().unwrap();
        let config = root.path().join("config/symbrain");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join("config.toml"),
            format!("[audit]\nenabled = {global_audit}\n"),
        )
        .unwrap();
        let profile = root.path().join("isolated.toml");
        fs::write(
            &profile,
            format!(
                "[profile]\nname = \"isolated\"\n\n[audit]\nenabled = {profile_audit}\n\n[servers.memory]\nenabled = false\n\n[servers.skills]\nenabled = true\n"
            ),
        )
        .unwrap();
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"skills_list\",\"arguments\":{}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"patterns\",\"arguments\":{}}}\n",
        );
        let mut process =
            super::command(&root, &["mcp", "--profile-file", profile.to_str().unwrap()]);
        if let Some(value) = audit_override {
            process.env("SYMBRAIN_AUDIT_ENABLED", value);
        }
        let output = run_process(&mut process, input.as_bytes());
        assert!(output.status.success(), "{case}: {:?}", output.stderr);
        assert!(output.stderr.is_empty(), "{case}: {:?}", output.stderr);
        let responses: Vec<Value> = output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 4, "{case}");
        assert_eq!(responses[0]["result"]["serverInfo"]["name"], "symbrain");
        let tools = responses[1]["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "skills_list"));
        assert!(!tools.iter().any(|tool| {
            let name = tool["name"].as_str().unwrap();
            name.starts_with("memory_") || name.starts_with("activity_")
        }));
        assert_eq!(responses[2]["result"]["isError"], false);
        assert_eq!(responses[3]["result"]["isError"], false);

        let data = root.path().join("home/.local/share");
        if expect_audit {
            let audit = data.join("symbrain/audit/isolated.jsonl");
            assert!(audit.is_file(), "{case}: expected explicit audit state");
            assert_eq!(
                fs::read_dir(data.join("symbrain")).unwrap().count(),
                1,
                "{case}"
            );
            assert_eq!(
                fs::read_dir(data.join("symbrain/audit")).unwrap().count(),
                1,
                "{case}"
            );
        } else {
            assert!(
                !data.exists(),
                "{case}: disabled features created data state"
            );
        }
        assert_eq!(fs::read_dir(&config).unwrap().count(), 1, "{case}");
        assert!(!root.path().join("cache").exists(), "{case}");
        assert!(!root.path().join("state").exists(), "{case}");
    }
}

fn run_process(command: &mut std::process::Command, input: &[u8]) -> Output {
    let mut child = command.spawn().unwrap();
    child.stdin.as_mut().unwrap().write_all(input).unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

#[test]
fn enabled_memory_is_initialized_and_exposed() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("memory.toml");
    fs::write(
        &profile,
        "[profile]\nname = \"memory-on\"\n\n[audit]\nenabled = false\n\n[servers.memory]\nenabled = true\nmode = \"read_only\"\n",
    )
    .unwrap();
    let input = concat!(
        "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
        "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
    );
    let output = super::run_with_input(
        &root,
        &["mcp", "--profile-file", profile.to_str().unwrap()],
        input.as_bytes(),
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let responses: Vec<Value> = output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    assert_eq!(responses.len(), 2);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    assert!(tools.iter().any(|tool| {
        tool["name"]
            .as_str()
            .is_some_and(|name| name.starts_with("memory_"))
    }));
    let database = root
        .path()
        .join("home/.local/share/symbrain/memory/default.db");
    assert!(
        database.is_file(),
        "enabled memory did not create its database"
    );
    assert!(
        !root
            .path()
            .join("home/.config/symbrain/memory/jwt.secret")
            .exists(),
        "native memory startup must not create legacy JWT key state"
    );
}
