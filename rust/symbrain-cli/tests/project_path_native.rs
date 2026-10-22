//! Native replay of Go-recorded `--project` path shapes for `install`,
//! `uninstall` and `sync` (#769 slice 3).
//!
//! `fixtures/project_path_go_shapes.{darwin,linux}.json` are written by
//! `scripts/config-harness-oracle/replay.py --record-project-fixture` from the
//! real Go CLI at the pinned oracle revision: argv, the fixture layout, Go's
//! stdout/stderr/exit and the full filesystem it left behind. Only shapes that
//! do not name the fixture root are recorded. Every Rust process has an absent
//! Go fallback, and the filesystem effects must match Go's, without a Go
//! toolchain.
//!
//! The result is filesystem dependent: APFS refuses to create non-UTF-8 names
//! (`EILSEQ`), ext4 accepts them. Each host therefore replays the fixture
//! recorded by Go on the same OS (macOS: darwin, Linux: linux). Any other unix
//! target has no recorded oracle and the test fails closed.
#![cfg(unix)]

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;

#[cfg(target_os = "macos")]
const FIXTURE: &str = include_str!("fixtures/project_path_go_shapes.darwin.json");
#[cfg(target_os = "linux")]
const FIXTURE: &str = include_str!("fixtures/project_path_go_shapes.linux.json");
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
compile_error!(
    "project_path_native replays a Go-recorded fixture and has one only for macOS and Linux"
);
#[cfg(target_os = "macos")]
const OTHER_FIXTURE: &str = include_str!("fixtures/project_path_go_shapes.linux.json");
#[cfg(target_os = "linux")]
const OTHER_FIXTURE: &str = include_str!("fixtures/project_path_go_shapes.darwin.json");
/// Both OS recordings come from the same corpus: only effects may differ.
const MIN_CASES: usize = 300;
const SEED_MCP: &[u8] = b"{\"mcpServers\":{\"other\":{\"command\":\"other\"},\"symbrain\":{\"command\":\"symbrain\",\"args\":[\"mcp\",\"--profile\",\"p\"]}}}";
const SEED_INSTRUCTIONS: &[u8] = b"# Project notes\n\nkeep this line\n";
const BASE_DIRS: [&str; 6] = ["home", "config", "cache", "data", "state", "project"];

fn from_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).unwrap())
        .collect()
}

/// Replaces `.bak.YYYYMMDDTHHMMSSZ[.N]` with `.bak.<timestamp>` like the oracle.
fn normalize_backup_names(bytes: &[u8]) -> Vec<u8> {
    let marker = b".bak.";
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at..].starts_with(marker) {
            let digits = &bytes[at + marker.len()..];
            let stamp = digits.len() >= 16
                && digits[..8].iter().all(u8::is_ascii_digit)
                && digits[8] == b'T'
                && digits[9..15].iter().all(u8::is_ascii_digit)
                && digits[15] == b'Z';
            if stamp {
                let mut end = 16;
                if digits.get(end) == Some(&b'.') {
                    let mut probe = end + 1;
                    while digits.get(probe).is_some_and(u8::is_ascii_digit) {
                        probe += 1;
                    }
                    if probe > end + 1 {
                        end = probe;
                    }
                }
                out.extend_from_slice(b".bak.<timestamp>");
                at += marker.len() + end;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    out
}

fn build_layout(project: &Path, case: &Value) {
    let layout = case["layout"].as_array().unwrap();
    for op in layout {
        let kind = op[0].as_str().unwrap();
        let path = project.join(op[1].as_str().unwrap());
        match kind {
            "dir" => std::fs::create_dir_all(&path).unwrap(),
            "file" => {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, from_hex(op[2].as_str().unwrap())).unwrap();
            }
            "link" => {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                symlink(op[2].as_str().unwrap(), &path).unwrap();
            }
            "mode" => {}
            other => panic!("unknown layout op {other}"),
        }
    }
    if case["seeded"].as_bool().unwrap()
        && let Some(seed_dir) = case["seed_dir"].as_str()
    {
        let target = project.join(seed_dir);
        if target.is_dir() {
            std::fs::write(target.join(".mcp.json"), SEED_MCP).unwrap();
            std::fs::write(target.join("CLAUDE.md"), SEED_INSTRUCTIONS).unwrap();
            std::fs::write(target.join("AGENTS.md"), SEED_INSTRUCTIONS).unwrap();
        }
    }
    for op in layout {
        if op[0].as_str() == Some("mode") {
            let mode = u32::try_from(op[2].as_u64().unwrap()).unwrap();
            std::fs::set_permissions(
                project.join(op[1].as_str().unwrap()),
                std::fs::Permissions::from_mode(mode),
            )
            .unwrap();
        }
    }
}

/// The oracle's `get_fs_manifest`: kind, mode and content of every entry,
/// minus the plain base directories.
fn manifest(root: &Path) -> BTreeMap<Vec<u8>, (String, u32, String)> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<Vec<u8>, (String, u32, String)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            // Raw bytes: ext4 allows names that are not valid UTF-8.
            let rel =
                normalize_backup_names(path.strip_prefix(root).unwrap().as_os_str().as_bytes());
            let mode = metadata.mode() & 0o777;
            let file_type = metadata.file_type();
            if file_type.is_symlink() {
                let target = std::fs::read_link(&path).unwrap();
                let hex = hex(target.into_os_string().as_encoded_bytes());
                // A symlink's own mode is platform noise (0o755 on macOS,
                // always 0o777 on Linux); only its target is compared.
                out.insert(rel, ("symlink".into(), 0, hex));
            } else if file_type.is_dir() {
                if !BASE_DIRS
                    .iter()
                    .any(|base| base.as_bytes() == rel.as_slice())
                {
                    out.insert(rel, ("dir".into(), mode, String::new()));
                }
                walk(root, &path, out);
            } else if file_type.is_file() {
                let content = std::fs::read(&path)
                    .unwrap_or_else(|error| format!("<read error: {error}>").into_bytes());
                out.insert(rel, ("file".into(), mode, hex(&content)));
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

fn restore_modes(dir: &Path) {
    let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                restore_modes(&entry.path());
            }
        }
    }
}

#[test]
fn go_recorded_project_path_shapes_are_native_without_a_go_fallback() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(
        cases.len() >= MIN_CASES,
        "fixture shrank to {} cases",
        cases.len()
    );
    let other: Value = serde_json::from_str(OTHER_FIXTURE).unwrap();
    let other_cases = other["cases"].as_array().unwrap();
    assert!(
        other_cases.len() >= MIN_CASES,
        "other-OS fixture shrank to {} cases",
        other_cases.len()
    );
    assert_eq!(
        fixture["go_oracle_ref"], other["go_oracle_ref"],
        "fixtures must come from the same pinned oracle"
    );
    let names = |cases: &[Value]| -> Vec<String> {
        cases
            .iter()
            .map(|case| case["name"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        names(cases),
        names(other_cases),
        "darwin and linux fixtures must cover the same cases"
    );
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let root = TempDir::new().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        for dir in BASE_DIRS {
            std::fs::create_dir_all(root_path.join(dir)).unwrap();
        }
        build_layout(&root_path.join("project"), case);
        let args: Vec<OsString> = case["argv_hex"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| OsString::from_vec(from_hex(arg.as_str().unwrap())))
            .collect();
        let output = Command::new(env!("CARGO_BIN_EXE_symbrain"))
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", root_path.join("home"))
            .env("XDG_CONFIG_HOME", root_path.join("config"))
            .env("XDG_DATA_HOME", root_path.join("data"))
            .env("XDG_CACHE_HOME", root_path.join("cache"))
            .env("XDG_STATE_HOME", root_path.join("state"))
            .env("PATH", "/usr/bin:/bin")
            .env("SYMBRAIN_GO_BINARY", root_path.join("absent-go-fallback"))
            .env("LC_ALL", "C.UTF-8")
            .env("TZ", "UTC")
            .current_dir(root_path.join("project"))
            .args(&args)
            .output()
            .unwrap();
        let actual = manifest(&root_path);
        restore_modes(&root_path);
        assert_eq!(
            output.status.code(),
            Some(i32::try_from(case["exit"].as_i64().unwrap()).unwrap()),
            "{name}: exit"
        );
        assert_eq!(
            normalize_backup_names(&output.stdout),
            normalize_backup_names(&from_hex(case["stdout_hex"].as_str().unwrap())),
            "{name}: stdout"
        );
        assert_eq!(
            normalize_backup_names(&output.stderr),
            normalize_backup_names(&from_hex(case["stderr_hex"].as_str().unwrap())),
            "{name}: stderr"
        );
        let expected: BTreeMap<Vec<u8>, (String, u32, String)> = case["manifest"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| {
                let kind = entry[1].as_str().unwrap().to_owned();
                let mode = if kind == "symlink" {
                    0
                } else {
                    u32::try_from(entry[2].as_u64().unwrap()).unwrap()
                };
                // A fifth element carries the raw name when it is not UTF-8.
                let rel = entry.get(4).map_or_else(
                    || entry[0].as_str().unwrap().as_bytes().to_vec(),
                    |raw| from_hex(raw.as_str().unwrap()),
                );
                (rel, (kind, mode, entry[3].as_str().unwrap().to_owned()))
            })
            .collect();
        assert_eq!(actual, expected, "{name}: filesystem effects");
    }
}
