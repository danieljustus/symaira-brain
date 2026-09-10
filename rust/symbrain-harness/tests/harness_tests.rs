use chrono::{TimeZone, Utc};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use symbrain_harness::{
    Entry, HarnessName, SERVER_NAME, SUPERSEDED_CORE_NAMES, all, backup_at, list_for_env, lookup,
    names, parse, unified_diff,
};
fn fixture(name: &str, file: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/golden")
        .join(name)
        .join(file);
    std::fs::read(path).expect("Go oracle fixture exists")
}
#[test]
fn registry_is_exactly_the_nine_entry_go_registry() {
    let names_in_order: Vec<_> = all().iter().map(|h| h.name.to_string()).collect();
    assert_eq!(
        names_in_order,
        [
            "claude",
            "claude-desktop",
            "cursor",
            "opencode",
            "codex",
            "antigravity",
            "agents",
            "hermes",
            "openclaw",
        ]
    );
    assert_eq!(
        names(),
        [
            "agents",
            "antigravity",
            "claude",
            "claude-desktop",
            "codex",
            "cursor",
            "hermes",
            "openclaw",
            "opencode"
        ]
    );
    assert_eq!(SUPERSEDED_CORE_NAMES, ["symmemory", "symskills"]);
    assert_eq!(SERVER_NAME, "symbrain");
    assert_eq!(lookup("openclaw").unwrap().name, HarnessName::OpenClaw);
    assert!(
        lookup("toolbox").is_err(),
        "Phase 6.2 prose must not expand the Go registry"
    );
}
#[test]
fn injected_paths_use_target_os_not_host_goos() {
    let windows = lookup("claude-desktop").unwrap();
    let appdata = vec![("APPDATA".into(), r"C:\Users\ada\AppData\Roaming".into())];
    assert_eq!(
        windows
            .config_path_for("windows", &appdata)
            .unwrap()
            .to_string_lossy(),
        r"C:\Users\ada\AppData\Roaming\Claude\claude_desktop_config.json"
    );
    let fallback = vec![("USERPROFILE".into(), r"C:\Users\ada".into())];
    assert_eq!(
        windows
            .config_path_for("windows", &fallback)
            .unwrap()
            .to_string_lossy(),
        r"C:\Users\ada\AppData\Roaming\Claude\claude_desktop_config.json"
    );
    let darwin = vec![("HOME".into(), "/Users/ada".into())];
    assert_eq!(
        windows
            .config_path_for("darwin", &darwin)
            .unwrap()
            .to_string_lossy(),
        "/Users/ada/Library/Application Support/Claude/claude_desktop_config.json"
    );
}
#[test]
fn entry_semantics_match_go_and_are_cross_platform() {
    let entry = Entry::new("personal");
    assert!(entry.is_symbrain());
    assert_eq!(entry.profile(), Some("personal"));
    assert!(
        !Entry {
            command: r"C:\bin\symbrain".into(),
            args: vec![],
        }
        .is_symbrain()
    );
    assert!(
        Entry {
            command: "/bin/symbrain".into(),
            args: vec![],
        }
        .is_symbrain()
    );
    assert_eq!(
        Entry {
            command: r"C:\bin\symmemory".into(),
            args: vec![],
        }
        .superseded_core(),
        None,
        "backslashes are ordinary characters on Unix"
    );
    assert_eq!(
        Entry {
            command: "/bin/symmemory".into(),
            args: vec![],
        }
        .superseded_core(),
        Some("symmemory")
    );
    assert_eq!(
        Entry {
            command: "symvault".into(),
            args: vec![]
        }
        .superseded_core(),
        None
    );
    assert_eq!(
        Entry {
            command: "symbrain".into(),
            args: vec!["--profile=restricted".into()]
        }
        .profile(),
        Some("restricted")
    );
    assert_eq!(
        Entry {
            command: "symbrain".into(),
            args: vec!["--profile".into()]
        }
        .profile(),
        None
    );
}
#[test]
fn go_json_goldens_are_byte_exact_and_reversible() {
    let profiles = [
        ("claude", "personal"),
        ("claude-desktop", "restricted"),
        ("cursor", "personal"),
        ("opencode", "default"),
        ("antigravity", "default"),
    ];
    for (name, profile) in profiles {
        let harness = lookup(name).unwrap();
        let before = fixture(name, "before.json");
        let after = fixture(name, "after.json");
        assert_eq!(
            parse(harness, &before).unwrap().marshal().unwrap(),
            before,
            "{name} before must be canonical"
        );
        let mut document = parse(harness, &before).unwrap();
        document.set_server(SERVER_NAME, Entry::new(profile));
        assert_eq!(
            document.marshal().unwrap(),
            after,
            "{name} insertion diverged from Go oracle"
        );
        let mut document = parse(harness, &after).unwrap();
        assert!(document.remove_server(SERVER_NAME));
        assert_eq!(
            document.marshal().unwrap(),
            before,
            "{name} removal did not restore Go fixture"
        );
    }
}
#[test]
fn go_toml_goldens_are_byte_exact_and_reversible() {
    let harness = lookup("codex").unwrap();
    let before = fixture("codex", "before.toml");
    let after = fixture("codex", "after.toml");
    assert_eq!(parse(harness, &before).unwrap().marshal().unwrap(), before);
    let mut document = parse(harness, &before).unwrap();
    document.set_server(SERVER_NAME, Entry::new("personal"));
    assert_eq!(document.marshal().unwrap(), after);
    assert!(document.remove_server(SERVER_NAME));
    assert_eq!(document.marshal().unwrap(), before);
}
#[test]
fn json_preserves_duplicate_key_order_and_numeric_lexemes() {
    let harness = lookup("claude").unwrap();
    let input = br#"{"z":1,"mcpServers":{"other":{"command":"x"}},"z":-0.0100E+02,"number":9007199254740993.000,"nested":{"b":true,"a":null}}"#;
    let document = parse(harness, input).unwrap();
    let output = String::from_utf8(document.marshal().unwrap()).unwrap();
    assert!(output.contains("\"z\": -0.0100E+02"));
    assert!(output.contains("\"number\": 9007199254740993.000"));
    assert!(output.find("\"z\"").unwrap() < output.find("\"mcpServers\"").unwrap());
    assert!(output.find("\"b\"").unwrap() < output.find("\"a\"").unwrap());
}
#[test]
fn parser_rejects_malformed_and_wrong_top_level_documents() {
    let json = lookup("cursor").unwrap();
    for input in [
        br"{" as &[u8],
        br"[]",
        br"{}garbage",
        br#"{"mcpServers": {"x": 1,}}"#,
    ] {
        assert!(parse(json, input).is_err(), "accepted invalid JSON");
    }
    let toml = lookup("codex").unwrap();
    for input in [
        b"[mcp_servers".as_slice(),
        b"a = 1\na = 2\n",
        b"not = = valid",
    ] {
        assert!(parse(toml, input).is_err(), "accepted invalid TOML");
    }
    assert!(parse(lookup("agents").unwrap(), b"{}").is_err());
}
#[test]
fn inventory_models_are_read_only_and_redact_environment_values() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let env = vec![("HOME".into(), root.to_string_lossy().into_owned())];
    let path = root.join(".claude.json");
    std::fs::write(&path, br#"{"mcpServers":{"vault":{"command":"x","env":{"TOKEN":"secret-value","API_KEY":"other"}}}}"#).unwrap();
    let report = list_for_env(None, "linux", &env);
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("secret-value"));
    assert!(json.contains("TOKEN"));
}
#[test]
fn backup_clock_and_mode_are_deterministic_on_unix() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("config.json");
    std::fs::write(&path, b"{}\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    }
    let timestamp = Utc.with_ymd_and_hms(2026, 9, 6, 20, 0, 0).unwrap();
    let backup = backup_at(&path, timestamp).unwrap().unwrap();
    assert_eq!(
        backup.file_name().unwrap(),
        "config.json.bak.20260906T200000Z"
    );
    assert_eq!(std::fs::read(&backup).unwrap(), b"{}\n");
    #[cfg(unix)]
    assert_eq!(
        std::fs::metadata(backup).unwrap().permissions().mode() & 0o777,
        0o640
    );
}
#[test]
fn unified_diff_matches_go_small_and_oversized_contracts() {
    assert_eq!(
        unified_diff("f", b"a\nb\nc\n", b"a\nX\nc\n"),
        "--- f\n+++ f\n@@ -1,3 +1,3 @@\n a\n-b\n+X\n c\n"
    );
    assert_eq!(unified_diff("f", b"a\nb\n", b"a\nb\n"), "");
    let old = b"x\n".repeat(2_500);
    let new = b"y\n".repeat(2_500);
    assert_eq!(
        unified_diff("f", &old, &new),
        "--- f\n+++ f\nfile too large to diff safely: 2500 old lines, 2500 new lines (max 10000 lines and 4000000 comparison cells); full diff skipped\n"
    );
}
#[test]
fn unsupported_adapters_are_not_mcp_documents() {
    for name in ["agents", "hermes", "openclaw"] {
        let harness = lookup(name).unwrap();
        assert!(harness.server_key().is_none());
        assert!(parse(harness, b"{}").is_err());
    }
}
#[test]
fn backups_with_same_timestamp_keep_both_snapshots() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let path = root.join("config.json");
    std::fs::write(&path, b"first\n").unwrap();
    let timestamp = Utc.with_ymd_and_hms(2026, 9, 6, 20, 0, 2).unwrap();
    let first = backup_at(&path, timestamp).unwrap().unwrap();
    std::fs::write(&path, b"second\n").unwrap();
    let second = backup_at(&path, timestamp).unwrap().unwrap();
    assert_ne!(first, second);
    assert_eq!(std::fs::read(first).unwrap(), b"first\n");
    assert_eq!(std::fs::read(second).unwrap(), b"second\n");
}

#[cfg(unix)]
#[test]
fn backup_rejects_symlink_targets() {
    let temp = tempfile::tempdir().unwrap();
    let target = temp.path().join("target.json");
    let link = temp.path().join("config.json");
    std::fs::write(&target, b"{\"ok\":true}\n").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let timestamp = Utc.with_ymd_and_hms(2026, 9, 6, 20, 0, 1).unwrap();
    assert!(backup_at(&link, timestamp).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"{\"ok\":true}\n");
}
#[cfg(unix)]
#[test]
fn backup_rejects_symlink_parents() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let outside = root.join("outside");
    let linked = root.join("linked");
    std::fs::create_dir(&outside).unwrap();
    let target = outside.join("config.json");
    std::fs::write(&target, b"outside\n").unwrap();
    std::os::unix::fs::symlink(&outside, &linked).unwrap();
    let path = linked.join("config.json");
    let timestamp = Utc.with_ymd_and_hms(2026, 9, 6, 20, 0, 1).unwrap();
    assert!(backup_at(&path, timestamp).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"outside\n");
}
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn backup_preserves_non_utf8_path_bytes() {
    use std::ffi::OsString;
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let temp = tempfile::tempdir().unwrap();
    let path = temp
        .path()
        .join(OsString::from_vec(b"config-\xff.json".to_vec()));
    std::fs::write(&path, b"{}\n").unwrap();
    let timestamp = Utc.with_ymd_and_hms(2026, 9, 6, 20, 0, 1).unwrap();
    let backup = backup_at(&path, timestamp).unwrap().unwrap();
    assert_eq!(
        backup.file_name().unwrap().as_bytes(),
        b"config-\xff.json.bak.20260906T200001Z"
    );
}
#[test]
fn parsers_reject_excessive_input_shapes() {
    let json_harness = lookup("claude").unwrap();
    let many_values = format!(
        "{{\"mcpServers\":{{\"x\":{{\"args\":[{}]}}}}}}",
        (0..100_001).map(|_| "0").collect::<Vec<_>>().join(",")
    );
    let Err(error) = parse(json_harness, many_values.as_bytes()) else {
        panic!("excessive JSON value count was accepted");
    };
    assert!(error.to_string().contains("maximum JSON value count"));
    let long_string = format!(
        "{{\"mcpServers\":{{\"x\":{{\"command\":\"{}\"}}}}}}",
        "x".repeat(1 << 20)
    );
    let Err(error) = parse(json_harness, long_string.as_bytes()) else {
        panic!("excessive JSON string size was accepted");
    };
    assert!(error.to_string().contains("maximum JSON string size"));
}
#[test]
fn parsers_reject_excessive_depth_and_redact_malformed_toml_values() {
    let json_harness = lookup("claude").unwrap();
    let nested = format!("{{\"mcpServers\":{{\"x\":{}}}}}", "[".repeat(20_000));
    let Err(json_error) = parse(json_harness, nested.as_bytes()) else {
        panic!("deeply nested JSON was accepted");
    };
    let json_error = json_error.to_string();
    assert!(json_error.contains("maximum nesting depth"));
    let toml_harness = lookup("codex").unwrap();
    let secret = "should-never-appear-in-diagnostics";
    let malformed = format!("token = \"{secret}\n");
    let Err(toml_error) = parse(toml_harness, malformed.as_bytes()) else {
        panic!("malformed TOML was accepted");
    };
    let toml_error = toml_error.to_string();
    assert!(!toml_error.contains(secret));
    assert!(toml_error.contains("invalid basic string"));
}
#[test]
fn failed_remove_is_rollback_safe() {
    let harness = lookup("claude").unwrap();
    let mut document = parse(harness, &fixture("claude", "before.json")).unwrap();
    let snapshot = document.marshal().unwrap();
    assert!(!document.remove_server("missing"));
    assert_eq!(document.marshal().unwrap(), snapshot);
}
