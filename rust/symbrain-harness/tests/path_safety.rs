use std::path::PathBuf;

use symbrain_harness::{list_for_env, profile_bindings};

#[test]
fn profile_binding_scan_preserves_leading_parent_sibling_path() {
    let project_dir = PathBuf::from("..").join("..").join("sibling");
    let config_path = project_dir.join(".mcp.json").to_string_lossy().into_owned();
    let scan = profile_bindings("sibling", Some(&project_dir));
    let project_error = scan
        .errors
        .iter()
        .find(|error| error.path == config_path)
        .expect("claude project path should be inspected");
    assert_eq!(project_error.path, config_path);
    assert!(!project_error.error.is_empty());
}

#[test]
fn inventory_display_preserves_leading_parent_sibling_path() {
    let project_dir = PathBuf::from("..").join("..").join("sibling");
    let config_path = project_dir.join(".mcp.json").to_string_lossy().into_owned();
    let inventory = list_for_env(Some(&project_dir), "linux", &[]);
    assert_eq!(
        inventory.project_dir.as_deref(),
        Some(project_dir.to_string_lossy().as_ref())
    );
    let claude = inventory
        .harnesses
        .iter()
        .find(|harness| harness.name.to_string() == "claude")
        .expect("claude harness should be present");
    assert_eq!(
        claude.project.as_ref().map(|project| project.path.as_str()),
        Some(config_path.as_str())
    );
}
