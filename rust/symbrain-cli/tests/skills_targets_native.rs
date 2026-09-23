//! Native symbrain skills targets user-scope byte contract.

use std::fmt::Write as _;
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
fn user_targets_table_matches_go_bytes_for_empty_sandbox() {
    let root = TempDir::new().unwrap();
    let home = root.path().join("home");
    let roots = [
        home.join(".claude").join("skills"),
        home.join(".config").join("opencode").join("skills"),
        home.join(".agents").join("skills"),
        home.join(".gemini").join("config").join("skills"),
        home.join(".hermes").join("skills").join("symaira"),
        home.join(".openclaw").join("skills"),
    ];
    let mut expected = String::from("TARGET\tINSTALLED\tMANAGED\tUNMANAGED\tSKILL ROOT\n");
    for (target, root) in [
        "claude",
        "opencode",
        "codex",
        "antigravity",
        "hermes",
        "openclaw",
    ]
    .into_iter()
    .zip(roots)
    {
        writeln!(&mut expected, "{target}\tfalse\t0\t0\t{}", root.display()).unwrap();
    }
    let output = run(&root, &["skills", "targets"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout, expected.as_bytes());
}

#[test]
fn user_targets_json_is_compact_and_has_go_schema() {
    let root = TempDir::new().unwrap();
    let output = run(&root, &["skills", "targets", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    assert!(
        !output.stdout[..output.stdout.len() - 1].contains(&b'\n'),
        "Go emits one compact JSON line"
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let targets = value["targets"].as_array().unwrap();
    assert_eq!(targets.len(), 6);
    assert_eq!(targets[0]["target"], "claude");
    assert_eq!(targets[0]["display_name"], "Claude Code");
    assert_eq!(targets[0]["install_state"], "missing");
    assert_eq!(targets[0]["verification_status"], "not_verified");
    assert_eq!(targets[0]["runtime_capabilities"]["subagents"], "supported");
}

fn opencode_row(output: &Output) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    value["targets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["target"] == "opencode")
        .unwrap()
        .clone()
}

#[test]
fn managed_and_unmanaged_skill_roots_are_classified_natively() {
    let root = TempDir::new().unwrap();
    let opencode = root.path().join("home/.config/opencode/skills");
    std::fs::create_dir_all(opencode.join("managed")).unwrap();
    std::fs::write(opencode.join("managed/.symskills.json"), b"{}").unwrap();
    std::fs::create_dir_all(opencode.join("handwritten")).unwrap();

    let output = run(&root, &["skills", "targets", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(output.stderr.is_empty());
    let row = opencode_row(&output);
    assert_eq!(row["skill_root_exists"], true);
    assert_eq!(row["skill_root_readable"], true);
    assert_eq!(row["managed_skills_count"], 1);
    assert_eq!(row["unmanaged_skills_count"], 1);
    assert_eq!(row["install_state"], "mixed");
    assert_eq!(
        row["setup_hint"],
        "Harness contains 1 managed and 1 unmanaged skill(s)"
    );
    // Creating the skill root also creates the harness config directory, which
    // is evidence in its own right.
    assert_eq!(row["installed"], true);
    assert!(
        row["evidence"].as_str().unwrap().starts_with("config_dir:"),
        "evidence: {}",
        row["evidence"]
    );
}

#[cfg(unix)]
#[test]
fn symlinked_skill_root_is_inventoried_natively() {
    use std::os::unix::fs::symlink;

    let root = TempDir::new().unwrap();
    let real_root = root.path().join("real-opencode-skills");
    let opencode = root.path().join("home/.config/opencode/skills");
    std::fs::create_dir_all(real_root.join("handwritten")).unwrap();
    std::fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    symlink(&real_root, &opencode).unwrap();

    let output = run(&root, &["skills", "targets", "--json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let row = opencode_row(&output);
    // Go reports the link itself as the skill root and follows it for counts.
    assert_eq!(row["skill_root_exists"], true);
    assert_eq!(row["skill_root_readable"], true);
    assert_eq!(row["managed_skills_count"], 0);
    assert_eq!(row["unmanaged_skills_count"], 1);
    assert_eq!(row["install_state"], "unmanaged");
}

#[test]
fn target_binary_is_reported_as_evidence_natively() {
    let root = TempDir::new().unwrap();
    let bin_dir = root.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let binary = bin_dir.join("opencode");
    std::fs::write(&binary, b"placeholder").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut command = command(&root, &["skills", "targets", "--json"]);
    command.env("PATH", &bin_dir);
    let output = command.output().unwrap();
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let row = opencode_row(&output);
    assert_eq!(row["installed"], true);
    assert_eq!(row["verification_status"], "verified");
    assert_eq!(row["evidence"], format!("binary:{}", binary.display()));
    assert_eq!(row["install_state"], "missing");
}

#[test]
fn scope_flag_is_native_but_config_stays_on_go() {
    // `--scope user` is the default scan, `--scope project` resolves the
    // workspace roots; only a dynamic symskills config still needs Go.
    let root = TempDir::new().unwrap();
    let user = run(&root, &["skills", "targets", "--json"]).stdout;
    let explicit = run(&root, &["skills", "targets", "--scope", "user", "--json"]);
    assert!(explicit.status.success(), "stderr: {:?}", explicit.stderr);
    assert_eq!(explicit.stdout, user);

    let project = run(
        &root,
        &["skills", "targets", "--scope", "project", "--json"],
    );
    assert!(project.status.success(), "stderr: {:?}", project.stderr);
    let row = opencode_row(&project);
    assert_eq!(
        row["effective_skill_root"],
        root.path()
            .join("project")
            .join(".opencode")
            .join("skills")
            .display()
            .to_string()
    );

    let mut command = command(&root, &["skills", "targets"]);
    let configured = command
        .env("SYMSKILLS_LIBRARY_DIR", "/different/library")
        .output()
        .unwrap();
    assert_eq!(configured.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&configured.stderr).contains("no Go fallback was found"));
}

#[test]
fn config_file_targets_variant_keeps_go_fallback_before_output() {
    let root = TempDir::new().unwrap();
    std::fs::create_dir_all(root.path().join("config/symskills")).unwrap();
    std::fs::write(
        root.path().join("config/symskills/config.toml"),
        b"[[targets]]\nname = \"custom\"\nskill_root_user = \"/tmp/custom\"\n",
    )
    .unwrap();
    let output = run(&root, &["skills", "targets"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}
