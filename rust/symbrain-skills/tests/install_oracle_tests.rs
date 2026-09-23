use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[path = "common/mod.rs"]
mod common;

use serde::Deserialize;
use sha2::{Digest, Sha256};
use symbrain_skills::install::{InstallOptions, StatusOptions, install_rendered, status};
use symbrain_skills::{RenderMetadata, load_bundle, materialize, render_target};

#[derive(Debug, Deserialize)]
struct OracleFixture {
    schema_version: u32,
    generator_sha256: String,
    go_sources: BTreeMap<String, String>,
    cases: Vec<OracleCase>,
}

#[derive(Debug, Deserialize)]
struct OracleCase {
    id: String,
    body: String,
    mutation: Option<String>,
    expected: OracleExpected,
}

#[derive(Debug, Deserialize)]
struct OracleExpected {
    action: Option<String>,
    mode: Option<String>,
    statuses: Vec<serde_json::Value>,
    artifacts: Vec<Artifact>,
}

#[derive(Debug, Deserialize, PartialEq, Eq)]
struct Artifact {
    path: String,
    #[serde(rename = "type")]
    kind: String,
    mode: u32,
    #[serde(default)]
    bytes: Option<String>,
}

#[test]
fn go_install_status_fixture_matches_rust_statuses_and_artifacts() {
    let fixture: OracleFixture =
        serde_json::from_slice(&common::skills_install_oracle()).expect("install oracle fixture");
    check_provenance(&fixture);
    assert_eq!(fixture.cases.len(), 11);
    for case in fixture.cases {
        let root = tempfile::tempdir().expect("case root");
        let home = root.path().join("home");
        let library = root.path().join("library");
        let base = root.path().join("custom-base");
        let source = library.join("oracle-skill");
        write_oracle_skill(&source, &case.body);
        let dry_run = case.id == "dry_run";
        let should_install = !matches!(
            case.mutation.as_deref(),
            Some("unmanaged" | "malformed" | "identity")
        );
        let mut action = None;
        let mut mode = None;
        if should_install {
            let bundle = load_bundle(&source).expect("bundle");
            let rendered =
                render_target(&bundle, "opencode", &RenderMetadata::default()).expect("render");
            let result = install_rendered(
                &bundle,
                &rendered,
                &InstallOptions {
                    home_dir: home.clone(),
                    base_dir: Some(base.clone()),
                    mode: "copy".to_owned(),
                    dry_run,
                    ..Default::default()
                },
            )
            .expect("install");
            action = Some(result.action);
            mode = Some(result.mode);
        }
        let destination = home.join(".config/opencode/skills/oracle-skill");
        apply_mutation(case.mutation.as_deref(), &source, &destination, root.path());
        assert_eq!(action, case.expected.action, "action case {}", case.id);
        assert_eq!(mode, case.expected.mode, "mode case {}", case.id);
        let rows = status(&StatusOptions {
            home_dir: home.clone(),
            library_dir: library,
            base_dir: Some(base.clone()),
            ..Default::default()
        })
        .expect("status");
        let got_statuses = rows
            .iter()
            .map(|row| {
                let mut value = serde_json::to_value(row).expect("status JSON");
                normalize_value(&mut value, root.path());
                value
            })
            .collect::<Vec<_>>();
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut expected_statuses = case.expected.statuses;
        #[cfg(windows)]
        {
            for status in &mut expected_statuses {
                normalize_windows_source_hashes(status);
            }
            let mut got_statuses = got_statuses;
            for status in &mut got_statuses {
                normalize_windows_source_hashes(status);
            }
            assert_eq!(got_statuses, expected_statuses, "status case {}", case.id);
        }
        #[cfg(not(windows))]
        assert_eq!(got_statuses, expected_statuses, "status case {}", case.id);
        let got_artifacts = artifacts(root.path(), &[home, base]);
        #[cfg_attr(not(windows), allow(unused_mut))]
        let mut expected_artifacts = case.expected.artifacts;
        #[cfg(windows)]
        for artifact in &mut expected_artifacts {
            if artifact.path.ends_with("/.symskills.json")
                && let Some(bytes) = &artifact.bytes
            {
                let decoded = decode_base64(bytes);
                artifact.bytes = Some(base64(&normalize_marker_bytes(&decoded, root.path())));
            }
        }
        assert_eq!(
            got_artifacts, expected_artifacts,
            "artifacts case {}",
            case.id
        );
    }
}

fn check_provenance(fixture: &OracleFixture) {
    assert_eq!(fixture.schema_version, 1);
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(
        fixture.generator_sha256,
        sha256(&repo.join("scripts/skills-install-oracle/main.go")),
        "Go install-oracle generator changed"
    );
    let expected_sources = BTreeSet::from([
        "internal/skills/install/install.go",
        "internal/skills/install/status.go",
        "internal/skills/install/base.go",
        "internal/skills/install/classify.go",
        "internal/skills/render/render_target.go",
    ]);
    assert_eq!(
        fixture
            .go_sources
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        expected_sources
    );
    for (source, expected) in &fixture.go_sources {
        assert_eq!(
            sha256(&repo.join(source)),
            *expected,
            "Go install/status source changed: {source}"
        );
    }
}

fn sha256(path: &Path) -> String {
    let bytes = fs::read(path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    format!("{:x}", Sha256::digest(bytes))
}

fn write_oracle_skill(root: &Path, body: &str) {
    fs::create_dir_all(root).expect("skill root");
    fs::write(
        root.join("SKILL.md"),
        format!("---\nname: oracle-skill\ndescription: oracle fixture\n---\n{body}"),
    )
    .expect("skill source");
    fs::write(root.join("reference.txt"), b"reference\n").expect("resource");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            root.join("reference.txt"),
            fs::Permissions::from_mode(0o755),
        )
        .expect("resource mode");
    }
}

fn apply_mutation(kind: Option<&str>, source: &Path, destination: &Path, root: &Path) {
    match kind {
        Some("unmanaged") => {
            fs::create_dir_all(destination).expect("unmanaged destination");
            fs::write(destination.join(".symskills.json"), b"oracle").expect("unmanaged marker");
        }
        Some("malformed") => {
            fs::create_dir_all(destination).expect("malformed destination");
            fs::write(destination.join(".symskills.json"), b"{bad").expect("malformed marker");
        }
        Some("identity") => {
            fs::create_dir_all(destination).expect("identity destination");
            fs::write(
                destination.join(".symskills.json"),
                br#"{"schema_version":1,"managed_by":"other","target":"opencode","name":"oracle-skill","mode":"copy"}"#,
            )
            .expect("identity marker");
        }
        Some("orphaned") => fs::remove_dir_all(source).expect("orphan source"),
        Some("stale") => write_oracle_skill(source, "body-v2\n"),
        Some("harness") => append_file(&destination.join("SKILL.md"), "harness-edit\n"),
        Some("conflict") => {
            write_oracle_skill(source, "body-v2\n");
            append_file(&destination.join("SKILL.md"), "harness-edit\n");
        }
        Some("added-deleted") => {
            fs::remove_file(destination.join("reference.txt")).expect("deleted file");
            fs::write(destination.join("harness.txt"), b"harness\n").expect("added file");
        }
        Some("converged") => {
            let marker = fs::read(destination.join(".symskills.json")).expect("converged marker");
            write_oracle_skill(source, "body-v2\n");
            let bundle = load_bundle(source).expect("converged bundle");
            let rendered = render_target(&bundle, "opencode", &RenderMetadata::default())
                .expect("converged render");
            let stage = tempfile::tempdir().expect("converged stage");
            let materialized = materialize(&bundle, &rendered, stage.path()).expect("materialize");
            copy_tree(&materialized.root, destination);
            fs::write(destination.join(".symskills.json"), marker).expect("restore marker");
        }
        None => {}
        Some(other) => panic!("unknown oracle mutation {other} at {}", root.display()),
    }
}

fn append_file(path: &Path, suffix: &str) {
    let mut bytes = fs::read(path).expect("mutation input");
    bytes.extend_from_slice(suffix.as_bytes());
    fs::write(path, bytes).expect("mutation output");
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("copy destination");
    for entry in fs::read_dir(source).expect("copy source") {
        let entry = entry.expect("copy entry");
        let from = entry.path();
        let to = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&from).expect("copy metadata");
        if metadata.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).expect("copy file");
        }
    }
}

fn artifacts(case_root: &Path, roots: &[PathBuf]) -> Vec<Artifact> {
    let mut output = Vec::new();
    for root in roots {
        if root.exists() {
            collect_artifacts(case_root, root, &mut output);
        }
    }
    output.sort_by(|left, right| left.path.cmp(&right.path));
    output
}

fn collect_artifacts(case_root: &Path, root: &Path, output: &mut Vec<Artifact>) {
    for entry in fs::read_dir(root).expect("artifact directory") {
        let entry = entry.expect("artifact entry");
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).expect("artifact metadata");
        let relative = path
            .strip_prefix(case_root)
            .expect("artifact relative")
            .to_string_lossy()
            .replace('\\', "/");
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".symskills-lock-"))
        {
            continue;
        }
        let (kind, bytes) = if metadata.is_dir() {
            ("dir", None)
        } else if metadata.is_file() {
            let raw = fs::read(&path).expect("artifact bytes");
            let normalized = if path
                .file_name()
                .is_some_and(|name| name == ".symskills.json")
            {
                normalize_marker_bytes(&raw, case_root)
            } else {
                raw
            };
            ("file", Some(base64(&normalized)))
        } else {
            ("other", None)
        };
        output.push(Artifact {
            path: format!("<root>/{relative}"),
            kind: kind.to_owned(),
            mode: file_mode(&metadata),
            bytes,
        });
        if metadata.is_dir() {
            collect_artifacts(case_root, &path, output);
        }
    }
}

fn normalize_value(value: &mut serde_json::Value, root: &Path) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                if key == "path" || key == "error" {
                    if let serde_json::Value::String(text) = value {
                        *text = text
                            .replace('\\', "/")
                            .replace(&root.to_string_lossy().replace('\\', "/"), "<root>");
                    }
                } else if key == "installed_at" {
                    *value = serde_json::Value::String("<timestamp>".to_owned());
                } else {
                    normalize_value(value, root);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_value(value, root);
            }
        }
        _ => {}
    }
}

#[cfg(windows)]
fn normalize_windows_source_hashes(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                if key == "source_hash" {
                    if let serde_json::Value::String(hash) = value {
                        assert!(
                            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                            "source hash must remain a SHA-256 digest: {hash}"
                        );
                        *value = serde_json::Value::String("<host-source-hash>".to_owned());
                    }
                } else {
                    normalize_windows_source_hashes(value);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_windows_source_hashes(value);
            }
        }
        _ => {}
    }
}

fn normalize_marker_bytes(bytes: &[u8], root: &Path) -> Vec<u8> {
    let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return bytes.to_vec();
    };
    normalize_marker_value(&mut value, root);
    let normalized = serde_json::to_vec_pretty(&value).expect("normalized marker");
    // Go's encoding/json escapes HTML-significant delimiters in strings.
    let normalized = String::from_utf8(normalized)
        .expect("normalized marker UTF-8")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e");
    let mut normalized = normalized.into_bytes();
    normalized.push(b'\n');
    normalized
}

fn normalize_marker_value(value: &mut serde_json::Value, root: &Path) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object.iter_mut() {
                if key == "rendered_at" {
                    *value = serde_json::Value::String("<rendered>".to_owned());
                } else if key == "installed" {
                    *value = serde_json::Value::String("<timestamp>".to_owned());
                } else if cfg!(windows) && key == "source_hash" {
                    if let serde_json::Value::String(hash) = value {
                        assert!(
                            hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()),
                            "source hash must remain a SHA-256 digest: {hash}"
                        );
                    }
                    *value = serde_json::Value::String("<host-source-hash>".to_owned());
                } else {
                    normalize_marker_value(value, root);
                }
            }
        }
        serde_json::Value::String(text) => {
            *text = text.replace(&root.to_string_lossy().replace('\\', "/"), "<root>");
        }
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_marker_value(value, root);
            }
        }
        _ => {}
    }
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        output.push(ALPHABET[(first >> 2) as usize] as char);
        output.push(ALPHABET[((first & 0x03) << 4 | second >> 4) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[((second & 0x0f) << 2 | third >> 6) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(third & 0x3f) as usize] as char
        } else {
            '='
        });
    }
    output
}

#[cfg(windows)]
fn decode_base64(input: &str) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut accumulator = 0_u32;
    let mut bits = 0_u8;
    for byte in input.bytes().filter(|byte| *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("invalid base64 fixture byte {byte}"),
        };
        accumulator = (accumulator << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(((accumulator >> bits) & 0xff) as u8);
            accumulator &= (1_u32 << bits).wrapping_sub(1);
        }
    }
    output
}

fn file_mode(metadata: &fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o777
    }
    #[cfg(windows)]
    {
        if metadata.is_dir() { 0o777 } else { 0o666 }
    }
    #[cfg(not(any(unix, windows)))]
    {
        if metadata.is_dir() { 0o755 } else { 0o644 }
    }
}
