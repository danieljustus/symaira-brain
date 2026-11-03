//! `symbrain skills` never delegates to Go (#764): scenarios that used to fall
//! back replay the recorded real-Go bytes (`skills_native_go_oracle.json`,
//! recorded from the Go oracle built from this tree, one isolated HOME/XDG root
//! per scenario) and require identical stdout, stderr and exit code.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use regex::Regex;
use serde_json::Value;
use tempfile::TempDir;

const FIXTURE: &str = include_str!("fixtures/skills_native_go_oracle.json");

fn write(root: &Path, relative: &str, data: &[u8]) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, data).unwrap();
}

fn setup(name: &str, root: &Path) {
    match name {
        "status_root_is_file" | "sync_root_is_file" => {
            write(root, "home/.claude/skills", b"iamafile");
        }
        "status_root_mode0" => {
            let skills = root.join("home/.claude/skills");
            std::fs::create_dir_all(&skills).unwrap();
            std::fs::set_permissions(&skills, std::fs::Permissions::from_mode(0)).unwrap();
        }
        "status_opencode_dangling_root" => {
            std::fs::create_dir_all(root.join("home/.config/opencode")).unwrap();
            std::os::unix::fs::symlink(
                root.join("nowhere"),
                root.join("home/.config/opencode/skills"),
            )
            .unwrap();
        }
        "status_root_symlink_loop" => {
            std::fs::create_dir_all(root.join("home/.claude")).unwrap();
            let link = root.join("home/.claude/skills");
            std::os::unix::fs::symlink(&link, &link).unwrap();
        }
        "log_malformed_lines" | "log_malformed_lines_text" => {
            let mut data = Vec::new();
            data.extend_from_slice(
                br#"{"ts":"2026-01-02T00:00:00Z","event":"install","skill":"a","target":"claude","outcome":"ok","actor":"cli"}"#,
            );
            data.extend_from_slice(b"\n{bad\n[1]\nnull\n\xff\xfe\n");
            data.extend_from_slice(
                br#"{"ts":"2026-01-04T00:00:00Z","ts":"2026-01-05T00:00:00Z","event":"dup","skill":"b"}"#,
            );
            data.extend_from_slice(b"\n");
            data.extend_from_slice(
                br#"{"TS":"2026-01-03T00:00:00Z","EVENT":"x","SKILL":"CaseSkill","outcome":"ok","actor":"cli"}"#,
            );
            data.extend_from_slice(b"\n");
            write(root, "home/.local/share/symskills/events.jsonl", &data);
        }
        "list_unclean_library_dir" => {
            write(
                root,
                "custom-lib/custom/SKILL.md",
                b"---\nname: custom\ndescription: d\n---\nbody\n",
            );
        }
        name if name.starts_with("lib_") => library_case(name, root),
        _ => {}
    }
}

const OK: &[u8] = b"---\nname: x\ndescription: y\n---\nb\n";

/// Mirrors `LIBCASES` of the recording script: one `good` skill plus one
/// damaged (or special) bundle `bad`.
fn library_case(name: &str, root: &Path) {
    let library = root.join("data/symbrain/skills/library");
    write(
        root,
        "data/symbrain/skills/library/good/SKILL.md",
        b"---\nname: good\ndescription: d\n---\ngood\n",
    );
    let bad = library.join("bad");
    if name != "lib_symlinked_dir_skipped" {
        std::fs::create_dir_all(&bad).unwrap();
    }
    let skill_md = bad.join("SKILL.md");
    match name {
        "lib_skill_md_is_dir" => std::fs::create_dir_all(&skill_md).unwrap(),
        "lib_skill_md_abs_symlink" => {
            std::fs::write(bad.join("real.md"), OK).unwrap();
            std::os::unix::fs::symlink(bad.join("real.md"), &skill_md).unwrap();
        }
        "lib_skill_md_dangling_symlink" => {
            std::os::unix::fs::symlink("nowhere", &skill_md).unwrap();
        }
        "lib_skill_md_symlink_loop" => {
            std::os::unix::fs::symlink("SKILL.md", &skill_md).unwrap();
        }
        "lib_skill_md_symlink_to_dir" => {
            std::fs::create_dir_all(bad.join("sub")).unwrap();
            std::os::unix::fs::symlink("sub", &skill_md).unwrap();
        }
        "lib_skill_md_mode0" => {
            std::fs::write(&skill_md, OK).unwrap();
            std::fs::set_permissions(&skill_md, std::fs::Permissions::from_mode(0)).unwrap();
        }
        "lib_bundle_mode0" => {
            std::fs::write(&skill_md, OK).unwrap();
            std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(0)).unwrap();
        }
        "lib_bad_utf8_frontmatter" => {
            std::fs::write(&skill_md, b"---\nname: x\ndescription: \xff\n---\n").unwrap();
        }
        "lib_duplicate_key" => {
            std::fs::write(&skill_md, "---\nname: x\nname: y\ndescription: y\n---\n").unwrap();
        }
        "lib_tab_indent" => {
            std::fs::write(&skill_md, "---\nname: x\n\tdescription: y\n---\n").unwrap();
        }
        "lib_unclosed_flow" => {
            std::fs::write(&skill_md, "---\nname: x\ndescription: [a\n---\n").unwrap();
        }
        "lib_scalar_frontmatter" => {
            std::fs::write(&skill_md, "---\njust a string\n---\n").unwrap();
        }
        "lib_float_version_text" => {
            std::fs::write(
                &skill_md,
                "---\nname: x\ndescription: y\nversion: 1.10\n---\n",
            )
            .unwrap();
        }
        "lib_symlinked_dir_skipped" => {
            write(root, "real-skill/SKILL.md", OK);
            std::os::unix::fs::symlink(root.join("real-skill"), library.join("linked")).unwrap();
        }
        _ => {}
    }
}

fn normalise(bytes: &[u8], root: &Path) -> String {
    let timestamp = Regex::new(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(\.\d+)?(Z|[+-]\d\d:\d\d)").unwrap();
    let text = String::from_utf8_lossy(bytes).replace(root.to_str().unwrap(), "$ROOT");
    let text = text.replace("/private$ROOT", "$ROOT");
    timestamp.replace_all(&text, "<TS>").into_owned()
}

fn restore_permissions(root: &Path) {
    for relative in [
        "home/.claude/skills",
        "data/symbrain/skills/library/bad",
        "data/symbrain/skills/library/bad/SKILL.md",
    ] {
        let path = root.join(relative);
        if path.exists() {
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
        }
    }
}

#[test]
fn previously_delegated_scenarios_match_recorded_go_bytes() {
    let fixture: BTreeMap<String, Value> = serde_json::from_str(FIXTURE).unwrap();
    assert!(fixture.len() >= 29, "fixture lost scenarios");
    for (name, case) in &fixture {
        let directory = TempDir::new().unwrap();
        let root = directory.path().canonicalize().unwrap();
        for sub in ["home", "config", "data", "cache", "project"] {
            std::fs::create_dir_all(root.join(sub)).unwrap();
        }
        setup(name, &root);
        let args: Vec<&str> = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
        command
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", root.join("home"))
            .env("USERPROFILE", root.join("home"))
            .env("XDG_CONFIG_HOME", root.join("config"))
            .env("XDG_DATA_HOME", root.join("data"))
            .env("XDG_CACHE_HOME", root.join("cache"))
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .current_dir(root.join("project"))
            .args(&args);
        for (key, value) in case["env"].as_object().unwrap() {
            command.env(
                key,
                value
                    .as_str()
                    .unwrap()
                    .replace("$ROOT", root.to_str().unwrap()),
            );
        }
        let output = command.output().unwrap();
        restore_permissions(&root);
        assert_eq!(
            output.status.code(),
            Some(i32::try_from(case["code"].as_i64().unwrap()).unwrap()),
            "{name}: exit code"
        );
        assert_eq!(
            normalise(&output.stdout, &root),
            case["out"].as_str().unwrap(),
            "{name}: stdout"
        );
        assert_eq!(
            normalise(&output.stderr, &root),
            case["err"].as_str().unwrap(),
            "{name}: stderr"
        );
    }
}
