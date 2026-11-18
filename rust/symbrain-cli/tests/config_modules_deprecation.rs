//! `config set modules.*` keeps working but says the keys are deprecated.

#[path = "../../test-support/coverage.rs"]
mod coverage;

use std::process::{Command, Output};

fn symbrain(root: &std::path::Path, args: &[&str]) -> Output {
    std::fs::create_dir_all(root.join("home")).unwrap();
    Command::new(env!("CARGO_BIN_EXE_symbrain"))
        .args(args)
        .env_clear()
        // Keep the coverage profile destination; a cleared environment drops it.
        .envs(std::env::var_os("LLVM_PROFILE_FILE").map(|path| ("LLVM_PROFILE_FILE", path)))
        .envs(coverage::profile_environment())
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("PATH", "/usr/bin:/bin")
        .current_dir(root)
        .output()
        .unwrap()
}

#[test]
fn setting_a_module_key_stores_it_and_notes_the_deprecation_on_stderr() {
    let root = tempfile::tempdir().unwrap();
    for module in ["browse", "operate", "scope"] {
        let key = format!("modules.{module}");
        let set = symbrain(root.path(), &["config", "set", &key, "true"]);
        assert!(set.status.success(), "{key}: {:?}", set.stderr);
        let stderr = String::from_utf8_lossy(&set.stderr);
        assert_eq!(stderr.lines().count(), 1, "{key}: {stderr}");
        assert!(
            stderr.contains(&format!("{key} is deprecated"))
                && stderr.contains("ship with symbrain"),
            "{key}: {stderr}"
        );
        let get = symbrain(root.path(), &["config", "get", &key]);
        assert_eq!(String::from_utf8_lossy(&get.stdout).trim(), "true", "{key}");
        assert!(get.stderr.is_empty(), "{key}: {:?}", get.stderr);
    }
}

#[test]
fn other_keys_stay_silent() {
    let root = tempfile::tempdir().unwrap();
    let other = symbrain(root.path(), &["config", "set", "patterns.enabled", "true"]);
    assert!(other.status.success());
    assert!(other.stderr.is_empty(), "{:?}", other.stderr);
}
