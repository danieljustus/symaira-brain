//! Profile opt-out is authoritative even when global audit defaults opt in.
use std::fs;

use serde_json::Value;
use tempfile::TempDir;

#[test]
fn disabled_memory_and_history_do_not_create_startup_state() {
    for audit_enabled in [false, true] {
        let root = TempDir::new().unwrap();
        let config = root.path().join("config/symbrain");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.toml"), "[audit]\nenabled = true\n").unwrap();
        let profile = root.path().join("isolated.toml");
        fs::write(
            &profile,
            format!(
                "[profile]\nname = \"isolated\"\n\n[audit]\nenabled = {audit_enabled}\n\n[servers.memory]\nenabled = false\n\n[servers.skills]\nenabled = true\n"
            ),
        )
        .unwrap();
        let input = concat!(
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"skills_list\",\"arguments\":{}}}\n",
            "{\"jsonrpc\":\"2.0\",\"id\":4,\"method\":\"tools/call\",\"params\":{\"name\":\"patterns\",\"arguments\":{}}}\n",
        );
        let output = super::run_with_input(
            &root,
            &["mcp", "--profile-file", profile.to_str().unwrap()],
            input.as_bytes(),
        );
        assert!(
            output.status.success(),
            "audit={audit_enabled}: {:?}",
            output.stderr
        );
        assert!(output.stderr.is_empty());
        let responses: Vec<Value> = output
            .stdout
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| serde_json::from_slice(line).unwrap())
            .collect();
        assert_eq!(responses.len(), 4);
        assert_eq!(responses[0]["result"]["serverInfo"]["name"], "symbrain");
        let tools = responses[1]["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|tool| tool["name"] == "skills_list"));
        assert!(!tools.iter().any(|tool| {
            let name = tool["name"].as_str().unwrap();
            name.starts_with("memory_") || name.starts_with("activity_")
        }));
        assert_eq!(responses[2]["result"]["isError"], false);
        assert_eq!(responses[3]["result"]["isError"], false);
        // Assert the complete state inventory, not only known DB filenames.
        // An explicitly enabled audit is the sole allowed persistent effect.
        let data = root.path().join("home/.local/share");
        if audit_enabled {
            let audit = data.join("symbrain/audit/isolated.jsonl");
            assert!(audit.is_file());
            assert_eq!(fs::read_dir(data.join("symbrain")).unwrap().count(), 1);
            assert_eq!(
                fs::read_dir(data.join("symbrain/audit")).unwrap().count(),
                1
            );
        } else {
            assert!(!data.exists(), "disabled features created state");
        }
        assert_eq!(fs::read_dir(&config).unwrap().count(), 1);
        assert!(!root.path().join("cache").exists());
        assert!(!root.path().join("state").exists());
    }
}
