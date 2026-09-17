//! Native symbrain skills targets user-scope byte contract.

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
    let expected = format!(
        "TARGET\tINSTALLED\tMANAGED\tUNMANAGED\tSKILL ROOT\n\
claude\tfalse\t0\t0\t{}/.claude/skills\n\
opencode\tfalse\t0\t0\t{}/.config/opencode/skills\n\
codex\tfalse\t0\t0\t{}/.agents/skills\n\
antigravity\tfalse\t0\t0\t{}/.gemini/config/skills\n\
hermes\tfalse\t0\t0\t{}/.hermes/skills/symaira\n\
openclaw\tfalse\t0\t0\t{}/.openclaw/skills\n",
        home.display(),
        home.display(),
        home.display(),
        home.display(),
        home.display(),
        home.display()
    );
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

#[test]
fn managed_and_unmanaged_skill_roots_keep_go_fallback_before_output() {
    let root = TempDir::new().unwrap();
    let opencode = root.path().join("home/.config/opencode/skills");
    std::fs::create_dir_all(opencode.join("managed")).unwrap();
    std::fs::write(opencode.join("managed/.symskills.json"), b"{}").unwrap();
    std::fs::create_dir_all(opencode.join("handwritten")).unwrap();

    let output = run(&root, &["skills", "targets"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[cfg(unix)]
#[test]
fn symlinked_skill_root_keeps_go_fallback_before_output() {
    use std::os::unix::fs::symlink;

    let root = TempDir::new().unwrap();
    let real_root = root.path().join("real-opencode-skills");
    let opencode = root.path().join("home/.config/opencode/skills");
    std::fs::create_dir_all(real_root.join("handwritten")).unwrap();
    std::fs::create_dir_all(opencode.parent().unwrap()).unwrap();
    symlink(&real_root, &opencode).unwrap();

    let output = run(&root, &["skills", "targets", "--json"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn target_binary_keeps_go_fallback_before_output() {
    let root = TempDir::new().unwrap();
    let bin_dir = root.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    std::fs::write(bin_dir.join("opencode"), b"placeholder").unwrap();

    let mut command = command(&root, &["skills", "targets"]);
    command.env("PATH", bin_dir);
    let output = command.output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"));
}

#[test]
fn parsed_and_dynamic_targets_variants_keep_go_fallback_before_output() {
    for (name, args, env_name, env_value) in [
        (
            "scope",
            &["skills", "targets", "--scope", "user"][..],
            None,
            None,
        ),
        (
            "project",
            &["skills", "targets", "--scope", "project"][..],
            None,
            None,
        ),
        (
            "config",
            &["skills", "targets"][..],
            Some("SYMSKILLS_LIBRARY_DIR"),
            Some("/different/library"),
        ),
    ] {
        let root = TempDir::new().unwrap();
        let mut command = command(&root, args);
        if let Some(name) = env_name {
            command.env(name, env_value.unwrap());
        }
        let output = command.output().unwrap();
        assert_eq!(output.status.code(), Some(1), "{name}: {output:?}");
        assert!(output.stdout.is_empty(), "{name}: {:?}", output.stdout);
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("no Go fallback was found"),
            "{name}: {:?}",
            output.stderr
        );
    }
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
