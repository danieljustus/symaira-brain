//! Globally disabled targets in the render, runner and inventory layers (#904).

use std::fs;
use std::path::Path;

use symbrain_skills::runner::{NoopContext, Options, run};
use symbrain_skills::targets_status::{StatusOptions, list_status};
use symbrain_skills::{RenderMetadata, enabled_target_names, load_bundle, render_target};

fn skill(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nbody\n",
    )
    .unwrap();
}

fn disabled() -> Vec<String> {
    vec!["openclaw".to_owned()]
}

#[test]
fn enabled_names_drop_only_the_disabled_target() {
    let all = symbrain_skills::target_names();
    let enabled = enabled_target_names(&disabled());
    assert!(all.contains(&"openclaw".to_owned()));
    assert!(!enabled.contains(&"openclaw".to_owned()));
    assert_eq!(enabled.len() + 1, all.len());
    assert_eq!(enabled_target_names(&[]), all);
}

#[test]
fn render_refuses_a_disabled_target() {
    let temp = tempfile::tempdir().unwrap();
    skill(temp.path());
    let bundle = load_bundle(temp.path()).unwrap();
    let metadata = RenderMetadata {
        disabled_targets: disabled(),
        ..RenderMetadata::default()
    };
    let error = render_target(&bundle, "openclaw", &metadata).unwrap_err();
    assert!(error.to_string().contains("disabled"), "{error}");
    assert!(render_target(&bundle, "claude", &metadata).is_ok());
    assert!(render_target(&bundle, "openclaw", &RenderMetadata::default()).is_ok());
}

#[test]
fn runner_skips_a_disabled_target_and_renders_nothing_for_it() {
    let temp = tempfile::tempdir().unwrap();
    skill(&temp.path().join("library/demo"));
    let results = run(
        &mut NoopContext,
        &["openclaw".into(), "claude".into()],
        Options {
            library_dir: temp.path().join("library").display().to_string(),
            render_dir: temp.path().join("rendered").display().to_string(),
            base_dir: temp.path().join("base").display().to_string(),
            home_dir: temp.path().join("home").display().to_string(),
            disabled_targets: disabled(),
            ..Options::default()
        },
        false,
    )
    .unwrap();
    assert_eq!(results[0].target, "openclaw");
    assert_eq!(results[0].status, "skipped");
    assert!(results[0].message.as_deref().unwrap().contains("disabled"));
    assert_eq!(results[1].status, "ok");
    assert!(!temp.path().join("rendered/openclaw").exists());
    assert!(!temp.path().join("base/openclaw").exists());
    assert!(!temp.path().join("home/.openclaw").exists());
    assert!(temp.path().join("rendered/claude").exists());
}

#[test]
fn target_inventory_marks_the_disabled_target() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir_all(temp.path().join(".openclaw/skills/leftover")).unwrap();
    let rows = list_status(&StatusOptions {
        home_dir: temp.path().to_path_buf(),
        disabled_targets: disabled(),
        ..StatusOptions::default()
    });
    let row = rows.iter().find(|row| row.target == "openclaw").unwrap();
    assert_eq!(row.install_state, "disabled");
    assert_eq!(row.unmanaged_skills_count, 0);
    assert!(
        rows.iter()
            .filter(|row| row.target != "openclaw")
            .all(|row| row.install_state != "disabled")
    );
}
