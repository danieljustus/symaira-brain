//! Tests for foreign server exposure and read/write classification.

use symbrain_policy::constants::{
    EXPOSURE_SOURCE_DEFAULT_WRITE, EXPOSURE_SOURCE_READ_ONLY_HINT, EXPOSURE_SOURCE_TOOLS_READ,
    EXPOSURE_SOURCE_TOOLS_WRITE, FOREIGN_ACCESS_READ, FOREIGN_ACCESS_WRITE,
};
use symbrain_policy::policy::{ForeignTool, ToolAnnotations, evaluate_foreign};
use symbrain_policy::profile::ServerConfig;

fn ro(name: &str, read_only: Option<bool>) -> ForeignTool {
    let mut tool = ForeignTool::new(name);
    if let Some(ro) = read_only {
        tool = tool.with_read_only_hint(ro);
    }
    tool
}

fn foreign_cfg(access: &str, overrides: &[(&str, &str)]) -> ServerConfig {
    let mut cfg = ServerConfig {
        enabled: true,
        access: access.to_string(),
        ..Default::default()
    };
    for &(name, class) in overrides {
        if class == "read" {
            cfg.tools_read.push(name.to_string());
        } else {
            cfg.tools_write.push(name.to_string());
        }
    }
    cfg
}

#[test]
fn test_evaluate_foreign_access_read_exposes_only_reading_tools() {
    let tools = vec![
        ro("search", Some(true)),
        ro("delete", Some(false)),
        ro("unannotated", None),
    ];
    let report = evaluate_foreign("fig", &foreign_cfg(FOREIGN_ACCESS_READ, &[]), &tools).unwrap();
    assert_eq!(report.exposed, vec!["search"]);
    assert_eq!(report.hidden, vec!["delete", "unannotated"]);

    let exposures = report.exposures.unwrap();
    let search = &exposures["search"];
    assert_eq!(search.class, "read");
    assert_eq!(search.source, EXPOSURE_SOURCE_READ_ONLY_HINT);

    let delete = &exposures["delete"];
    assert_eq!(delete.class, "write");
    assert_eq!(delete.source, EXPOSURE_SOURCE_DEFAULT_WRITE);
}

#[test]
fn test_evaluate_foreign_access_write_exposes_everything() {
    let tools = vec![ro("search", Some(true)), ro("delete", Some(false))];
    let report = evaluate_foreign("fig", &foreign_cfg(FOREIGN_ACCESS_WRITE, &[]), &tools).unwrap();
    assert_eq!(report.exposed, vec!["delete", "search"]);
    assert!(report.hidden.is_empty());
}

#[test]
fn test_evaluate_foreign_tools_read_overrides_write_hint() {
    let cfg = foreign_cfg(FOREIGN_ACCESS_READ, &[("search", "read")]);
    let tools = vec![ro("search", Some(false))];
    let report = evaluate_foreign("fig", &cfg, &tools).unwrap();
    assert_eq!(report.exposed, vec!["search"]);

    let exposures = report.exposures.unwrap();
    assert_eq!(exposures["search"].source, EXPOSURE_SOURCE_TOOLS_READ);
}

#[test]
fn test_evaluate_foreign_tools_write_overrides_read_hint() {
    let cfg = foreign_cfg(FOREIGN_ACCESS_READ, &[("search", "write")]);
    let tools = vec![ro("search", Some(true))];
    let report = evaluate_foreign("fig", &cfg, &tools).unwrap();
    assert_eq!(report.hidden, vec!["search"]);

    let exposures = report.exposures.unwrap();
    assert_eq!(exposures["search"].source, EXPOSURE_SOURCE_TOOLS_WRITE);
}

#[test]
fn test_evaluate_foreign_unannotated_tool_defaults_to_write() {
    let cfg = foreign_cfg(FOREIGN_ACCESS_READ, &[]);
    let tools = vec![ForeignTool::new("some_tool")];
    let report = evaluate_foreign("fig", &cfg, &tools).unwrap();
    assert_eq!(report.hidden, vec!["some_tool"]);

    let exposures = report.exposures.unwrap();
    assert_eq!(exposures["some_tool"].class, "write");
    assert_eq!(exposures["some_tool"].source, EXPOSURE_SOURCE_DEFAULT_WRITE);
}

#[test]
fn test_evaluate_foreign_tools_deny_wins_over_access_and_allow() {
    let mut cfg = foreign_cfg(FOREIGN_ACCESS_WRITE, &[]);
    cfg.tools_deny.push("delete".to_string());
    let tools = vec![ro("delete", Some(true)), ro("search", Some(true))];
    let report = evaluate_foreign("fig", &cfg, &tools).unwrap();
    assert_eq!(report.hidden, vec!["delete"]);
    assert_eq!(report.exposed, vec!["search"]);
}

#[test]
fn test_evaluate_foreign_tools_allow_narrows() {
    let mut cfg = foreign_cfg(FOREIGN_ACCESS_WRITE, &[]);
    cfg.tools_allow.push("search".to_string());
    let tools = vec![ro("search", Some(true)), ro("other", Some(true))];
    let report = evaluate_foreign("fig", &cfg, &tools).unwrap();
    assert_eq!(report.exposed, vec!["search"]);
    assert_eq!(report.hidden, vec!["other"]);
}

#[test]
fn test_evaluate_foreign_disabled_hides_everything() {
    let mut cfg = foreign_cfg(FOREIGN_ACCESS_WRITE, &[]);
    cfg.enabled = false;
    let tools = vec![ro("search", Some(true))];
    let report = evaluate_foreign("fig", &cfg, &tools).unwrap();
    assert!(report.exposed.is_empty());
    assert_eq!(report.hidden, vec!["search"]);
}

#[test]
fn test_evaluate_foreign_invalid_access_errors() {
    let cfg = ServerConfig {
        enabled: true,
        access: "exec".to_string(),
        ..Default::default()
    };
    assert!(evaluate_foreign("fig", &cfg, &[]).is_err());
}

#[test]
fn test_evaluate_foreign_default_access_is_write() {
    let cfg = ServerConfig {
        enabled: true,
        ..Default::default()
    };
    let tools = vec![ForeignTool::new("anything")];
    let report = evaluate_foreign("fig", &cfg, &tools).unwrap();
    assert_eq!(report.exposed, vec!["anything"]);
}

#[test]
fn test_foreign_tool_with_full_annotations() {
    let tool = ForeignTool::new("destructive_op").with_annotations(ToolAnnotations {
        title: Some("Destructive Operation".to_string()),
        read_only_hint: Some(false),
        destructive_hint: Some(true),
        idempotent_hint: Some(false),
        open_world_hint: Some(true),
    });

    let cfg = foreign_cfg(FOREIGN_ACCESS_READ, &[]);
    let report = evaluate_foreign("destructive_srv", &cfg, &[tool]).unwrap();
    assert_eq!(report.hidden, vec!["destructive_op"]);
    let exp = &report.exposures.unwrap()["destructive_op"];
    assert_eq!(exp.class, "write");
}

#[test]
fn test_evaluate_foreign_empty_catalog_read() {
    let cfg = foreign_cfg(FOREIGN_ACCESS_READ, &[]);
    let report = evaluate_foreign("myforeign", &cfg, &[]).unwrap();
    assert!(report.exposed.is_empty());
    assert!(report.hidden.is_empty());
    assert!(report.enabled);
    assert_eq!(report.server, "myforeign");
}

#[test]
fn test_evaluate_foreign_empty_catalog_write() {
    let cfg = foreign_cfg(FOREIGN_ACCESS_WRITE, &[]);
    let report = evaluate_foreign("myforeign", &cfg, &[]).unwrap();
    assert!(report.exposed.is_empty());
    assert!(report.hidden.is_empty());
    assert!(report.enabled);
    assert_eq!(report.server, "myforeign");
}

#[test]
fn test_evaluate_foreign_empty_catalog_disabled() {
    let mut cfg = foreign_cfg(FOREIGN_ACCESS_WRITE, &[]);
    cfg.enabled = false;
    let report = evaluate_foreign("myforeign", &cfg, &[]).unwrap();
    assert!(report.exposed.is_empty());
    assert!(report.hidden.is_empty());
    assert!(!report.enabled);
    assert_eq!(report.server, "myforeign");
}
