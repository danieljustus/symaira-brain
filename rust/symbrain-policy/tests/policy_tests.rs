//! Tests for core policy evaluation and presets.

use std::collections::BTreeSet;

use symbrain_policy::constants::{
    MEMORY_MODE_READ_ONLY, MEMORY_MODE_READ_WRITE, SERVER_MEMORY, SERVER_OPERATE, SERVER_SCOPE,
    SERVER_SKILLS, SERVER_USAGE, SERVER_VAULT, VAULT_MODE_FULL, VAULT_MODE_OFF,
    VAULT_MODE_REQUEST_ONLY,
};
use symbrain_policy::policy::eval::{evaluate, evaluate_preset};
use symbrain_policy::policy::identity::identity_parameter;
use symbrain_policy::policy::presets::{known_tools, preset_tools};
use symbrain_policy::policy::{Report, Verdict};
use symbrain_policy::profile::ServerConfig;

fn assert_partition(live: &[String], report: &Report) {
    let mut seen = BTreeSet::new();
    for tool in &report.exposed {
        assert!(seen.insert(tool), "duplicate tool in exposed: {tool}");
    }
    for tool in &report.hidden {
        assert!(seen.insert(tool), "duplicate tool in hidden: {tool}");
    }
    for tool in &report.unknown {
        assert!(seen.insert(tool), "duplicate tool in unknown: {tool}");
    }

    assert_eq!(
        seen.len(),
        live.len(),
        "partition size mismatch: seen {} vs live {}",
        seen.len(),
        live.len()
    );
    for tool in live {
        assert!(seen.contains(tool), "tool {tool} missing from partition");
    }
}

#[test]
fn test_identity_parameter_mapping() {
    assert_eq!(identity_parameter(SERVER_MEMORY), Some("client_id"));
    assert_eq!(identity_parameter(SERVER_VAULT), None);
    assert_eq!(identity_parameter(SERVER_SKILLS), None);
    assert_eq!(identity_parameter("some_other"), None);
    assert_eq!(identity_parameter(""), None);
}

#[test]
fn test_evaluate_vault_modes() {
    let mut live = known_tools(SERVER_VAULT);
    live.push("vault_new_upstream_tool".to_string());

    let vault_full = known_tools(SERVER_VAULT);
    let cases = vec![
        (
            "request_only",
            ServerConfig {
                enabled: true,
                mode: VAULT_MODE_REQUEST_ONLY.to_string(),
                ..Default::default()
            },
            vec!["generate_password", "health", "request_credential"],
            vec!["vault_new_upstream_tool"],
        ),
        (
            "full",
            ServerConfig {
                enabled: true,
                mode: VAULT_MODE_FULL.to_string(),
                ..Default::default()
            },
            vault_full.iter().map(String::as_str).collect(),
            vec!["vault_new_upstream_tool"],
        ),
        (
            "off",
            ServerConfig {
                enabled: true,
                mode: VAULT_MODE_OFF.to_string(),
                ..Default::default()
            },
            vec![],
            vec!["vault_new_upstream_tool"],
        ),
        (
            "disabled",
            ServerConfig {
                enabled: false,
                mode: VAULT_MODE_FULL.to_string(),
                ..Default::default()
            },
            vec![],
            vec!["vault_new_upstream_tool"],
        ),
    ];

    for (name, cfg, want_exposed, want_unknown) in cases {
        let report = evaluate(SERVER_VAULT, &cfg, &live).expect(name);
        assert_eq!(report.exposed, want_exposed, "mismatch in {name} exposed");
        assert_eq!(
            report.unknown,
            want_unknown
                .iter()
                .map(|&s| s.to_string())
                .collect::<Vec<_>>(),
            "mismatch in {name} unknown"
        );
        assert_partition(&live, &report);
    }
}

#[test]
fn test_evaluate_memory_modes() {
    let mut live = known_tools(SERVER_MEMORY);
    live.push("memory_new_upstream_tool".to_string());

    let read_only_cfg = ServerConfig {
        enabled: true,
        mode: MEMORY_MODE_READ_ONLY.to_string(),
        ..Default::default()
    };
    let read_only_report = evaluate(SERVER_MEMORY, &read_only_cfg, &live).unwrap();
    assert_eq!(
        read_only_report.exposed,
        vec![
            "entity_list",
            "entity_resolve",
            "graph_neighbors",
            "memory_get",
            "memory_list",
            "memory_search"
        ]
    );
    assert_eq!(
        read_only_report.unknown,
        vec!["memory_new_upstream_tool".to_string()]
    );
    assert_partition(&live, &read_only_report);

    let read_write_cfg = ServerConfig {
        enabled: true,
        mode: MEMORY_MODE_READ_WRITE.to_string(),
        ..Default::default()
    };
    let read_write_report = evaluate(SERVER_MEMORY, &read_write_cfg, &live).unwrap();
    assert_eq!(read_write_report.exposed, known_tools(SERVER_MEMORY));
    assert_eq!(
        read_write_report.unknown,
        vec!["memory_new_upstream_tool".to_string()]
    );
    assert_partition(&live, &read_write_report);
}

#[test]
fn test_evaluate_unknown_upstream_tool_never_exposed_under_preset() {
    let unknown = "vault_totally_new_dangerous_tool".to_string();
    let mut live = known_tools(SERVER_VAULT);
    live.push(unknown.clone());

    for mode in [VAULT_MODE_REQUEST_ONLY, VAULT_MODE_FULL] {
        let cfg = ServerConfig {
            enabled: true,
            mode: mode.to_string(),
            ..Default::default()
        };
        let report = evaluate(SERVER_VAULT, &cfg, &live).unwrap();
        assert!(
            !report.exposed.contains(&unknown),
            "unknown tool exposed under mode {mode}"
        );
        assert!(
            report.unknown.contains(&unknown),
            "unknown tool missing from Unknown bucket"
        );
    }
}

#[test]
fn test_optional_modules_allowlist_cannot_widen_hard_maximum() {
    let cases = [
        (
            SERVER_OPERATE,
            vec!["click"],
            vec!["click"],
            Vec::<&str>::new(),
            vec!["click"],
        ),
        (
            SERVER_OPERATE,
            vec!["version", "click"],
            vec!["version", "click"],
            vec!["version"],
            vec!["click"],
        ),
        (
            SERVER_SCOPE,
            vec!["write_host"],
            vec!["write_host"],
            Vec::<&str>::new(),
            vec!["write_host"],
        ),
        (
            SERVER_OPERATE,
            vec!["unknown_upstream_tool"],
            Vec::<&str>::new(),
            Vec::<&str>::new(),
            vec!["unknown_upstream_tool"],
        ),
    ];
    for (alias, live_names, allow_names, want_exposed, want_unknown) in cases {
        let live: Vec<String> = live_names.into_iter().map(String::from).collect();
        let cfg = ServerConfig {
            enabled: true,
            tools_allow: allow_names.into_iter().map(String::from).collect(),
            ..ServerConfig::default()
        };
        let report = evaluate(alias, &cfg, &live).expect("optional policy should evaluate");
        assert_eq!(
            report.exposed,
            want_exposed
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            report.unknown,
            want_unknown
                .into_iter()
                .map(String::from)
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_evaluate_tools_allow_overrides_preset() {
    let live = known_tools(SERVER_MEMORY);
    let cfg = ServerConfig {
        enabled: true,
        mode: MEMORY_MODE_READ_ONLY.to_string(),
        tools_allow: vec!["memory_set".to_string()],
        ..Default::default()
    };

    let report = evaluate(SERVER_MEMORY, &cfg, &live).unwrap();
    assert_eq!(report.exposed, vec!["memory_set"]);
    assert_partition(&live, &report);
}

#[test]
fn test_evaluate_tools_deny_removes_from_preset() {
    let live = known_tools(SERVER_VAULT);
    let cfg = ServerConfig {
        enabled: true,
        mode: VAULT_MODE_FULL.to_string(),
        tools_deny: vec!["set_entry_field".to_string()],
        ..Default::default()
    };

    let report = evaluate(SERVER_VAULT, &cfg, &live).unwrap();
    assert!(!report.exposed.contains(&"set_entry_field".to_string()));
    assert!(report.hidden.contains(&"set_entry_field".to_string()));
    assert_partition(&live, &report);
}

#[test]
fn test_evaluate_deny_wins_over_allow() {
    let live = vec!["memory_set".to_string(), "memory_search".to_string()];
    let cfg = ServerConfig {
        enabled: true,
        mode: MEMORY_MODE_READ_WRITE.to_string(),
        tools_allow: vec!["memory_set".to_string(), "memory_search".to_string()],
        tools_deny: vec!["memory_set".to_string()],
        ..Default::default()
    };

    let report = evaluate(SERVER_MEMORY, &cfg, &live).unwrap();
    assert_eq!(report.exposed, vec!["memory_search"]);
    assert_eq!(report.hidden, vec!["memory_set"]);
}

#[test]
fn test_evaluate_skills_matrix() {
    let live = vec![
        "skill_install".to_string(),
        "skill_list".to_string(),
        "skill_render".to_string(),
    ];

    // Enabled with no allow/deny -> always-full
    let cfg_full = ServerConfig {
        enabled: true,
        ..Default::default()
    };
    let rep_full = evaluate(SERVER_SKILLS, &cfg_full, &live).unwrap();
    assert_eq!(
        rep_full.exposed,
        vec!["skill_install", "skill_list", "skill_render"]
    );
    assert_eq!(rep_full.unknown, Vec::<String>::new());

    // Disabled -> nothing exposed
    let cfg_dis = ServerConfig {
        enabled: false,
        ..Default::default()
    };
    let rep_dis = evaluate(SERVER_SKILLS, &cfg_dis, &live).unwrap();
    assert!(rep_dis.exposed.is_empty());
    assert_eq!(
        rep_dis.hidden,
        vec!["skill_install", "skill_list", "skill_render"]
    );

    // Deny narrows
    let cfg_deny = ServerConfig {
        enabled: true,
        tools_deny: vec!["skill_install".to_string()],
        ..Default::default()
    };
    let rep_deny = evaluate(SERVER_SKILLS, &cfg_deny, &live).unwrap();
    assert_eq!(rep_deny.exposed, vec!["skill_list", "skill_render"]);
    assert_eq!(rep_deny.hidden, vec!["skill_install"]);

    // Allow replaces
    let cfg_allow = ServerConfig {
        enabled: true,
        tools_allow: vec!["skill_render".to_string()],
        ..Default::default()
    };
    let rep_allow = evaluate(SERVER_SKILLS, &cfg_allow, &live).unwrap();
    assert_eq!(rep_allow.exposed, vec!["skill_render"]);
    assert_eq!(rep_allow.hidden, vec!["skill_install", "skill_list"]);
}

#[test]
fn test_evaluate_usage_matrix() {
    let live = vec!["get_ai_usage".to_string()];

    let cfg_on = ServerConfig {
        enabled: true,
        ..Default::default()
    };
    let rep_on = evaluate(SERVER_USAGE, &cfg_on, &live).unwrap();
    assert_eq!(rep_on.exposed, vec!["get_ai_usage"]);

    let cfg_off = ServerConfig {
        enabled: false,
        ..Default::default()
    };
    let rep_off = evaluate(SERVER_USAGE, &cfg_off, &live).unwrap();
    assert!(rep_off.exposed.is_empty());
    assert_eq!(rep_off.hidden, vec!["get_ai_usage"]);

    let cfg_deny = ServerConfig {
        enabled: true,
        tools_deny: vec!["get_ai_usage".to_string()],
        ..Default::default()
    };
    let rep_deny = evaluate(SERVER_USAGE, &cfg_deny, &live).unwrap();
    assert!(rep_deny.exposed.is_empty());
    assert_eq!(rep_deny.hidden, vec!["get_ai_usage"]);

    let live_with_unknown = vec!["get_ai_usage".to_string(), "usage_delete_all".to_string()];
    let rep_unknown = evaluate(SERVER_USAGE, &cfg_on, &live_with_unknown).unwrap();
    assert_eq!(rep_unknown.exposed, vec!["get_ai_usage"]);
    assert_eq!(rep_unknown.unknown, vec!["usage_delete_all".to_string()]);
}

#[test]
fn test_evaluate_preset_vault_and_memory() {
    let v_cfg = ServerConfig {
        enabled: true,
        mode: VAULT_MODE_REQUEST_ONLY.to_string(),
        ..Default::default()
    };
    let v_rep = evaluate_preset(SERVER_VAULT, &v_cfg).unwrap();
    assert_eq!(
        v_rep.exposed,
        vec!["generate_password", "health", "request_credential"]
    );
    assert_eq!(v_rep.unknown, Vec::<String>::new());

    let m_cfg = ServerConfig {
        enabled: true,
        mode: MEMORY_MODE_READ_WRITE.to_string(),
        ..Default::default()
    };
    let m_rep = evaluate_preset(SERVER_MEMORY, &m_cfg).unwrap();
    assert_eq!(m_rep.exposed, known_tools(SERVER_MEMORY));

    assert!(evaluate_preset(SERVER_SKILLS, &v_cfg).is_err());
}

#[test]
fn test_preset_tools_lookup() {
    let tools = preset_tools(SERVER_VAULT, VAULT_MODE_OFF).unwrap();
    assert!(tools.is_empty());

    let req_only = preset_tools(SERVER_VAULT, VAULT_MODE_REQUEST_ONLY).unwrap();
    assert_eq!(
        req_only,
        vec!["request_credential", "generate_password", "health"]
    );

    assert!(preset_tools(SERVER_VAULT, "not-a-mode").is_err());
}

#[test]
fn test_report_verdict_lookup() {
    let live = vec![
        "memory_search".to_string(),
        "memory_set".to_string(),
        "totally_new".to_string(),
    ];
    let cfg = ServerConfig {
        enabled: true,
        mode: MEMORY_MODE_READ_ONLY.to_string(),
        ..Default::default()
    };

    let report = evaluate(SERVER_MEMORY, &cfg, &live).unwrap();
    assert_eq!(report.verdict("memory_search"), Verdict::Exposed);
    assert_eq!(report.verdict("memory_set"), Verdict::Hidden);
    assert_eq!(report.verdict("totally_new"), Verdict::Unknown);
    assert_eq!(report.verdict("never_seen"), Verdict::Unknown);
}
