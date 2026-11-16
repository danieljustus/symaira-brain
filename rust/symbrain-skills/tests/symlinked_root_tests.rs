//! A target's user skills root may itself be a symlink to a directory (#1066);
//! symlinks below that root stay rejected.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use symbrain_skills::install::{
    InstallOptions, StatusKind, StatusOptions, install_copy, install_rendered, read_events, status,
    uninstall,
};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

fn write_source(root: &Path) {
    fs::create_dir_all(root).expect("source root");
    fs::write(
        root.join("SKILL.md"),
        "---\nname: demo\ndescription: test\n---\nbody\n",
    )
    .expect("skill source");
}

#[test]
fn copy_install_into_symlinked_root_succeeds_and_round_trips() {
    let dir = tempfile::tempdir().expect("disposable root");
    let source = dir.path().join("source");
    write_source(&source);
    let home = dir.path().join("home");
    let real_root = dir.path().join("real-skills");
    fs::create_dir_all(&real_root).expect("real root");
    let link = home.join(".config/opencode/skills");
    fs::create_dir_all(link.parent().expect("parent")).expect("config dir");
    symlink(&real_root, &link).expect("symlinked root");
    let events = dir.path().join("events.jsonl");
    let options = InstallOptions {
        home_dir: home.clone(),
        events_path: Some(events.clone()),
        ..Default::default()
    };

    install_copy(&source, "demo", "hash", &options).expect("install into symlinked root");
    assert!(real_root.join("demo/SKILL.md").is_file());
    assert!(real_root.join("demo/.symskills.json").is_file());
    assert!(
        fs::symlink_metadata(&link)
            .expect("root metadata")
            .file_type()
            .is_symlink(),
        "the configured root stays a symlink"
    );
    let recorded = read_events(&events, Some("demo"), None).expect("events");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].outcome, "ok", "{}", recorded[0].error);

    let rows = status(&StatusOptions {
        home_dir: home.clone(),
        library_dir: dir.path().join("library"),
        targets: vec!["opencode".to_owned()],
        ..Default::default()
    })
    .expect("status");
    assert_eq!(rows.len(), 1);
    assert_ne!(rows[0].status, StatusKind::Unmanaged, "{:?}", rows[0].error);

    install_copy(&source, "demo", "hash", &options).expect("reinstall");
    assert!(uninstall("opencode", "demo", &options).expect("uninstall"));
    assert!(!real_root.join("demo").exists());
}

#[test]
fn symlink_mode_install_into_symlinked_claude_root_records_ok() {
    let dir = tempfile::tempdir().expect("disposable root");
    let library = dir.path().join("library");
    let skill = library.join("demo");
    write_source(&skill);
    let bundle = load_bundle(&skill).expect("bundle");
    let rendered = render_target(&bundle, "claude", &RenderMetadata::default()).expect("render");
    let home = dir.path().join("home");
    let real_root = dir.path().join("elsewhere/claude-skills");
    fs::create_dir_all(&real_root).expect("real root");
    fs::create_dir_all(home.join(".claude")).expect("claude dir");
    symlink(&real_root, home.join(".claude/skills")).expect("symlinked root");
    let events = dir.path().join("events.jsonl");

    let result = install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: home,
            mode: "symlink".to_owned(),
            events_path: Some(events.clone()),
            ..Default::default()
        },
    )
    .expect("install into symlinked root");
    assert_eq!(result.action, "installed");
    assert!(
        fs::symlink_metadata(real_root.join("demo"))
            .expect("installed link")
            .file_type()
            .is_symlink()
    );
    let recorded = read_events(&events, Some("demo"), Some("claude")).expect("events");
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].outcome, "ok", "{}", recorded[0].error);
}

#[test]
fn symlink_below_a_symlinked_root_is_still_rejected() {
    let dir = tempfile::tempdir().expect("disposable root");
    let source = dir.path().join("source");
    write_source(&source);
    let home = dir.path().join("home");
    let real_root = dir.path().join("real-skills");
    let outside = dir.path().join("outside");
    fs::create_dir_all(&real_root).expect("real root");
    fs::create_dir_all(&outside).expect("outside");
    fs::write(outside.join("keep.txt"), b"keep").expect("outside file");
    let link = home.join(".config/opencode/skills");
    fs::create_dir_all(link.parent().expect("parent")).expect("config dir");
    symlink(&real_root, &link).expect("symlinked root");
    // A link swapped in below the resolved root must not be followed.
    symlink(&outside, real_root.join("demo")).expect("link below root");
    let options = InstallOptions {
        home_dir: home,
        ..Default::default()
    };

    install_copy(&source, "demo", "hash", &options).expect_err("below-root symlink is rejected");
    assert_eq!(
        fs::read(outside.join("keep.txt")).expect("outside file"),
        b"keep"
    );
    assert_eq!(fs::read_dir(&outside).expect("outside dir").count(), 1);
}

#[test]
fn root_linked_into_the_render_directory_installs_and_reinstalls() {
    // The reporter's layout: `~/.claude/skills -> <render_dir>/claude`.
    let dir = tempfile::tempdir().expect("disposable root");
    let skill = dir.path().join("library/demo");
    write_source(&skill);
    let bundle = load_bundle(&skill).expect("bundle");
    let rendered = render_target(&bundle, "claude", &RenderMetadata::default()).expect("render");
    let home = dir.path().join("home");
    let render_dir = dir.path().join("rendered");
    fs::create_dir_all(render_dir.join("claude")).expect("render root");
    fs::create_dir_all(home.join(".claude")).expect("claude dir");
    symlink(render_dir.join("claude"), home.join(".claude/skills")).expect("symlinked root");
    let events = dir.path().join("events.jsonl");
    let options = InstallOptions {
        home_dir: home,
        render_dir: Some(render_dir.clone()),
        mode: "symlink".to_owned(),
        events_path: Some(events.clone()),
        ..Default::default()
    };

    for _ in 0..2 {
        install_rendered(&bundle, &rendered, &options).expect("install into symlinked root");
    }
    let tree = render_dir.join("claude/demo");
    assert!(
        tree.join("SKILL.md").is_file(),
        "render tree stays a real directory"
    );
    assert!(tree.join(".symskills.json").is_file(), "marker written");
    let recorded = read_events(&events, Some("demo"), Some("claude")).expect("events");
    assert_eq!(recorded.len(), 2);
    assert!(
        recorded.iter().all(|event| event.outcome == "ok"),
        "{recorded:?}"
    );
}
