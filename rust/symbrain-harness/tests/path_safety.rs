use std::path::Path;

use symbrain_harness::{list_for_env, profile_bindings};

#[test]
fn profile_binding_scan_preserves_leading_parent_sibling_path() {
    let scan = profile_bindings("sibling", Some(Path::new("../../sibling")));
    let project_error = scan
        .errors
        .iter()
        .find(|error| error.path == "../../sibling/.mcp.json")
        .expect("claude project path should be inspected");
    assert_eq!(project_error.path, "../../sibling/.mcp.json");
    assert!(
        project_error.error.contains("No such file")
            || project_error.error.contains("no such file")
    );
}

#[test]
fn inventory_display_preserves_leading_parent_sibling_path() {
    let inventory = list_for_env(Some(Path::new("../../sibling")), "linux", &[]);
    assert_eq!(inventory.project_dir.as_deref(), Some("../../sibling"));
    let claude = inventory
        .harnesses
        .iter()
        .find(|harness| harness.name.to_string() == "claude")
        .expect("claude harness should be present");
    assert_eq!(
        claude.project.as_ref().map(|project| project.path.as_str()),
        Some("../../sibling/.mcp.json")
    );
}
