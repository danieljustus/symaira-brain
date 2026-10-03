//! Regression for edits made through installed links into the render cache.
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use symbrain_skills::install::{
    InstallOptions, RenderStatus, StatusKind, StatusOptions, install_rendered, status,
};
use symbrain_skills::{RenderMetadata, load_bundle, materialize, render_target};

struct Fixture {
    root: tempfile::TempDir,
    options: StatusOptions,
    cached: PathBuf,
    installed: PathBuf,
}

impl Fixture {
    fn new(target: &str, mode: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let library = root.path().join("library");
        let source = library.join("example");
        fs::create_dir_all(source.join("references")).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: example\ndescription: test render drift\n---\nLibrary body\n",
        )
        .unwrap();
        fs::write(source.join("references/lesson.md"), "Original lesson\n").unwrap();
        fs::write(
            source.join("skill.toml"),
            format!("[targets.{target}]\nenabled = true\nprepend = \"Target introduction\"\n"),
        )
        .unwrap();
        let bundle = load_bundle(&source).unwrap();
        let rendered = render_target(&bundle, target, &RenderMetadata::default()).unwrap();
        let render_dir = root.path().join("rendered");
        let result = install_rendered(
            &bundle,
            &rendered,
            &InstallOptions {
                home_dir: home.clone(),
                render_dir: Some(render_dir.clone()),
                mode: mode.to_owned(),
                ..Default::default()
            },
        )
        .unwrap();
        let cached = render_dir.join(target).join("example");
        if mode == "copy" {
            materialize(&bundle, &rendered, &render_dir).unwrap();
        }
        Self {
            root,
            options: StatusOptions {
                home_dir: home,
                library_dir: library,
                render_dir: Some(render_dir),
                targets: vec![target.to_owned()],
                ..Default::default()
            },
            cached,
            installed: result.path,
        }
    }

    fn row(&self) -> symbrain_skills::install::InstallStatus {
        let rows = status(&self.options).unwrap();
        assert_eq!(rows.len(), 1);
        rows.into_iter().next().unwrap()
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, relative: &Path, result: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(root.join(relative)).unwrap() {
            let entry = entry.unwrap();
            let path = relative.join(entry.file_name());
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                result.insert(path.clone(), b"directory".to_vec());
                visit(root, &path, result);
            } else if kind.is_symlink() {
                result.insert(
                    path,
                    fs::read_link(entry.path())
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec(),
                );
            } else if kind.is_file() {
                result.insert(path, fs::read(entry.path()).unwrap());
            } else {
                result.insert(path, Vec::new());
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, Path::new(""), &mut result);
    result
}

#[test]
fn cache_only_reference_edits_remain_visible_with_an_unchanged_copy() {
    let fixture = Fixture::new("opencode", "copy");
    assert_ne!(
        fs::read(fixture.options.library_dir.join("example/SKILL.md")).unwrap(),
        fs::read(fixture.cached.join("SKILL.md")).unwrap(),
        "the test must exercise a target transformation"
    );
    let clean = fixture.row();
    assert_eq!(clean.render_status, Some(RenderStatus::InSync));
    assert!(clean.render_drift.is_empty());
    fs::write(
        fixture.cached.join("references/lesson.md"),
        "Edited only in render\n",
    )
    .unwrap();
    let before = snapshot(fixture.root.path());
    let row = fixture.row();
    assert_eq!(row.status, StatusKind::InSync);
    assert_eq!(row.render_status, Some(RenderStatus::Drift));
    assert_eq!(row.render_drift.len(), 1);
    assert_eq!(row.render_drift[0].path, "references/lesson.md");
    assert_ne!(
        row.render_drift[0].library_hash,
        row.render_drift[0].render_hash
    );
    assert_eq!(
        snapshot(fixture.root.path()),
        before,
        "status must never alter SSOT, base, cache, or install"
    );
}

#[test]
fn added_and_removed_render_paths_are_reported_in_order() {
    let fixture = Fixture::new("claude", "copy");
    fs::remove_file(fixture.cached.join("references/lesson.md")).unwrap();
    fs::write(fixture.cached.join("extra.md"), "Render-only knowledge\n").unwrap();
    let row = fixture.row();
    assert_eq!(
        row.render_drift
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["extra.md", "references/lesson.md"]
    );
    assert!(row.render_drift[0].library_hash.is_empty());
    assert!(!row.render_drift[0].render_hash.is_empty());
    assert!(!row.render_drift[1].library_hash.is_empty());
    assert!(row.render_drift[1].render_hash.is_empty());
}

#[test]
fn absent_render_cache_preserves_the_old_status_shape() {
    let fixture = Fixture::new("opencode", "copy");
    fs::remove_dir_all(&fixture.cached).unwrap();
    let row = fixture.row();
    assert_eq!(row.status, StatusKind::InSync);
    let encoded = serde_json::to_value(row).unwrap();
    for field in ["render_status", "render_drift", "render_error"] {
        assert!(encoded.get(field).is_none(), "unexpected {field}");
    }
}

#[cfg(any(unix, windows))]
fn link_directory(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    std::os::windows::fs::symlink_dir(target, link).unwrap();
}

#[test]
fn deeply_edited_cache_is_rejected_before_unbounded_recursion() {
    let fixture = Fixture::new("opencode", "copy");
    let mut path = fixture.cached.clone();
    for _ in 0..=symbrain_skills::MAX_RESOURCE_DEPTH {
        path = path.join("d");
        fs::create_dir(&path).unwrap();
    }
    let row = fixture.row();
    assert_eq!(row.status, StatusKind::InSync);
    assert_eq!(row.render_status, Some(RenderStatus::Unreadable));
    assert!(
        row.render_error
            .unwrap()
            .contains("maximum directory depth")
    );
}

#[cfg(any(unix, windows))]
#[test]
fn managed_hermes_links_are_healthy_until_the_render_is_edited() {
    let fixture = Fixture::new("hermes", "symlink");
    let row = fixture.row();
    assert_eq!(row.status, StatusKind::InSync);
    assert_eq!(row.mode.as_deref(), Some("linked"));
    assert_eq!(row.render_status, Some(RenderStatus::InSync));
    fs::write(
        fixture.installed.join("references/lesson.md"),
        "Edit through harness link\n",
    )
    .unwrap();
    let before = snapshot(fixture.root.path());
    let row = fixture.row();
    assert_eq!(row.status, StatusKind::HarnessChanged);
    assert_eq!(row.mode.as_deref(), Some("linked"));
    assert_eq!(row.render_status, Some(RenderStatus::Drift));
    assert_eq!(row.render_drift[0].path, "references/lesson.md");
    assert_eq!(snapshot(fixture.root.path()), before);
}

#[cfg(any(unix, windows))]
#[test]
fn cache_root_and_nested_links_do_not_expand_read_authority() {
    for nested in [false, true] {
        let fixture = Fixture::new("opencode", "copy");
        let outside = fixture.root.path().join("outside");
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.md"), "OUTSIDE_SECRET_SENTINEL").unwrap();
        let link = if nested {
            fixture.cached.join("references")
        } else {
            fixture.cached.clone()
        };
        fs::remove_dir_all(&link).unwrap();
        link_directory(&outside, &link);
        let row = fixture.row();
        assert_eq!(row.status, StatusKind::InSync);
        assert_eq!(row.render_status, Some(RenderStatus::Unreadable));
        assert!(row.render_error.is_some());
        assert!(row.render_drift.is_empty());
        assert!(
            !serde_json::to_string(&row)
                .unwrap()
                .contains("OUTSIDE_SECRET_SENTINEL")
        );
    }
}

#[cfg(unix)]
#[test]
fn render_special_files_are_rejected_without_opening_them() {
    let fixture = Fixture::new("opencode", "copy");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(fixture.cached.join("blocked"))
            .status()
            .unwrap()
            .success()
    );
    let start = std::time::Instant::now();
    let row = fixture.row();
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
    assert_eq!(row.render_status, Some(RenderStatus::Unreadable));
    assert!(row.render_error.unwrap().contains("special file"));
}

#[cfg(any(unix, windows))]
#[test]
fn unreadable_linked_render_keeps_both_comparison_diagnostics() {
    for kind in ["nested-link", "depth", "fifo"] {
        if kind == "fifo" && !cfg!(unix) {
            continue;
        }
        let fixture = Fixture::new("hermes", "symlink");
        match kind {
            "nested-link" => {
                let outside = fixture.root.path().join("outside");
                fs::create_dir(&outside).unwrap();
                fs::write(outside.join("secret.md"), "OUTSIDE_SECRET_SENTINEL").unwrap();
                link_directory(&outside, &fixture.cached.join("outside-link"));
            }
            "depth" => {
                let mut path = fixture.cached.clone();
                for _ in 0..=symbrain_skills::MAX_RESOURCE_DEPTH {
                    path = path.join("d");
                    fs::create_dir(&path).unwrap();
                }
            }
            "fifo" => assert!(
                std::process::Command::new("mkfifo")
                    .arg(fixture.cached.join("blocked"))
                    .status()
                    .unwrap()
                    .success()
            ),
            _ => unreachable!(),
        }
        let before = snapshot(fixture.root.path());
        let row = fixture.row();
        assert_eq!(row.status, StatusKind::Stale, "{kind}");
        assert_eq!(row.render_status, Some(RenderStatus::Unreadable), "{kind}");
        assert_eq!(row.error, row.render_error, "{kind}");
        assert!(row.error.is_some(), "{kind}");
        assert_eq!(row.mode.as_deref(), Some("symlink"), "{kind}");
        assert!(row.render_drift.is_empty(), "{kind}");
        assert!(
            !serde_json::to_string(&row)
                .unwrap()
                .contains("OUTSIDE_SECRET_SENTINEL")
        );
        assert_eq!(
            snapshot(fixture.root.path()),
            before,
            "{kind}: status wrote state"
        );
    }
}
