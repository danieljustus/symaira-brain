//! Black-box tests for the native profile command slice.

use std::process::Command;
use tempfile::TempDir;

fn command_for(root: &TempDir, args: &[&str]) -> Command {
    let base = root.path().canonicalize().unwrap();
    let config = base.join("config");
    let home = base.join("home");
    let project = base.join("project");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&project).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_symbrain"));
    command
        .args(args)
        .current_dir(&project)
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("XDG_CONFIG_HOME", config)
        .env_remove("SYMBRAIN_GO_BINARY");
    command
}

fn run_profile(root: &TempDir, args: &[&str]) -> std::process::Output {
    command_for(root, args).output().unwrap()
}

fn run_profile_with_args(root: &TempDir, args: &[&str]) -> std::process::Output {
    run_profile(root, args)
}

fn run_profile_with_stdin(root: &TempDir, args: &[&str], input: &[u8]) -> std::process::Output {
    let mut child = command_for(root, args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), input).unwrap();
    drop(child.stdin.take());
    child.wait_with_output().unwrap()
}

fn create_file_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(target, link)
    }
}

fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link)
    }
}

#[test]
fn add_uses_restricted_template_by_default_and_secure_modes() {
    let root = TempDir::new().unwrap();
    let output = run_profile(&root, &["profile", "add", "new_profile"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let path = root
        .path()
        .join("config/symbrain/profiles/new_profile.toml");
    let contents = std::fs::read_to_string(&path).unwrap();
    assert!(contents.contains("name        = \"new_profile\""));
    assert!(contents.contains("request_only"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(root.path().join("config/symbrain/profiles"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
}

#[test]
fn add_personal_flag_after_name_and_show_json_are_native() {
    let root = TempDir::new().unwrap();
    let output = run_profile(
        &root,
        &["profile", "add", "personal_copy", "--from", "personal"],
    );
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let path = root
        .path()
        .join("config/symbrain/profiles/personal_copy.toml");
    let contents = std::fs::read_to_string(path).unwrap();
    assert!(contents.contains("mode    = \"full\""));
    assert!(!contents.contains("name        = \"personal\""));

    let output = run_profile(&root, &["profile", "show", "--json", "personal_copy"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["name"], "personal_copy");
    assert_eq!(report["audit"]["enabled"], true);
    assert!(report["servers"][0]["effective_policy"]["unknown"].is_array());
}

#[test]
fn list_json_has_canonical_core_order_and_partial_errors() {
    let root = TempDir::new().unwrap();
    let added = run_profile(&root, &["profile", "add", "zeta"]);
    assert!(added.status.success());
    let config = root.path().join("config/symbrain/profiles");
    std::fs::write(config.join("broken.toml"), "[profile]\nname = \"wrong\"\n").unwrap();
    let output = run_profile(&root, &["profile", "list", "--output=json"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    let entries: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(entries.as_array().unwrap().len(), 2);
    assert_eq!(entries[0]["name"], "broken");
    assert!(entries[0]["error"].is_string());
    assert_eq!(entries[1]["name"], "zeta");
    assert_eq!(entries[1]["servers"][0]["server"], "vault");
    assert_eq!(entries[1]["servers"][3]["server"], "usage");
}

#[test]
fn remove_is_native_and_force_deletes_profile() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("config/symbrain/profiles/existing.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"[profile]\nname = \"existing\"\n").unwrap();
    let output = run_profile(&root, &["profile", "remove", "existing", "--force"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(!path.exists());
}

#[test]
fn remove_prompt_matches_go_for_yes_no_and_eof() {
    for (input, removed) in [
        (b"yes\n".as_slice(), true),
        (b"n\n".as_slice(), false),
        (b"".as_slice(), false),
    ] {
        let root = TempDir::new().unwrap();
        let path = root.path().join("config/symbrain/profiles/existing.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"malformed but removable\n").unwrap();
        let output = run_profile_with_stdin(&root, &["profile", "remove", "existing"], input);
        assert!(output.status.success(), "stderr: {:?}", output.stderr);
        assert_eq!(path.exists(), !removed, "stdout: {:?}", output.stdout);
        if removed {
            assert!(
                output
                    .stdout
                    .windows(b"removed ".len())
                    .any(|window| window == b"removed "),
                "stdout: {:?}",
                output.stdout
            );
            assert!(output.stdout.ends_with(b"\n"));
        } else {
            assert!(output.stdout.ends_with(b"aborted\n"));
        }
    }
}

#[test]
fn remove_refuses_global_and_project_bindings_without_side_effects() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("config/symbrain/profiles/bound.toml");
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(&profile, b"[profile]\nname = \"bound\"\n").unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    std::fs::write(
        root.path().join("home/.claude.json"),
        br#"{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","bound"]}}}"#,
    )
    .unwrap();
    let output = run_profile(&root, &["profile", "remove", "bound"]);
    assert_eq!(output.status.code(), Some(1), "stderr: {:?}", output.stderr);
    assert!(profile.exists());
    assert!(
        output
            .stderr
            .windows(b"is still bound to harnesses".len())
            .any(|w| w == b"is still bound to harnesses")
    );
    assert!(
        output
            .stderr
            .windows(b"claude".len())
            .any(|w| w == b"claude")
    );

    let root = TempDir::new().unwrap();
    let profile = root.path().join("config/symbrain/profiles/existing.toml");
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(&profile, b"[profile]\nname = \"existing\"\n").unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(
        project.join(".mcp.json"),
        br#"{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","existing"]}}}"#,
    )
    .unwrap();
    let output = run_profile_with_args(
        &root,
        &[
            "profile",
            "remove",
            "existing",
            "--project",
            project.to_str().unwrap(),
        ],
    );
    assert_eq!(output.status.code(), Some(1), "stderr: {:?}", output.stderr);
    assert!(profile.exists());
}

#[test]
fn remove_deletes_file_symlink_without_following() {
    let root = TempDir::new().unwrap();
    let profiles = root.path().join("config/symbrain/profiles");
    std::fs::create_dir_all(&profiles).unwrap();
    let outside = root.path().join("outside.toml");
    std::fs::write(&outside, b"outside\n").unwrap();
    create_file_symlink(&outside, &profiles.join("existing.toml")).unwrap();
    let output = run_profile(&root, &["profile", "remove", "existing", "--force"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(!profiles.join("existing.toml").exists());
    assert_eq!(std::fs::read(&outside).unwrap(), b"outside\n");
}

#[cfg(unix)]
#[test]
fn remove_deletes_fifo_without_following() {
    let root = TempDir::new().unwrap();
    let profiles = root.path().join("config/symbrain/profiles");
    std::fs::create_dir_all(&profiles).unwrap();
    std::process::Command::new("mkfifo")
        .arg(profiles.join("existing.toml"))
        .status()
        .unwrap();
    let output = run_profile(&root, &["profile", "remove", "existing", "--force"]);
    assert!(output.status.success(), "stderr: {:?}", output.stderr);
    assert!(!profiles.join("existing.toml").exists());
}

#[test]
fn remove_oracle_fixture_records_source_provenance() {
    let path = if let Some(path) = std::env::var_os("SYMBRAIN_PROFILE_REMOVE_ORACLE_FIXTURE") {
        std::path::PathBuf::from(path)
    } else if cfg!(windows) {
        panic!("native Windows requires SYMBRAIN_PROFILE_REMOVE_ORACLE_FIXTURE from the Go oracle")
    } else {
        let fixture_name = if cfg!(target_os = "macos") {
            "profile_remove_oracle_darwin.json"
        } else {
            "profile_remove_oracle_linux.json"
        };
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture_name)
    };
    let fixture: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(fixture["schema_version"], 1);
    assert!(fixture["go_revision"].as_str().unwrap().len() >= 7);
    assert_eq!(fixture["generator_sha256"].as_str().unwrap().len(), 64);
    for source in [
        "cmd/symbrain/cmd_profile.go",
        "cmd/symbrain/cmd_profile_add.go",
        "cmd/symbrain/cmd_profile_list.go",
        "cmd/symbrain/cmd_profile_remove.go",
        "cmd/symbrain/cmd_profile_show.go",
        "internal/harness/document.go",
        "internal/harness/entry.go",
        "internal/harness/generic_document.go",
        "internal/harness/inventory.go",
        "internal/harness/inventory_read_other.go",
        "internal/harness/inventory_read_unix.go",
        "internal/harness/json_document.go",
        "internal/profile/remove_windows.go",
        "internal/safefs/doc.go",
        "internal/safefs/secure_other.go",
        "internal/safefs/secure_windows.go",
        "internal/harness/ordered_map.go",
        "internal/harness/registry.go",
        "internal/harness/toml_document.go",
        "internal/profile/profile.go",
        "internal/profile/remove_other.go",
        "internal/profile/remove_unix.go",
        "internal/xdg/xdg.go",
    ] {
        assert_eq!(fixture["go_sources"][source].as_str().unwrap().len(), 64);
    }
    assert!(
        fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|case| case["id"] == "bound_global_refuses")
    );
    let case_ids = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|case| case["id"].as_str())
        .collect::<std::collections::BTreeSet<_>>();
    if cfg!(windows) {
        assert_eq!(case_ids.len(), 19);
        for id in [
            "bound_project_symlink_refuses",
            "symlink",
            "symlink_profiles_root",
        ] {
            assert!(
                case_ids.contains(id),
                "Windows oracle omitted native case {id}"
            );
        }
        let skipped = fixture["skipped_cases"].as_array().unwrap();
        assert_eq!(skipped.len(), 1);
        assert_eq!(skipped[0]["id"], "special_file");
        assert_eq!(
            skipped[0]["reason"],
            "POSIX FIFO created with mkfifo has no ordinary Win32 filesystem entry equivalent"
        );
        assert_eq!(
            skipped[0]["native_alternative"],
            "symlink and symlink_profiles_root exercise Windows reparse-point deletion and ancestor refusal; neither is claimed equivalent to a FIFO"
        );
    } else {
        assert_eq!(case_ids.len(), 20);
        assert!(fixture["skipped_cases"].is_null());
    }
}

#[test]
fn remove_rejects_symlinked_profiles_root_without_touching_target() {
    let root = TempDir::new().unwrap();
    let profiles_parent = root.path().join("config/symbrain");
    let outside = root.path().join("outside-profiles");
    std::fs::create_dir_all(&profiles_parent).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let victim = outside.join("existing.toml");
    std::fs::write(&victim, b"outside\n").unwrap();
    create_dir_symlink(&outside, &profiles_parent.join("profiles")).unwrap();

    let output = run_profile(&root, &["profile", "remove", "existing", "--force"]);
    assert!(!output.status.success(), "stderr: {:?}", output.stderr);
    assert_eq!(std::fs::read(&victim).unwrap(), b"outside\n");
}

#[test]
fn remove_fails_closed_for_symlink_harness_config() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("config/symbrain/profiles/bound.toml");
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(&profile, b"[profile]\nname = \"bound\"\n").unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    let real_config = root.path().join("real-claude.json");
    std::fs::write(
        &real_config,
        br#"{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","bound"]}}}"#,
    )
    .unwrap();
    create_file_symlink(&real_config, &root.path().join("home/.claude.json")).unwrap();
    let output = run_profile(&root, &["profile", "remove", "bound"]);
    assert_eq!(output.status.code(), Some(1), "stderr: {:?}", output.stderr);
    assert!(profile.exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains(
        "symbrain profile remove: unable to safely inspect harness bindings:\n  - claude ("
    ));
}

#[cfg(unix)]
#[test]
fn remove_fails_closed_for_fifo_harness_config() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("config/symbrain/profiles/existing.toml");
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(&profile, b"[profile]\nname = \"existing\"\n").unwrap();
    std::fs::create_dir_all(root.path().join("home")).unwrap();
    std::process::Command::new("mkfifo")
        .arg(root.path().join("home/.claude.json"))
        .status()
        .unwrap();
    let output = run_profile(&root, &["profile", "remove", "existing"]);
    assert_eq!(output.status.code(), Some(1), "stderr: {:?}", output.stderr);
    assert!(profile.exists());
}

#[test]
fn remove_keeps_relative_project_display_and_rejects_symlink_project_root() {
    let root = TempDir::new().unwrap();
    let profile = root.path().join("config/symbrain/profiles/existing.toml");
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(&profile, b"[profile]\nname = \"existing\"\n").unwrap();
    std::fs::create_dir_all(root.path().join("project")).unwrap();
    std::fs::write(
        root.path().join("project/.mcp.json"),
        br#"{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","existing"]}}}"#,
    )
    .unwrap();
    let output = run_profile(&root, &["profile", "remove", "existing", "--project", "."]);
    assert_eq!(output.status.code(), Some(1), "stderr: {:?}", output.stderr);
    assert!(String::from_utf8_lossy(&output.stderr).contains("claude (.mcp.json)"));
    assert!(profile.exists());

    let root = TempDir::new().unwrap();
    let profile = root.path().join("config/symbrain/profiles/existing.toml");
    std::fs::create_dir_all(profile.parent().unwrap()).unwrap();
    std::fs::write(&profile, b"[profile]\nname = \"existing\"\n").unwrap();
    let real_project = root.path().join("real-project");
    std::fs::create_dir_all(root.path().join("project")).unwrap();
    std::fs::create_dir_all(&real_project).unwrap();
    std::fs::write(
        real_project.join(".mcp.json"),
        br#"{"mcpServers":{"symbrain":{"command":"symbrain","args":["mcp","--profile","existing"]}}}"#,
    )
    .unwrap();
    create_dir_symlink(&real_project, &root.path().join("project/project-link")).unwrap();
    let output = run_profile(
        &root,
        &["profile", "remove", "existing", "--project", "project-link"],
    );
    assert_eq!(output.status.code(), Some(1), "stderr: {:?}", output.stderr);
    assert!(profile.exists());
}

#[test]
fn remove_aborts_on_oversized_confirmation_input() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("config/symbrain/profiles/existing.toml");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"[profile]\nname = \"existing\"\n").unwrap();
    let input = vec![b'x'; 1025];
    let output = run_profile_with_stdin(&root, &["profile", "remove", "existing"], &input);
    assert_eq!(output.status.code(), Some(1), "stderr: {:?}", output.stderr);
    assert!(path.exists());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "symbrain profile remove: confirmation input exceeds maximum of 1024 bytes\n"
    );
}
