//! Exercise the requested reports through the actual native CLI with no Go.
use std::fs;
use std::process::Command;

use symbrain_skills::install::{InstallOptions, install_rendered};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

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
    let mode = if cfg!(unix) { "symlink" } else { "copy" };
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
