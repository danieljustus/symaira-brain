//! CLI byte checks for the native `skills list` empty-library slice.

use std::process::{Command, Output};

use tempfile::TempDir;

fn command(root: &TempDir, args: &[&str]) -> Command {
    let home = root.path().join("home");
    let config = root.path().join("config");
    let data = root.path().join("data");
    let cache = root.path().join("cache");
    let project = root.path().join("project");
    for path in [&home, &config, &data, &cache, &project] {
        std::fs::create_dir_all(path).unwrap();
    }
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", &config)
        .env("XDG_DATA_HOME", &data)
        .env("XDG_CACHE_HOME", &cache)
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .env("TZ", "UTC")
        .current_dir(project)
        .args(args);
    command
}

fn run(root: &TempDir, args: &[&str]) -> Output {
    command(root, args).output().unwrap()
}

#[test]
fn empty_library_table_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "list"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"No skills in the library.\n");
}

#[test]
fn empty_library_json_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "list", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    // Go renders this with `json.Encoder`: compact, no trailing spaces.
    assert_eq!(
        output.stdout,
        b"{\"skills\":[],\"category_counts\":{},\"issues\":[]}\n"
    );
}

#[test]
fn populated_library_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let skill = root.path().join("data/symbrain/skills/library/demo");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(
        skill.join("SKILL.md"),
        b"---\nname: demo\ndescription: fixture\nlicense: Apache-2.0\n---\n\n# demo\n",
    )
    .unwrap();
    // The native slice only owns the empty-library report: a populated library
    // needs the Go metadata contract (created/modified times, per-target
    // installs, last-used and the four-column table).
    let output = run(&root, &["skills", "list", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn empty_library_with_extra_flag_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "list", "--bogus"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn sync_dry_run_json_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "sync", "--dry-run", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"{\"results\":[],\"dry_run\":true}\n");
}
