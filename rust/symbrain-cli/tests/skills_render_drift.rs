//! Exercise the requested reports through the actual native CLI with no Go.
//!
//! The Go reports are frozen in `fixtures/skills_status_go_oracle.json` (recorded from the
//! immutable Go binary; the per-run temp root is tokenised as `<TEMP>`). Re-recording is
//! historical tooling only: `SYMBRAIN_SKILLS_STATUS_RECORD_GO=<symbrain-go>` plus
//! `SYMBRAIN_SKILLS_STATUS_RECORD_OUT=<path>`; it is never part of a gate.
use std::fs;
use std::process::Command;

use symbrain_skills::install::{InstallOptions, install_rendered};
use symbrain_skills::{RenderMetadata, load_bundle, render_target};

#[test]
fn explicit_target_reports_render_drift_without_a_go_fallback() {
    let recorder = std::env::var_os("SYMBRAIN_SKILLS_STATUS_RECORD_GO");
    let frozen: serde_json::Value = serde_json::from_str(FROZEN).expect("frozen Go reports");
    assert_eq!(frozen["total"], 12);
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
        observations.push(compare_go(
            recorder.as_deref(),
            &frozen,
            &command,
            temp.path(),
            &target,
            "clean",
            &clean,
        ));
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
        observations.push(compare_go(
            recorder.as_deref(),
            &frozen,
            &command,
            temp.path(),
            &target,
            "render-edit",
            &changed,
        ));
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

const FROZEN: &str = include_str!("fixtures/skills_status_go_oracle.json");
const TEMP_TOKEN: &str = "<TEMP>";

fn tokenize(value: &serde_json::Value, temp: &std::path::Path) -> serde_json::Value {
    let mut text = serde_json::to_string(value).unwrap();
    let plain = temp.to_str().unwrap().to_owned();
    let canonical = fs::canonicalize(temp).unwrap().to_str().unwrap().to_owned();
    for path in [canonical, plain] {
        let escaped = serde_json::to_string(&path).unwrap();
        text = text.replace(&escaped[1..escaped.len() - 1], TEMP_TOKEN);
    }
    serde_json::from_str(&text).unwrap()
}

fn detokenize(value: &serde_json::Value, temp: &std::path::Path) -> serde_json::Value {
    let text = serde_json::to_string(value).unwrap();
    let path = serde_json::to_string(temp.to_str().unwrap()).unwrap();
    serde_json::from_str(&text.replace(TEMP_TOKEN, &path[1..path.len() - 1])).unwrap()
}

fn compare_go(
    recorder: Option<&std::ffi::OsStr>,
    frozen: &serde_json::Value,
    command: &Command,
    temp: &std::path::Path,
    target: &str,
    state: &str,
    rust: &serde_json::Value,
) -> serde_json::Value {
    if recorder.is_none() && cfg!(windows) {
        // The frozen Go reports were recorded on darwin (symlink installs); Windows reports use
        // copy installs whose Go bytes were never recorded, so no frozen oracle exists there.
        return serde_json::Value::Null;
    }
    let go = if let Some(oracle) = recorder {
        // Historical re-recording against a live Go binary.
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
        serde_json::from_slice(&output.stdout).unwrap()
    } else {
        let case = frozen["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["target"] == target && case["state"] == state)
            .unwrap_or_else(|| panic!("no frozen Go report for {target}/{state}"));
        detokenize(&case["go"], temp)
    };
    // `installed_at` is the per-invocation wall clock and is an accepted difference.
    let mut go = go;
    for row in go["installs"].as_array_mut().unwrap() {
        row["installed_at"] = serde_json::json!("<TS>");
    }
    let mut baseline = rust.clone();
    for row in baseline["installs"].as_array_mut().unwrap() {
        row.as_object_mut()
            .unwrap()
            .insert("installed_at".to_owned(), serde_json::json!("<TS>"));
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
    serde_json::json!({"target":target,"state":state,"go":tokenize(&go, temp),"rust":tokenize(rust, temp),"existing_report_matches":true})
}

fn write_observations(observations: &[serde_json::Value]) {
    if let Some(path) = std::env::var_os("SYMBRAIN_SKILLS_STATUS_RECORD_OUT") {
        assert_eq!(
            observations.len(),
            12,
            "no zero-case or partial live receipt"
        );
        fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "schema_version": 1,
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
fn unreadable_real_render_links_keep_json_and_table_reports_read_only() {
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
        assert_eq!(snapshot(temp.path()), before, "{kind}: status wrote state");
    }
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
