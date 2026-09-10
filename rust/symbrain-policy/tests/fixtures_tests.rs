//! Deterministic production fixtures and parameterized contract tests.

use serde_json::Value;

use symbrain_policy::constants::{
    FOREIGN_ACCESS_READ, MEMORY_MODE_READ_ONLY, MEMORY_MODE_READ_WRITE, SERVER_MEMORY,
    SERVER_SKILLS, SERVER_USAGE, SERVER_VAULT, VAULT_MODE_FULL, VAULT_MODE_REQUEST_ONLY,
};
use symbrain_policy::policy::ForeignTool;
use symbrain_policy::policy::eval::evaluate_preset;
use symbrain_policy::policy::foreign::evaluate_foreign;
use symbrain_policy::profile::parse::parse;

/// Production template from Go `cmd/symbrain/cmd_init.go:personalProfileTOML`.
const PROD_PERSONAL_PROFILE_TOML: &str = r#"# Example profile: full access for trusted personal use.
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

[servers.usage]
enabled = true

[audit]
enabled = true
"#;

/// Production template from Go `cmd/symbrain/cmd_init.go:restrictedProfileTOML`.
const PROD_RESTRICTED_PROFILE_TOML: &str = r#"# Example profile: least-privilege for untrusted or shared harnesses.
[profile]
name        = "restricted"
description = "Least-privilege profile for untrusted or shared harnesses"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[audit]
enabled = true
"#;

/// Production template from Go `cmd/symbrain/cmd_init.go:foreignReadOnlyProfileTOML`.
const PROD_FOREIGN_READ_ONLY_PROFILE_TOML: &str = r#"# Example profile: read-only access to a foreign (non-core) MCP server,
# alongside restricted access to the four cores.
[profile]
name        = "foreign-read-only"
description = "Read-only access to a foreign server, plus the four cores"

[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true

[servers.usage]
enabled = true

[servers.docs]
enabled    = true
command    = "/usr/local/bin/docs-mcp"
access     = "read"
tools_read = ["search", "get"]

[audit]
enabled = true
"#;

#[test]
fn test_prod_personal_fixture_contract() {
    let p = parse("personal", PROD_PERSONAL_PROFILE_TOML).expect("parse personal failed");
    assert_eq!(p.name, "personal");
    assert_eq!(p.description, "Full access for trusted personal use");
    assert!(p.warnings.is_empty());

    let vault = p.server(SERVER_VAULT);
    assert!(vault.enabled);
    assert_eq!(vault.mode, VAULT_MODE_FULL);

    let memory = p.server(SERVER_MEMORY);
    assert!(memory.enabled);
    assert_eq!(memory.mode, MEMORY_MODE_READ_WRITE);

    let skills = p.server(SERVER_SKILLS);
    assert!(skills.enabled);

    let usage = p.server(SERVER_USAGE);
    assert!(usage.enabled);

    assert!(p.audit.enabled);

    // Evaluate preset policies
    let v_rep = evaluate_preset(SERVER_VAULT, &vault).unwrap();
    assert_eq!(v_rep.exposed.len(), 10);

    let m_rep = evaluate_preset(SERVER_MEMORY, &memory).unwrap();
    assert_eq!(m_rep.exposed.len(), 8);

    // JSON serialization shape check
    let json_val: Value = serde_json::to_value(&p).unwrap();
    assert_eq!(json_val["name"], "personal");
    assert_eq!(json_val["servers"]["vault"]["mode"], "full");
    assert_eq!(json_val["servers"]["memory"]["mode"], "read_write");
    assert_eq!(json_val["servers"]["skills"]["enabled"], true);
    assert_eq!(json_val["servers"]["usage"]["enabled"], true);
    assert_eq!(json_val["audit"]["enabled"], true);
    assert!(json_val.get("warnings").is_none());
}

#[test]
fn test_prod_restricted_fixture_contract() {
    let p = parse("restricted", PROD_RESTRICTED_PROFILE_TOML).expect("parse restricted failed");
    assert_eq!(p.name, "restricted");
    assert_eq!(
        p.description,
        "Least-privilege profile for untrusted or shared harnesses"
    );
    assert!(p.warnings.is_empty());

    let vault = p.server(SERVER_VAULT);
    assert!(vault.enabled);
    assert_eq!(vault.mode, VAULT_MODE_REQUEST_ONLY);

    let memory = p.server(SERVER_MEMORY);
    assert!(memory.enabled);
    assert_eq!(memory.mode, MEMORY_MODE_READ_ONLY);

    let skills = p.server(SERVER_SKILLS);
    assert!(skills.enabled);

    let usage = p.server(SERVER_USAGE);
    assert!(usage.enabled);

    // Evaluate preset policies
    let v_rep = evaluate_preset(SERVER_VAULT, &vault).unwrap();
    assert_eq!(v_rep.exposed.len(), 3);
    assert_eq!(
        v_rep.exposed,
        vec!["generate_password", "health", "request_credential"]
    );

    let m_rep = evaluate_preset(SERVER_MEMORY, &memory).unwrap();
    assert_eq!(m_rep.exposed.len(), 6);
    assert_eq!(
        m_rep.exposed,
        vec![
            "entity_list",
            "entity_resolve",
            "graph_neighbors",
            "memory_get",
            "memory_list",
            "memory_search"
        ]
    );
}

#[test]
fn test_prod_foreign_read_only_fixture_contract() {
    let p = parse("foreign-read-only", PROD_FOREIGN_READ_ONLY_PROFILE_TOML)
        .expect("parse foreign-read-only failed");
    assert_eq!(p.name, "foreign-read-only");
    assert!(p.warnings.is_empty());

    let docs = p.server("docs");
    assert!(docs.enabled);
    assert_eq!(docs.command, "/usr/local/bin/docs-mcp");
    assert_eq!(docs.access, FOREIGN_ACCESS_READ);
    assert_eq!(docs.tools_read, vec!["search", "get"]);

    // Evaluate foreign server policy
    let tools = vec![
        ForeignTool::new("search"),
        ForeignTool::new("get"),
        ForeignTool::new("write_doc"),
    ];

    let report = evaluate_foreign("docs", &docs, &tools).unwrap();
    assert_eq!(report.exposed, vec!["get", "search"]);
    assert_eq!(report.hidden, vec!["write_doc"]);
}

#[test]
fn test_parameterized_negative_cases() {
    let test_cases = [
        ("empty name", "", "[profile]\nname = \"\"\n", "invalid name"),
        (
            "bad vault mode",
            "bad-vault",
            "[profile]\nname = \"bad-vault\"\n[servers.vault]\nenabled = true\nmode = \"invalid\"\n",
            "servers.vault: invalid mode",
        ),
        (
            "bad memory mode",
            "bad-mem",
            "[profile]\nname = \"bad-mem\"\n[servers.memory]\nenabled = true\nmode = \"read_only_write\"\n",
            "servers.memory: invalid mode",
        ),
        (
            "core as foreign",
            "core-foreign",
            "[profile]\nname = \"core-foreign\"\n[servers.vault]\ncommand = \"/bin/vault\"\n",
            "core server cannot carry command/args/url",
        ),
        (
            "foreign without transport",
            "no-transport",
            "[profile]\nname = \"no-transport\"\n[servers.my_server]\nenabled = true\n",
            "foreign server requires command",
        ),
        (
            "foreign bad access",
            "bad-access",
            "[profile]\nname = \"bad-access\"\n[servers.my_server]\ncommand = \"/bin/s\"\naccess = \"execute\"\n",
            "invalid access",
        ),
    ];

    for (label, name, toml, expected_substr) in test_cases {
        let res = parse(name, toml);
        assert!(res.is_err(), "case {label} was expected to fail");
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains(expected_substr),
            "case {label}: expected {expected_substr} in {err_msg}"
        );
    }
}
