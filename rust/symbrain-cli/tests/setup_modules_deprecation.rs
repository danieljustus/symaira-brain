//! Browse, Operate and Scope ship with symbrain: `--modules` and the
//! `modules.*` config keys stay accepted but no longer select or download them.
#![cfg(target_os = "macos")]

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

struct Home {
    root: tempfile::TempDir,
}

impl Home {
    /// A HOME whose mandatory cores already match the pinned versions, so
    /// `setup --fix` never reaches the network.
    fn with_current_cores() -> Self {
        let root = tempfile::tempdir().unwrap();
        let bin_dir = root.path().join("home/.symaira/bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../symbrain-managed/assets/manifest.json"))
                .unwrap();
        for name in ["symcockpit", "symdesk", "symvault"] {
            let version = manifest["cores"][name]["version"]
                .as_str()
                .unwrap()
                .trim_start_matches('v');
            executable(
                &bin_dir.join(name),
                &format!("#!/bin/sh\nprintf '{{\"version\":\"{version}\"}}\\n'\n"),
            );
        }
        Self { root }
    }

    fn enable_browse_module(&self) {
        let config = self.root.path().join("config/symbrain");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.toml"), "[modules]\nbrowse = true\n").unwrap();
    }

    fn setup(&self, binary: &Path, args: &[&str]) -> Output {
        Command::new(binary)
            .args(args)
            .env_clear()
            .envs(coverage::profile_environment())
            .env("HOME", self.root.path().join("home"))
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_DATA_HOME", self.root.path().join("data"))
            .env("XDG_CACHE_HOME", self.root.path().join("cache"))
            .env("PATH", "/usr/bin:/bin")
            .current_dir(self.root.path())
            .output()
            .unwrap()
    }
}

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A copy of the real binary with a worker beside it, as in a release bundle.
fn bundle(root: &Path, workers: &[&str]) -> PathBuf {
    let dir = root.join("bundle");
    fs::create_dir_all(&dir).unwrap();
    let symbrain = dir.join("symbrain");
    fs::copy(env!("CARGO_BIN_EXE_symbrain"), &symbrain).unwrap();
    for worker in workers {
        executable(&dir.join(worker), "#!/bin/sh\nexit 0\n");
    }
    symbrain
}

#[test]
fn modules_flag_without_from_source_is_a_noted_no_op() {
    let home = Home::with_current_cores();
    let output = home.setup(
        Path::new(env!("CARGO_BIN_EXE_symbrain")),
        &["setup", "--fix", "--json", "--modules", "browse,operate"],
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert!(
        !stderr.contains("requires --from-source"),
        "the old usage error must be gone: {stderr}"
    );
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.contains("--modules is deprecated") && stderr.contains("ship with symbrain"),
        "{stderr}"
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["results"].as_array().unwrap().len(), 3);
}

#[test]
fn enabled_browse_module_is_not_downloaded_when_symbrowse_ships_beside_symbrain() {
    let home = Home::with_current_cores();
    home.enable_browse_module();
    let symbrain = bundle(home.root.path(), &["symbrowse"]);
    let output = home.setup(&symbrain, &["setup", "--fix", "--json"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let names: Vec<_> = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|result| result["name"].as_str().unwrap())
        .collect();
    assert!(!names.contains(&"symbrowse"), "{names:?}");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(
        stderr.contains("symbrowse ships with symbrain") && stderr.contains("modules.browse"),
        "{stderr}"
    );
}
