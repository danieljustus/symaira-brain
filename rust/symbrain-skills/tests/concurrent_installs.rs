use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use symbrain_skills::install::{
    InstallOptions, StatusKind, StatusOptions, install_path_for, read_events, status,
};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

const TARGETS: &[&str] = &[
    "antigravity",
    "claude",
    "codex",
    "hermes",
    "openclaw",
    "opencode",
];
const WORKER_ROOT: &str = "SYMBRAIN_CONCURRENT_INSTALL_ROOT";
const WORKER_SKILL: &str = "SYMBRAIN_CONCURRENT_INSTALL_SKILL";
const WORKER_MODE: &str = "SYMBRAIN_CONCURRENT_INSTALL_MODE";

fn worker_environment() -> Option<(PathBuf, String, String)> {
    Some((
        PathBuf::from(std::env::var_os(WORKER_ROOT)?),
        std::env::var(WORKER_SKILL).ok()?,
        std::env::var(WORKER_MODE).ok()?,
    ))
}

#[test]
fn worker_process() {
    let Some((root, skill, mode)) = worker_environment() else {
        return;
    };
    let barrier = root.join("barrier");
    fs::create_dir_all(&barrier).expect("barrier directory");
    fs::write(barrier.join(format!("{skill}.ready")), b"ready").expect("ready marker");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !barrier.join("release").is_file() {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for readiness barrier"
        );
        thread::sleep(Duration::from_millis(5));
    }

    let library = root.join("library");
    let source = library.join(&skill);
    let bundle = load_bundle(&source).expect("load fixture skill");
    let home = root.join("home");
    let events = root.join("events/install.jsonl");
    let render_dir = root.join("rendered");
    for target in TARGETS {
        let rendered = render_target(&bundle, target, &RenderMetadata::default())
            .expect("render fixture skill");
        let result = symbrain_skills::install::install_rendered(
            &bundle,
            &rendered,
            &InstallOptions {
                home_dir: home.clone(),
                render_dir: Some(render_dir.clone()),
                mode: mode.clone(),
                events_path: Some(events.clone()),
                ..Default::default()
            },
        )
        .unwrap_or_else(|error| panic!("install {skill} for {target}: {error}"));
        assert_eq!(result.action, "installed");
        assert_eq!(result.name, skill);
        assert!(
            result.path.is_dir(),
            "installed path missing: {}",
            result.path.display()
        );
    }
}

#[test]
fn concurrent_process_installs_preserve_each_target_path_and_record() {
    run_concurrent_install("copy");
}

#[test]
fn absent_uninstall_does_not_initialize_install_roots() {
    let temp = tempfile::tempdir().expect("test root");
    let root = fs::canonicalize(temp.path()).expect("canonical test root");
    let options = InstallOptions {
        home_dir: root.join("absent-home"),
        ..Default::default()
    };
    assert!(
        !symbrain_skills::install::uninstall("opencode", "missing", &options)
            .expect("absent uninstall is a no-op")
    );
    assert_eq!(fs::read_dir(&root).expect("root entries").count(), 0);
}

#[cfg(unix)]
#[test]
fn concurrent_process_symlink_installs_preserve_each_target_path_and_record() {
    run_concurrent_install("symlink");
}

#[allow(clippy::too_many_lines)]
fn run_concurrent_install(mode: &str) {
    let temp = tempfile::tempdir().expect("test root");
    let root = fs::canonicalize(temp.path()).expect("canonical test root");
    let library = root.join("library");
    fs::create_dir_all(&library).expect("library");
    for (name, content) in [
        ("concurrent-alpha", "alpha fixture body"),
        ("concurrent-beta", "beta fixture body"),
    ] {
        let skill = library.join(name);
        fs::create_dir_all(&skill).expect("fixture skill");
        fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: concurrent fixture\n---\n{content}\n"),
        )
        .expect("fixture source");
    }

    let executable = std::env::current_exe().expect("integration-test executable");
    let mut children = Vec::new();
    for skill in ["concurrent-alpha", "concurrent-beta"] {
        let child = Command::new(&executable)
            .arg("--exact")
            .arg("worker_process")
            .arg("--nocapture")
            .env(WORKER_ROOT, &root)
            .env(WORKER_SKILL, skill)
            .env(WORKER_MODE, mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start independent installer process");
        children.push((skill, child));
    }

    let barrier = root.join("barrier");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !["concurrent-alpha", "concurrent-beta"]
        .iter()
        .all(|skill| barrier.join(format!("{skill}.ready")).is_file())
    {
        assert!(
            Instant::now() < deadline,
            "installer readiness barrier timed out"
        );
        thread::sleep(Duration::from_millis(5));
    }
    fs::write(barrier.join("release"), b"go").expect("release installers together");

    for (skill, child) in children {
        let output = child.wait_with_output().expect("wait for installer");
        assert!(
            output.status.success(),
            "{skill} installer failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let home = root.join("home");
    let mut statuses = status(&StatusOptions {
        home_dir: home.clone(),
        library_dir: library,
        targets: TARGETS.iter().map(|target| (*target).to_owned()).collect(),
        ..Default::default()
    })
    .expect("status scan");
    statuses.sort_by(|left, right| {
        left.target
            .cmp(&right.target)
            .then_with(|| left.name.cmp(&right.name))
    });
    assert_eq!(statuses.len(), TARGETS.len() * 2, "all status rows survive");
    for target in TARGETS {
        for (name, content) in [
            ("concurrent-alpha", "alpha fixture body"),
            ("concurrent-beta", "beta fixture body"),
        ] {
            let expected_path = install_path_for(target, &home, None, "user", name)
                .expect("resolve installed path");
            let row = statuses
                .iter()
                .find(|row| row.target == *target && row.name == name)
                .unwrap_or_else(|| panic!("missing status row for {target}/{name}"));
            assert_eq!(row.path, expected_path);
            assert_eq!(row.status, StatusKind::InSync, "{target}/{name}");
            let installed = fs::read_to_string(row.path.join("SKILL.md"))
                .unwrap_or_else(|error| panic!("read {}: {error}", row.path.display()));
            assert!(
                installed.contains(content),
                "wrong content in {target}/{name}"
            );
        }
    }

    let events = read_events(&root.join("events/install.jsonl"), None, None)
        .expect("read shared install event log");
    assert_eq!(
        events.len(),
        TARGETS.len() * 2,
        "all install records survive"
    );
    for target in TARGETS {
        for name in ["concurrent-alpha", "concurrent-beta"] {
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.event == "install"
                        && event.target == *target
                        && event.skill == name
                        && event.outcome == "ok")
                    .count(),
                1,
                "exactly one successful install event for {target}/{name}"
            );
        }
    }
}
