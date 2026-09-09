//! Negative validation, error handling, and usage tests for profiles.

use symbrain_policy::constants::{FOREIGN_ACCESS_WRITE, SERVER_SKILLS, SERVER_USAGE, SERVER_VAULT};
use symbrain_policy::profile::parse::parse;
use symbrain_policy::validate_name;

#[test]
fn parse_foreign_server_access_validation() {
    let bad = r#"
[profile]
name = "bad-access"

[servers.fig]
enabled = true
command = "/usr/bin/fig-mcp"
access = "exec"
"#;
    assert!(parse("bad-access", bad).is_err());

    let read = r#"
[profile]
name = "read-access"

[servers.fig]
enabled = true
command = "/usr/bin/fig-mcp"
access = "read"
"#;
    let p = parse("read-access", read).expect("failed to parse");
    assert_eq!(p.server("fig").access, "read");

    let def = r#"
[profile]
name = "default-access"

[servers.fig]
enabled = true
command = "/usr/bin/fig-mcp"
"#;
    let p2 = parse("default-access", def).expect("failed to parse");
    assert_eq!(p2.server("fig").access, FOREIGN_ACCESS_WRITE);
}

#[test]
fn parse_name_mismatch_errors() {
    let toml = r#"
[profile]
name = "different-name"
"#;

    let err = parse("on-disk-name", toml).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("name mismatch"));
}

#[test]
fn parse_invalid_vault_mode_errors() {
    let toml = r#"
[profile]
name = "bad-vault-mode"

[servers.vault]
enabled = true
mode    = "godmode"
"#;

    let err = parse("bad-vault-mode", toml).unwrap_err();
    assert!(err.to_string().contains("servers.vault: invalid mode"));
}

#[test]
fn parse_invalid_memory_mode_errors() {
    let toml = r#"
[profile]
name = "bad-memory-mode"

[servers.memory]
enabled = true
mode    = "write_only"
"#;

    let err = parse("bad-memory-mode", toml).unwrap_err();
    assert!(err.to_string().contains("servers.memory: invalid mode"));
}

#[test]
fn parse_skills_mode_is_ignored_with_warning() {
    let toml = r#"
[profile]
name = "skills-mode"

[servers.skills]
enabled = true
mode    = "full"
"#;

    let p = parse("skills-mode", toml).expect("failed to parse");
    assert!(p.server(SERVER_SKILLS).enabled);
    assert!(
        p.warnings
            .iter()
            .any(|w| w.contains("servers.skills: mode") && w.contains("ignored"))
    );
}

#[test]
fn parse_core_alias_access_and_tools_ignored_with_warning() {
    let toml = r#"
[profile]
name = "vault-access"

[servers.vault]
enabled = true
access      = "read"
tools_read  = ["get_entry"]
tools_write = ["set_entry_field"]
"#;

    let p = parse("vault-access", toml).expect("failed to parse");
    assert!(p.server(SERVER_VAULT).enabled);
    assert!(
        p.warnings
            .iter()
            .any(|w| w.contains("access/tools_read/tools_write are ignored"))
    );
}

#[test]
fn parse_malformed_toml_errors() {
    let toml = r#"[profile
name = "malformed""#;

    let err = parse("malformed", toml).unwrap_err();
    assert!(err.to_string().contains("failed to parse TOML"));
}

#[test]
fn parse_usage_server_options() {
    let toml_on = r#"
[profile]
name = "usage-on"

[servers.usage]
enabled = true
"#;
    let p = parse("usage-on", toml_on).unwrap();
    assert!(p.server(SERVER_USAGE).enabled);

    let toml_off = r#"
[profile]
name = "usage-off"
"#;
    let p2 = parse("usage-off", toml_off).unwrap();
    assert!(!p2.server(SERVER_USAGE).enabled);

    let toml_mode = r#"
[profile]
name = "usage-mode"

[servers.usage]
enabled = true
mode = "full"
"#;
    let p3 = parse("usage-mode", toml_mode).unwrap();
    assert!(p3.server(SERVER_USAGE).mode.is_empty());
    assert!(
        p3.warnings
            .iter()
            .any(|w| w.contains("servers.usage: mode") && w.contains("ignored"))
    );
}

#[test]
fn parse_usage_tools_allow_deny() {
    let toml = r#"
[profile]
name = "usage-tools"

[servers.usage]
enabled = true
tools_deny = ["get_ai_usage"]
"#;
    let p = parse("usage-tools", toml).unwrap();
    assert_eq!(p.server(SERVER_USAGE).tools_deny, vec!["get_ai_usage"]);
}

#[test]
fn validate_name_cases() {
    assert!(validate_name("cursor-arbeit").is_ok());
    assert!(validate_name("personal").is_ok());
    assert!(validate_name("restricted_2").is_ok());
    assert!(validate_name("").is_err());
    assert!(validate_name("..").is_err());
    assert!(validate_name("../../etc/passwd").is_err());
    assert!(validate_name("has/slash").is_err());
    assert!(validate_name("has\\backslash").is_err());
    assert!(validate_name("has space").is_err());
    assert!(validate_name("has\"quote").is_err());
}

#[test]
fn audit_verbose_is_omitted_when_default_and_serialized_when_enabled() {
    let default_profile = parse("audit-default", "[profile]\nname = \"audit-default\"\n").unwrap();
    let default_json = serde_json::to_value(&default_profile).unwrap();
    assert_eq!(default_json["audit"]["enabled"], true);
    assert!(default_json["audit"].get("verbose").is_none());

    let verbose_profile = parse(
        "audit-verbose",
        "[profile]\nname = \"audit-verbose\"\n\n[audit]\nverbose = true\n",
    )
    .unwrap();
    let verbose_json = serde_json::to_value(&verbose_profile).unwrap();
    assert_eq!(verbose_json["audit"]["verbose"], true);
}
