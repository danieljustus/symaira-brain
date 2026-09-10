//! Tests for profile TOML parsing, defaulting, and schema validation.

use symbrain_policy::constants::{
    MEMORY_MODE_READ_ONLY, MEMORY_MODE_READ_WRITE, SERVER_MEMORY, SERVER_SKILLS, SERVER_USAGE,
    SERVER_VAULT, VAULT_MODE_FULL, VAULT_MODE_REQUEST_ONLY,
};
use symbrain_policy::profile::parse::parse;

#[test]
fn parse_valid_full_profile() {
    let toml = r#"
[profile]
name        = "personal"
description = "Full access for trusted personal use"

[servers.vault]
enabled = true
mode    = "full"

[servers.memory]
enabled = true
mode    = "read_write"

[servers.skills]
enabled = true

[audit]
enabled = true
"#;

    let p = parse("personal", toml).expect("failed to parse valid profile");
    assert_eq!(p.name, "personal");
    assert_eq!(p.description, "Full access for trusted personal use");
    assert!(p.server(SERVER_VAULT).enabled);
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_FULL);
    assert!(p.server(SERVER_MEMORY).enabled);
    assert_eq!(p.server(SERVER_MEMORY).mode, MEMORY_MODE_READ_WRITE);
    assert!(p.server(SERVER_SKILLS).enabled);
    assert!(p.audit.enabled);
    assert!(p.warnings.is_empty());
}

#[test]
fn parse_valid_restricted_profile() {
    let toml = r#"
[profile]
name        = "restricted"
description = "Least-privilege profile"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true

[audit]
enabled = true
"#;

    let p = parse("restricted", toml).expect("failed to parse restricted profile");
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_REQUEST_ONLY);
    assert_eq!(p.server(SERVER_MEMORY).mode, MEMORY_MODE_READ_ONLY);
}

#[test]
fn parse_defaults_when_servers_and_audit_omitted() {
    let toml = r#"
[profile]
name = "bare"
"#;

    let p = parse("bare", toml).expect("failed to parse bare profile");
    assert!(!p.server(SERVER_VAULT).enabled);
    assert!(!p.server(SERVER_MEMORY).enabled);
    assert!(!p.server(SERVER_SKILLS).enabled);
    assert!(!p.server(SERVER_USAGE).enabled);
    assert!(p.audit.enabled);
}

#[test]
fn parse_server_enabled_without_mode_gets_least_privilege_default() {
    let toml = r#"
[profile]
name = "no-mode"

[servers.vault]
enabled = true

[servers.memory]
enabled = true
"#;

    let p = parse("no-mode", toml).expect("failed to parse profile");
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_REQUEST_ONLY);
    assert_eq!(p.server(SERVER_MEMORY).mode, MEMORY_MODE_READ_ONLY);
}

#[test]
fn parse_tools_allow_and_deny_both_parse() {
    let toml = r#"
[profile]
name = "lists"

[servers.memory]
enabled     = true
mode        = "read_write"
tools_allow = ["memory_search", "memory_set"]
tools_deny  = ["memory_set"]
"#;

    let p = parse("lists", toml).expect("failed to parse profile");
    let allow = &p.server(SERVER_MEMORY).tools_allow;
    let deny = &p.server(SERVER_MEMORY).tools_deny;
    assert_eq!(allow, &["memory_search", "memory_set"]);
    assert_eq!(deny, &["memory_set"]);
}

#[test]
fn parse_unknown_top_level_key_warns_not_fails() {
    let toml = r#"
[profile]
name   = "warny"
author = "someone"

[servers.vault]
enabled     = true
mode        = "full"
rate_limit  = 5
"#;

    let p = parse("warny", toml).expect("unknown keys should warn, not fail");
    assert_eq!(
        p.warnings,
        vec![
            "unknown key \"profile.author\"",
            "unknown key \"servers.vault.rate_limit\""
        ]
    );
}

#[test]
fn parse_foreign_server_without_transport_errors() {
    let toml = r#"
[profile]
name = "bad-alias"

[servers.wat]
enabled = true
"#;

    let err = parse("bad-alias", toml).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("requires command"),
        "expected error to mention requires command, got: {msg}"
    );
}

#[test]
fn parse_core_alias_cannot_be_foreign() {
    let toml = r#"
[profile]
name = "vault-as-foreign"

[servers.vault]
enabled = true
command = "/usr/bin/some-mcp"
"#;

    let err = parse("vault-as-foreign", toml).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("not a foreign server"),
        "expected error to mention not a foreign server, got: {msg}"
    );
}

#[test]
fn parse_foreign_server_with_command_and_args() {
    let toml = r#"
[profile]
name = "with-foreign"

[servers.zotero]
enabled = true
command = "/usr/local/bin/zotero-mcp"
args = ["--stdio", "--log", "/tmp/z.log"]

[servers.vault]
enabled = true
mode = "full"
"#;

    let p = parse("with-foreign", toml).expect("failed to parse profile");
    let z = p.server("zotero");
    assert!(z.enabled);
    assert_eq!(z.command, "/usr/local/bin/zotero-mcp");
    assert_eq!(z.args, vec!["--stdio", "--log", "/tmp/z.log"]);

    assert!(p.server(SERVER_VAULT).enabled);
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_FULL);
    assert!(!p.server(SERVER_MEMORY).enabled);
    assert!(!p.server(SERVER_USAGE).enabled);
}

#[test]
fn parse_foreign_server_with_url() {
    let toml = r#"
[profile]
name = "with-url"

[servers.fig]
enabled = true
url = "https://mcp.example.com/sse"
"#;

    let p = parse("with-url", toml).expect("failed to parse profile");
    let fig = p.server("fig");
    assert!(fig.enabled);
    assert_eq!(fig.url, "https://mcp.example.com/sse");
}

#[test]
fn parse_foreign_server_mode_ignored_with_warning() {
    let toml = r#"
[profile]
name = "foreign-mode"

[servers.fig]
enabled = true
mode = "full"
command = "/usr/bin/fig-mcp"
"#;

    let p = parse("foreign-mode", toml).expect("failed to parse profile");
    assert!(
        p.warnings
            .iter()
            .any(|w| w.contains("fig") && w.contains("mode"))
    );
}

#[test]
fn parse_inline_tables_under_servers_profile_audit() {
    let toml = r#"
profile = { name = "inline-all", description = "fully inline" }
audit = { enabled = false }
servers = { vault = { enabled = true, mode = "full" }, memory = { enabled = true, mode = "read_write" }, skills = { enabled = true } }
"#;

    let p = parse("inline-all", toml).expect("failed to parse inline profile");
    assert_eq!(p.name, "inline-all");
    assert_eq!(p.description, "fully inline");
    assert!(!p.audit.enabled);
    assert!(p.server(SERVER_VAULT).enabled);
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_FULL);
    assert!(p.server(SERVER_MEMORY).enabled);
    assert_eq!(p.server(SERVER_MEMORY).mode, MEMORY_MODE_READ_WRITE);
    assert!(p.server(SERVER_SKILLS).enabled);
    assert!(p.warnings.is_empty());
}

#[test]
fn parse_nested_inline_and_dotted_combinations() {
    let toml = r#"
[profile]
name = "combo"

[servers]
vault = { enabled = true, mode = "request_only" }
memory = { enabled = true, mode = "read_only" }

[servers.echo]
command = "/bin/echo"
args = ["test"]
"#;

    let p = parse("combo", toml).expect("failed to parse combo profile");
    assert_eq!(p.name, "combo");
    assert!(p.server(SERVER_VAULT).enabled);
    assert_eq!(p.server(SERVER_VAULT).mode, VAULT_MODE_REQUEST_ONLY);
    assert!(p.server(SERVER_MEMORY).enabled);
    assert_eq!(p.server(SERVER_MEMORY).mode, MEMORY_MODE_READ_ONLY);
    let echo = p.server("echo");
    assert_eq!(echo.command, "/bin/echo");
    assert_eq!(echo.args, vec!["test"]);
    assert!(p.warnings.is_empty());
}

#[test]
fn unknown_keys_reports_only_leaf_and_explicit_tables_never_intermediate() {
    let toml = r#"
[profile]
name = "warn-check"

[servers.vault]
enabled = true
foo.bar = 1

[servers.vault.sub1.sub2]
leaf = 42

[unknown.top]
deep_leaf = 99
"#;

    let p = parse("warn-check", toml).expect("failed to parse profile");
    assert_eq!(
        p.warnings,
        vec![
            "unknown key \"servers.vault.foo.bar\"",
            "unknown key \"servers.vault.sub1.sub2\"",
            "unknown key \"servers.vault.sub1.sub2.leaf\"",
            "unknown key \"unknown.top\"",
            "unknown key \"unknown.top.deep_leaf\"",
        ]
    );
}
