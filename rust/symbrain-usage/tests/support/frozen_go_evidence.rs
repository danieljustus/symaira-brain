//! Replays frozen fresh-Go usage evidence (see `scripts/oracle-freeze/record_usage_evidence.py`).
//!
//! The legacy gates ran a Go test that wrote evidence JSON and an on-disk credential tree, then ran
//! the Rust test against both. A fixture now carries both: the JSON documents plus the recorded
//! filesystem tree, with the scratch roots replaced by `<ROOT>`-style tokens. [`restore`] rebuilds
//! the tree below a fresh temp directory and substitutes the live paths. No Go is involved.
#![allow(dead_code)]

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct Frozen {
    pub root: tempfile::TempDir,
    pub fixture: Value,
}

impl Frozen {
    pub fn documents(&self, name: &str) -> &Value {
        &self.fixture["documents"][name]
    }

    pub fn records(&self) -> Vec<Value> {
        self.documents("records")
            .as_array()
            .expect("frozen records")
            .clone()
    }

    pub fn dir(&self, token: &str) -> PathBuf {
        let name = token.trim_matches(|c| c == '<' || c == '>').to_lowercase();
        std::fs::canonicalize(self.root.path())
            .expect("temp root")
            .join(name)
    }
}

/// Rebuild the recorded trees and detokenize the whole fixture.
pub fn restore(text: &str) -> Frozen {
    let raw: Value = serde_json::from_str(text).expect("frozen Go evidence JSON");
    assert_eq!(raw["schema_version"], 1);
    let root = tempfile::tempdir().expect("evidence root");
    let base = std::fs::canonicalize(root.path()).expect("temp root");
    let mut live = BTreeMap::new();
    for token in raw["tokens"].as_array().expect("tokens") {
        let token = token.as_str().expect("token").to_owned();
        let name = token.trim_matches(|c| c == '<' || c == '>').to_lowercase();
        let dir = base.join(name);
        std::fs::create_dir_all(&dir).expect("token dir");
        live.insert(token, dir);
    }
    let mut serialized = serde_json::to_string(&raw).expect("serialize fixture");
    for (token, dir) in &live {
        let escaped = serde_json::to_string(dir.to_str().expect("utf-8 temp path")).unwrap();
        serialized = serialized.replace(token, &escaped[1..escaped.len() - 1]);
    }
    let fixture: Value = serde_json::from_str(&serialized).expect("detokenized fixture");
    // The tree keys were tokens too, so they are live paths after substitution.
    for dir in live.values() {
        if let Some(entries) = fixture["trees"]
            .get(dir.to_str().expect("utf-8 temp path"))
            .and_then(Value::as_array)
        {
            restore_tree(dir, entries);
        }
    }
    Frozen { root, fixture }
}

fn unhex(value: &str) -> Vec<u8> {
    (0..value.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&value[i..i + 2], 16).expect("hex"))
        .collect()
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("mode");
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: u32) {}

fn restore_tree(root: &Path, entries: &[Value]) {
    let mut dirs = Vec::new();
    for entry in entries {
        let path = root.join(entry["path"].as_str().expect("path"));
        let mode = u32::try_from(entry["mode"].as_u64().unwrap_or(0o644)).expect("mode");
        match entry["kind"].as_str().expect("kind") {
            "dir" => {
                std::fs::create_dir_all(&path).expect("dir");
                dirs.push((path, mode));
            }
            "file" => {
                std::fs::write(&path, unhex(entry["hex"].as_str().expect("hex"))).expect("file");
                set_mode(&path, mode);
            }
            "symlink" => {
                #[cfg(unix)]
                std::os::unix::fs::symlink(entry["target"].as_str().expect("target"), &path)
                    .expect("symlink");
            }
            other => panic!("unknown tree entry kind {other}"),
        }
    }
    // Directory modes last, children before parents, so restrictive modes cannot block restore.
    for (path, mode) in dirs.into_iter().rev() {
        set_mode(&path, mode);
    }
}

/// Compile the synthetic `symvault` helper once (the gates built it with `rustc` the same way) and
/// install it into `bin_dir` as `symvault` and `security`.
pub fn install_fake_symvault(bin_dir: &Path) {
    use std::process::Command;
    use std::sync::OnceLock;
    static IMAGE: OnceLock<Vec<u8>> = OnceLock::new();
    let image = IMAGE.get_or_init(|| {
        let temp = tempfile::tempdir().expect("fake symvault build directory");
        let source =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/support/fake_symvault.rs");
        let output = temp.path().join("fake-symvault");
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let result = Command::new(rustc)
            .arg("--edition=2024")
            .arg(source)
            .arg("-o")
            .arg(&output)
            .output()
            .expect("start rustc for fake symvault");
        assert!(
            result.status.success(),
            "compile fake symvault: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        std::fs::read(output).expect("compiled fake symvault")
    });
    std::fs::create_dir_all(bin_dir).expect("bin dir");
    for name in ["symvault", "security"] {
        let path = bin_dir.join(name);
        std::fs::write(&path, image).expect("fake executable");
        set_mode(&path, 0o755);
    }
}
