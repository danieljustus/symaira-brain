//! CLI byte checks for the native `OpenCode` user-scope status slice.

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
fn opencode_user_status_empty_table_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"No installed skills found.\n");
}

#[test]
fn opencode_user_status_populated_table_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let skill = root
        .path()
        .join("home")
        .join(".config")
        .join("opencode")
        .join("skills")
        .join("handwritten");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), b"handwritten\n").unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let expected = format!(
        "TARGET\tSKILL\tSTATUS\tMODE\tPATH\nopencode\thandwritten\tunmanaged\t-\t{}\n",
        skill.display()
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[test]
fn opencode_user_status_empty_json_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(
        &root,
        &["skills", "status", "--target", "opencode", "--json"],
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"{\"installs\":[],\"summary\":{\"in_sync\":0,\"stale\":0,\"harness_changed\":0,\"conflict\":0,\"orphaned\":0,\"unmanaged\":0}}\n"
    );
}

#[test]
fn default_user_status_empty_table_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "status"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, b"No installed skills found.\n");
}

#[test]
fn default_user_status_empty_json_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "status", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout,
        b"{\"installs\":[],\"summary\":{\"in_sync\":0,\"stale\":0,\"harness_changed\":0,\"conflict\":0,\"orphaned\":0,\"unmanaged\":0}}\n"
    );
}

#[test]
fn unsupported_status_target_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "status", "--target", "claude"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn configured_status_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("config/symskills")).unwrap();
    std::fs::write(
        root.path().join("config/symskills/config.toml"),
        b"library_dir = \"/different/library\"\n",
    )
    .unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn default_status_with_dynamic_target_state_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("home/.config/opencode/skills")).unwrap();
    let output = run(&root, &["skills", "status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn legacy_go_config_wins_when_new_skills_config_dir_exists() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("config/symbrain/skills")).unwrap();
    std::fs::create_dir_all(root.path().join("config/symskills")).unwrap();
    std::fs::write(
        root.path().join("config/symskills/config.toml"),
        b"library_dir = \"/different/library\"\n",
    )
    .unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn symskills_config_override_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let mut command = command(&root, &["skills", "status", "--target", "opencode"]);
    let output = command
        .env("SYMSKILLS_LIBRARY_DIR", "/different/library")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[cfg(unix)]
#[test]
fn opencode_user_status_link_onto_file_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let skills = root.path().join("home/.config/opencode/skills");
    std::fs::create_dir_all(&skills).unwrap();
    let target = root.path().join("home/.config/opencode/plain.md");
    std::fs::write(&target, b"plain\n").unwrap();
    std::os::unix::fs::symlink(&target, skills.join("linkedfile")).unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let expected = format!(
        "TARGET\tSKILL\tSTATUS\tMODE\tPATH\nopencode\tlinkedfile\tunmanaged\t-\t{}\n",
        skills.join("linkedfile").display()
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[cfg(unix)]
#[test]
fn opencode_user_status_symlink_chain_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let skills = root.path().join("home/.config/opencode/skills");
    std::fs::create_dir_all(&skills).unwrap();
    let real = root.path().join("home/.config/opencode/real");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("SKILL.md"), b"real\n").unwrap();
    let middle = root.path().join("home/.config/opencode/middle");
    std::os::unix::fs::symlink(&real, &middle).unwrap();
    std::os::unix::fs::symlink(&middle, skills.join("chain")).unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn opencode_user_status_marker_states_keep_go_fallback() {
    // Go reports these markers with encoding/json error text and keeps the
    // fields it managed to fill (an unknown schema_version is even accepted
    // outright), so the native scan leaves them to Go instead of exchanging
    // one set of bytes for another.
    let markers: [(&str, &[u8]); 4] = [
        ("broken", b"{not json"),
        ("emptymarker", b""),
        (
            "wrongtype",
            br#"{"schema_version":1,"managed_by":"symskills","target":"opencode","name":"wrongtype","mode":"copy","installed":5,"source_hash":"abc123"}"#,
        ),
        (
            "schema",
            br#"{"schema_version":99,"managed_by":"symskills","target":"opencode","name":"schema","mode":"copy","installed":"2026-01-02T03:04:05Z","source_hash":"abc123"}"#,
        ),
    ];
    for (name, marker) in markers {
        let root = TempDir::new().unwrap();
        let skill = root
            .path()
            .join(format!("home/.config/opencode/skills/{name}"));
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::write(skill.join("SKILL.md"), b"body\n").unwrap();
        std::fs::write(skill.join(".symskills.json"), marker).unwrap();
        let output = run(&root, &["skills", "status", "--target", "opencode"]);
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
fn opencode_user_status_managed_marker_row_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    let skill = root
        .path()
        .join("home")
        .join(".config")
        .join("opencode")
        .join("skills")
        .join("managed");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), b"managed\n").unwrap();
    std::fs::write(
        skill.join(".symskills.json"),
        br#"{"schema_version":1,"managed_by":"symskills","target":"opencode","name":"managed","mode":"copy","installed":"2026-01-02T03:04:05Z","source_hash":"abc123","allow_executable":true}"#,
    )
    .unwrap();
    let output = run(
        &root,
        &["skills", "status", "--target", "opencode", "--json"],
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let expected = format!(
        "{{\"installs\":[{{\"target\":\"opencode\",\"name\":\"managed\",\"path\":\"{}\",\"status\":\"orphaned\",\"mode\":\"copy\",\"installed_at\":\"2026-01-02T03:04:05Z\",\"source_hash\":\"abc123\",\"allow_executable\":true}}],\"summary\":{{\"in_sync\":0,\"stale\":0,\"harness_changed\":0,\"conflict\":0,\"orphaned\":1,\"unmanaged\":0}}}}\n",
        skill.display()
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[test]
fn opencode_project_status_matches_go_bytes() {
    let root = TempDir::new().unwrap();
    // Project scope resolves its root from the process working directory, and
    // Go's `os.Getwd` with no inherited `PWD` (which `env_clear` guarantees)
    // returns the symlink-resolved path -- measured: `/private/var/...`, not
    // the `/var/...` spelling `TempDir` hands out on macOS. The shipped bytes
    // therefore carry the resolved root, so the expectation has to be built
    // from it too. Only this case needs it: every other case in this file is
    // user scope, where the root comes from the environment verbatim.
    #[cfg(windows)]
    let resolved = root.path().to_path_buf();
    #[cfg(not(windows))]
    let resolved = root.path().canonicalize().unwrap();
    let skill = resolved
        .join("project")
        .join(".opencode")
        .join("skills")
        .join("handwritten");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), b"handwritten\n").unwrap();
    let output = run(
        &root,
        &[
            "skills", "status", "--target", "opencode", "--scope", "project",
        ],
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let expected = format!(
        "TARGET\tSKILL\tSTATUS\tMODE\tPATH\nopencode\thandwritten\tunmanaged\t-\t{}\n",
        skill.display()
    );
    assert_eq!(output.stdout, expected.as_bytes());
}

#[test]
fn opencode_project_status_ignores_the_user_root() {
    let root = TempDir::new().unwrap();
    let user_skill = root.path().join("home/.config/opencode/skills/user-only");
    std::fs::create_dir_all(&user_skill).unwrap();
    std::fs::write(user_skill.join("SKILL.md"), b"user\n").unwrap();
    let project_skill = root.path().join("project/.opencode/skills/only-here");
    std::fs::create_dir_all(&project_skill).unwrap();
    std::fs::write(project_skill.join("SKILL.md"), b"project\n").unwrap();
    let output = run(
        &root,
        &[
            "skills", "status", "--target", "opencode", "--scope", "project",
        ],
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(!String::from_utf8_lossy(&output.stdout).contains("user-only"));
    assert!(String::from_utf8_lossy(&output.stdout).contains("only-here"));
}

#[cfg(unix)]
#[test]
fn opencode_project_status_symlink_chain_keeps_go_fallback() {
    let root = TempDir::new().unwrap();
    let skills = root.path().join("project/.opencode/skills");
    std::fs::create_dir_all(&skills).unwrap();
    let real = root.path().join("project/.opencode/real");
    std::fs::create_dir_all(&real).unwrap();
    std::fs::write(real.join("SKILL.md"), b"real\n").unwrap();
    let middle = root.path().join("project/.opencode/middle");
    std::os::unix::fs::symlink(&real, &middle).unwrap();
    std::os::unix::fs::symlink(&middle, skills.join("chain")).unwrap();
    let output = run(
        &root,
        &[
            "skills", "status", "--target", "opencode", "--scope", "project",
        ],
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn opencode_project_status_no_scope_flag_defaults_to_user() {
    let root = TempDir::new().unwrap();
    let project_skill = root.path().join("project/.opencode/skills/only-here");
    std::fs::create_dir_all(&project_skill).unwrap();
    std::fs::write(project_skill.join("SKILL.md"), b"project\n").unwrap();
    let output = run(&root, &["skills", "status", "--target", "opencode"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(output.stdout, b"No installed skills found.\n");
}

#[test]
fn opencode_user_status_json_escapes_html_like_go() {
    let root = TempDir::new().unwrap();
    let skill = root
        .path()
        .join("home")
        .join(".config")
        .join("opencode")
        .join("skills")
        .join("a&b");
    std::fs::create_dir_all(&skill).unwrap();
    std::fs::write(skill.join("SKILL.md"), b"esc\n").unwrap();
    let output = run(
        &root,
        &["skills", "status", "--target", "opencode", "--json"],
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    // Go encodes with `json.Encoder`, which escapes `&`, `<` and `>`.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a\\u0026b"), "{stdout}");
    assert!(!stdout.contains("a&b"), "{stdout}");
}
