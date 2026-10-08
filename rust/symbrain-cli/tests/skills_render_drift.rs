//! Exercise the requested reports through the actual native CLI with no Go.
use std::fs;
use std::process::Command;

use symbrain_skills::install::{InstallOptions, install_rendered};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

#[cfg(any(unix, windows))]
#[path = "support/skills_status_cache.rs"]
mod skills_status_cache;

#[test]
fn explicit_target_reports_render_drift_without_a_go_fallback() {
    let oracle = std::env::var_os("SYMBRAIN_SKILLS_STATUS_GO_ORACLE");
    if std::env::var_os("SYMBRAIN_SKILLS_STATUS_REQUIRE_ORACLE").is_some() {
        assert!(
            oracle.is_some(),
            "live evidence requires the immutable Go binary"
        );
    }
    let mut observations = Vec::new();
    for target in symbrain_skills::default_targets() {
        let Fixture {
            temp,
            source,
            cache,
            mode,
            mut command,
        } = fixture(&target);
        command.args(["skills", "status", "--target", &target, "--json"]);
        let clean = command.output().unwrap();
        assert!(
            clean.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&clean.stderr)
        );
        assert!(clean.stderr.is_empty());
        let clean: serde_json::Value = serde_json::from_slice(&clean.stdout).unwrap();
        assert_eq!(clean["installs"][0]["status"], "in-sync", "{target}");
        assert_eq!(clean["installs"][0]["render_status"], "in-sync", "{target}");
        assert_eq!(
            clean["installs"][0]["mode"],
            if mode == "symlink" { "linked" } else { "copy" },
            "{target}"
        );
        if let Some(oracle) = &oracle {
            observations.push(compare_go(oracle, &command, &target, "clean", &clean));
        }
        fs::write(
            cache.join(&target).join("example/references/lesson.md"),
            "Edited render only\n",
        )
        .unwrap();
        let changed = command.output().unwrap();
        assert!(
            changed.status.success(),
            "{target}: {}",
            String::from_utf8_lossy(&changed.stderr)
        );
        assert!(changed.stderr.is_empty());
        let changed: serde_json::Value = serde_json::from_slice(&changed.stdout).unwrap();
        assert_eq!(changed["installs"][0]["render_status"], "drift", "{target}");
        assert_eq!(
            changed["installs"][0]["render_drift"][0]["path"], "references/lesson.md",
            "{target}"
        );
        if let Some(oracle) = &oracle {
            observations.push(compare_go(
                oracle,
                &command,
                &target,
                "render-edit",
                &changed,
            ));
        }
        assert_eq!(
            fs::read(source.join("references/lesson.md")).unwrap(),
            b"Original lesson\n"
        );
        // Rebuild the arguments so the same native report is visible in a table.
        let mut table = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        table.env_clear().envs(
            command
                .get_envs()
                .filter_map(|(key, value)| value.map(|value| (key, value))),
        );
        table
            .current_dir(temp.path())
            .args(["skills", "status", "--target", &target]);
        let table = table.output().unwrap();
        assert!(table.status.success());
        let table = String::from_utf8(table.stdout).unwrap();
        assert!(
            table.starts_with("TARGET\tSKILL\tSTATUS\tMODE\tPATH\tRENDER\n"),
            "{target}: {table}"
        );
        assert!(table.ends_with("\tdrift\n"));
    }
    write_observations(&observations);
}

struct Fixture {
    temp: tempfile::TempDir,
    source: std::path::PathBuf,
    cache: std::path::PathBuf,
    mode: &'static str,
    command: Command,
}

fn fixture(target: &str) -> Fixture {
    fixture_with_mode(target, if cfg!(unix) { "symlink" } else { "copy" })
}

fn fixture_with_mode(target: &str, mode: &'static str) -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let data = temp.path().join("data");
    let source = data.join("symskills/library/example");
    fs::create_dir_all(source.join("references")).unwrap();
    fs::write(
        source.join("SKILL.md"),
        "---\nname: example\ndescription: example\n---\nBody\n",
    )
    .unwrap();
    fs::write(source.join("references/lesson.md"), "Original lesson\n").unwrap();
    let bundle = load_bundle(&source).unwrap();
    let rendered = render_target(&bundle, target, &RenderMetadata::default()).unwrap();
    let cache = data.join("symskills/rendered");
    // Windows copy reports do not require developer-mode link privileges.
    // Native link behavior is covered in the skills crate's OS tests.
    install_rendered(
        &bundle,
        &rendered,
        &InstallOptions {
            home_dir: home.clone(),
            base_dir: Some(data.join("symskills/base")),
            render_dir: Some(cache.clone()),
            mode: mode.to_owned(),
            ..Default::default()
        },
    )
    .unwrap();
    if mode == "copy" {
        symbrain_skills::materialize(&bundle, &rendered, &cache).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", temp.path().join("cache"))
        .env("PATH", temp.path().join("empty-bin"))
        .current_dir(temp.path());
    for key in ["SystemRoot", "WINDIR", "LLVM_PROFILE_FILE"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    Fixture {
        temp,
        source,
        cache,
        mode,
        command,
    }
}

fn compare_go(
    oracle: &std::ffi::OsStr,
    command: &Command,
    target: &str,
    state: &str,
    rust: &serde_json::Value,
) -> serde_json::Value {
    let mut go = Command::new(oracle);
    go.env_clear()
        .envs(
            command
                .get_envs()
                .filter_map(|(key, value)| value.map(|value| (key, value))),
        )
        .current_dir(command.get_current_dir().unwrap())
        .args(command.get_args());
    let output = go.output().unwrap();
    assert!(
        output.status.success(),
        "Go {target}/{state}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "Go {target}/{state}");
    let go: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let mut baseline = rust.clone();
    for row in baseline["installs"].as_array_mut().unwrap() {
        let row = row.as_object_mut().unwrap();
        // These are the explicit #621 product additions, never parity claims.
        for field in ["render_status", "render_drift", "render_error"] {
            row.remove(field);
        }
        if row.get("mode").and_then(serde_json::Value::as_str) == Some("linked") {
            row.insert("mode".to_owned(), serde_json::json!("symlink"));
        }
    }
    assert_eq!(
        baseline, go,
        "existing Go report changed for {target}/{state}"
    );
    serde_json::json!({"target":target,"state":state,"go":go,"rust":rust,"existing_report_matches":true})
}

fn write_observations(observations: &[serde_json::Value]) {
    if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_STATUS_EVIDENCE") {
        assert_eq!(
            observations.len(),
            12,
            "no zero-case or partial live receipt"
        );
        fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "go_oracle_ref": "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c",
                "total": observations.len(),
                "cases": observations,
            }))
            .unwrap(),
        )
        .unwrap();
    }
}

#[cfg(any(unix, windows))]
#[test]
fn unreadable_real_render_links_preserve_protected_state_and_only_cache_comparisons() {
    for kind in ["nested-link", "depth", "fifo"] {
        if kind == "fifo" && !cfg!(unix) {
            continue;
        }
        let Fixture {
            temp,
            cache,
            mut command,
            ..
        } = fixture_with_mode("hermes", "symlink");
        let cached = cache.join("hermes/example");
        match kind {
            "nested-link" => {
                let outside = temp.path().join("outside");
                fs::create_dir(&outside).unwrap();
                fs::write(outside.join("secret.md"), "OUTSIDE_SECRET_SENTINEL").unwrap();
                #[cfg(unix)]
                std::os::unix::fs::symlink(&outside, cached.join("outside-link")).unwrap();
                #[cfg(windows)]
                std::os::windows::fs::symlink_dir(&outside, cached.join("outside-link")).unwrap();
            }
            "depth" => {
                let mut path = cached.clone();
                for _ in 0..=symbrain_skills::MAX_RESOURCE_DEPTH {
                    path = path.join("d");
                    fs::create_dir(&path).unwrap();
                }
            }
            "fifo" => assert!(
                Command::new("mkfifo")
                    .arg(cached.join("blocked"))
                    .status()
                    .unwrap()
                    .success()
            ),
            _ => unreachable!(),
        }
        let before = snapshot(temp.path());
        let diagnostic = match kind {
            "nested-link" => "contains symlink",
            "depth" => "maximum directory depth",
            "fifo" => "contains special file",
            _ => unreachable!(),
        };
        command.args(["skills", "status", "--target", "hermes"]);
        let table = command.output().unwrap();
        assert!(table.status.success(), "{kind}");
        assert!(table.stderr.is_empty(), "{kind}");
        let table = String::from_utf8(table.stdout).unwrap();
        assert!(
            table.starts_with("TARGET\tSKILL\tSTATUS\tMODE\tPATH\tRENDER\n"),
            "{kind}"
        );
        assert!(table.ends_with("\tunreadable\n"), "{kind}: {table}");
        let after_table = snapshot(temp.path());
        skills_status_cache::assert_only_cache_created(&before, &after_table, kind);
        command.arg("--json");
        let json = command.output().unwrap();
        assert!(json.status.success(), "{kind}");
        assert!(json.stderr.is_empty(), "{kind}");
        let report: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        let row = &report["installs"][0];
        assert_eq!(row["status"], "stale", "{kind}");
        assert_eq!(row["render_status"], "unreadable", "{kind}");
        assert_eq!(row["error"], row["render_error"], "{kind}");
        assert!(
            row["error"].as_str().unwrap().contains(diagnostic),
            "{kind}"
        );
        assert_eq!(row["mode"], "symlink", "{kind}");
        assert!(row.get("render_drift").is_none(), "{kind}");
        assert_eq!(report["summary"]["stale"], 1, "{kind}");
        assert!(!String::from_utf8_lossy(&json.stdout).contains("OUTSIDE_SECRET_SENTINEL"));
        assert_eq!(
            snapshot(temp.path()),
            after_table,
            "{kind}: hot-cache scan changed state"
        );
    }
}

#[test]
fn default_and_empty_comparison_cache_keep_status_api_read_only() {
    let Fixture {
        temp,
        source,
        cache,
        ..
    } = fixture_with_mode("hermes", "copy");
    let options = symbrain_skills::install::StatusOptions {
        home_dir: temp.path().join("home"),
        targets: vec!["hermes".to_owned()],
        library_dir: source.parent().unwrap().to_path_buf(),
        base_dir: Some(temp.path().join("data/symskills/base")),
        render_dir: Some(cache),
        ..Default::default()
    };
    let before = snapshot(temp.path());
    let default = symbrain_skills::install::status(&options).unwrap();
    assert_eq!(default.len(), 1);
    let empty =
        symbrain_skills::install::status_with_cache(&options, Some(std::path::Path::new("")))
            .unwrap();
    assert_eq!(default, empty);
    let sync = symbrain_skills::install::SyncOptions {
        library_dir: options.library_dir.clone(),
        home_dir: options.home_dir.clone(),
        base_dir: options.base_dir.clone(),
        render_dir: options.render_dir.clone(),
        targets: options.targets.clone(),
        dry_run: true,
        ..Default::default()
    };
    assert!(symbrain_skills::install::sync(&sync).unwrap().is_empty());
    assert!(
        symbrain_skills::install::sync_with_cache(&sync, Some(std::path::Path::new("")))
            .unwrap()
            .is_empty()
    );
    assert_eq!(snapshot(temp.path()), before);
}

fn snapshot(root: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(
        root: &std::path::Path,
        relative: &std::path::Path,
        output: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        for entry in fs::read_dir(root.join(relative)).unwrap() {
            let entry = entry.unwrap();
            let path = relative.join(entry.file_name());
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                output.insert(path.clone(), b"directory".to_vec());
                visit(root, &path, output);
            } else if kind.is_symlink() {
                output.insert(
                    path,
                    fs::read_link(entry.path())
                        .unwrap()
                        .as_os_str()
                        .as_encoded_bytes()
                        .to_vec(),
                );
            } else if kind.is_file() {
                output.insert(path, fs::read(entry.path()).unwrap());
            } else {
                output.insert(path, Vec::new());
            }
        }
    }
    let mut output = std::collections::BTreeMap::new();
    visit(root, std::path::Path::new(""), &mut output);
    output
}
