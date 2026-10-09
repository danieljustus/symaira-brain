//! Original target inventory assertions against the shared production API.
fn display_name(target: &str) -> &'static str {
    symbrain_skills::lookup(target)
        .expect("registered target")
        .display_name
}
fn target_status(
    target: &str,
    home: &std::path::Path,
    project: Option<&std::path::Path>,
    scope: &str,
) -> Result<serde_json::Value, crate::GatewayError> {
    let options = symbrain_skills::targets_status::StatusOptions {
        home_dir: home.to_path_buf(),
        project_dir: project.map(std::path::Path::to_path_buf),
        scope: scope.into(),
    };
    let row = symbrain_skills::targets_status::list_status(&options)
        .into_iter()
        .find(|row| row.target == target)
        .expect("target row");
    serde_json::to_value(row).map_err(|error| crate::GatewayError::Serialization(error.to_string()))
}

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn target_status_matches_harness_inventory_semantics() {
    assert_eq!(display_name("codex"), "Codex");
    let root = std::env::temp_dir().join(format!(
        "symbrain-gateway-status-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    fs::create_dir_all(&root).expect("test root");
    let home = root.join("home");
    fs::create_dir_all(&home).expect("home");

    let missing = target_status("opencode", &home, None, "user").expect("missing");
    assert_eq!(missing["skill_root_exists"], false);
    assert_eq!(missing["managed_skills_count"], 0);
    assert_eq!(missing["install_state"], "missing");

    let skill_root = home.join(".config/opencode/skills");
    fs::create_dir_all(skill_root.join("manual")).expect("manual skill root");
    fs::write(skill_root.join("manual/.symskills.json"), b"not-json").expect("marker");
    #[cfg(unix)]
    std::os::unix::fs::symlink("manual", skill_root.join("linked")).expect("linked skill");
    fs::create_dir_all(skill_root.join("unmanaged")).expect("unmanaged skill");
    let present = target_status("opencode", &home, None, "user").expect("present");
    assert_eq!(present["display_name"], "OpenCode");
    #[cfg(unix)]
    assert_eq!(present["managed_skills_count"], 2);
    #[cfg(not(unix))]
    assert_eq!(present["managed_skills_count"], 1);
    assert_eq!(present["unmanaged_skills_count"], 1);
    assert_eq!(present["install_state"], "mixed");

    #[cfg(unix)]
    {
        let real_root = root.join("real-opencode-skills");
        fs::rename(&skill_root, &real_root).expect("move real skill root");
        std::os::unix::fs::symlink(&real_root, &skill_root).expect("link skill root");
        let linked = target_status("opencode", &home, None, "user").expect("linked root status");
        assert_eq!(linked["skill_root_exists"], true);
        assert_eq!(linked["skill_root_readable"], true);
        assert_eq!(linked["managed_skills_count"], 2);
        assert_eq!(linked["unmanaged_skills_count"], 1);
        assert_eq!(linked["install_state"], "mixed");
        fs::remove_file(&skill_root).expect("remove skill root link");
        fs::rename(real_root, skill_root).expect("restore real skill root");
    }

    let project = root.join("project");
    fs::create_dir_all(project.join(".hermes/skills/manual")).expect("project skill root");
    let project_status =
        target_status("hermes", &home, Some(&project), "project").expect("project");
    assert_eq!(project_status["display_name"], "Hermes");
    let expected = project
        .join(".hermes")
        .join("skills")
        .to_string_lossy()
        .into_owned();
    assert_eq!(project_status["effective_skill_root"], expected);
    fs::remove_dir_all(root).expect("test root cleanup");
}
