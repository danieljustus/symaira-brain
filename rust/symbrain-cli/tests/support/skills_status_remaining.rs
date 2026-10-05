// Original project-chain and default-scope assertions, retained at the module scope.
#[cfg(unix)]
#[test]
fn opencode_project_status_symlink_chain_is_native() {
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
    assert!(output.status.success(), "{:?}", output.stderr);
    assert!(output.stderr.is_empty());
    assert!(!output.stdout.is_empty());
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
