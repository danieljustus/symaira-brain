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

fn write_library_skill(root: &TempDir, name: &str, extra: &str) -> std::path::PathBuf {
    let directory = root.path().join("data/symbrain/skills/library").join(name);
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} skill\nlicense: Apache-2.0\n{extra}---\n\n# {name}\n"),
    )
    .unwrap();
    directory
}

fn list_json(root: &TempDir) -> serde_json::Value {
    let output = run(root, &["skills", "list", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn populated_library_is_reported_natively() {
    let root = TempDir::new().unwrap();
    let skill = write_library_skill(&root, "demo", "");
    let value = list_json(&root);
    let entry = &value["skills"][0];
    assert_eq!(entry["name"], "demo");
    assert_eq!(entry["description"], "demo skill");
    assert_eq!(entry["path"], skill.display().to_string());
    // Go's record fields: filesystem times, always an array, a null last_used.
    assert!(entry["created_at"].as_str().unwrap().ends_with('Z'));
    assert!(entry["modified_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(entry["installs"].as_array().unwrap().len(), 0);
    assert!(entry["last_used"].is_null());
    assert!(value["issues"].as_array().unwrap().is_empty());
}

#[test]
fn category_variants_collapse_to_one_spelling() {
    let root = TempDir::new().unwrap();
    write_library_skill(&root, "alpha", "category: \"Guides\"\n");
    write_library_skill(&root, "beta", "category: \"  guides  \"\n");
    write_library_skill(&root, "gamma", "category: \"GUIDES\"\n");
    let value = list_json(&root);
    assert_eq!(value["category_counts"]["Guides"], 3);
    for entry in value["skills"].as_array().unwrap() {
        assert_eq!(entry["category"], "Guides");
    }
}

#[test]
fn marker_install_and_access_time_fill_the_record() {
    let root = TempDir::new().unwrap();
    write_library_skill(&root, "demo", "");
    let installed = root.path().join("home/.config/opencode/skills/demo");
    std::fs::create_dir_all(&installed).unwrap();
    std::fs::write(installed.join("SKILL.md"), b"# demo\n").unwrap();
    std::fs::write(
        installed.join(".symskills.json"),
        br#"{"schema_version":1,"managed_by":"symskills","target":"opencode","name":"demo","mode":"copy","installed":"2026-01-02T03:04:05Z","source_hash":"abc123"}"#,
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::time::{Duration, SystemTime};
        let written = SystemTime::UNIX_EPOCH + Duration::from_secs(1_767_323_045);
        let accessed = written + Duration::from_secs(3600);
        std::fs::File::open(installed.join("SKILL.md")).unwrap();
        let times = std::fs::FileTimes::new()
            .set_accessed(accessed)
            .set_modified(written);
        std::fs::File::options()
            .write(true)
            .open(installed.join("SKILL.md"))
            .unwrap()
            .set_times(times)
            .unwrap();
    }
    let value = list_json(&root);
    let entry = &value["skills"][0];
    let installs = entry["installs"].as_array().unwrap();
    assert_eq!(installs.len(), 1);
    assert_eq!(installs[0]["target"], "opencode");
    assert_eq!(installs[0]["installed_at"], "2026-01-02T03:04:05Z");
    #[cfg(unix)]
    {
        // atime > mtime and beyond the install gap: the only usage evidence.
        assert_eq!(entry["last_used"], "2026-01-02T04:04:05Z");
        assert_eq!(entry["last_used_source"], "install_atime");
    }
}

#[test]
fn unloadable_library_entry_keeps_go_fallback() {
    // Go reports these with cap-std error text that the native path does not
    // reproduce, so the whole report stays on Go.
    for (name, body) in [("broken", "no frontmatter here\n"), ("empty-dir", "")] {
        let root = TempDir::new().unwrap();
        write_library_skill(&root, "good", "");
        let directory = root.path().join("data/symbrain/skills/library").join(name);
        std::fs::create_dir_all(&directory).unwrap();
        if !body.is_empty() {
            std::fs::write(directory.join("SKILL.md"), body).unwrap();
        }
        let output = run(&root, &["skills", "list", "--json"]);
        assert_eq!(output.status.code(), Some(1), "{name}");
        assert!(output.stdout.is_empty(), "{name}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"),
            "{name}: {:?}",
            output.stderr
        );
    }
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
