use super::*;

fn dirs(root: &Path) -> Dirs {
    Dirs {
        library: root.join("library"),
        render: root.join("render"),
        base: root.join("base"),
        profiles: root.join("profiles"),
    }
}

fn call(dirs: &Dirs, name: &str, dry_run: bool) -> Outcome {
    let raw = RawValue::from_string(
        json!({"name": name, "rev": "HEAD~1", "dry_run": dry_run}).to_string(),
    )
    .unwrap();
    restore(dirs, Some(&raw))
}

#[test]
fn restore_rejects_unsafe_names_before_filesystem_access() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs(root.path());
    for name in [
        "",
        ".",
        "..",
        "../../../../victim",
        "a/b",
        "a\\b",
        "/victim",
        "a\0b",
    ] {
        for dry_run in [true, false] {
            let error = match call(&dirs, name, dry_run) {
                Err(error) => error,
                Ok(text) => panic!("unsafe restore succeeded: {text}"),
            };
            assert_eq!(error.code, "validation");
            let expected = if name.is_empty() {
                "restore skill: name and rev are required".to_owned()
            } else {
                format!(
                    "restore skill: skill name {} must be a single lowercase alphanumeric-dash segment without consecutive, leading, or trailing hyphens",
                    gotext::quote(name)
                )
            };
            assert_eq!(error.message, expected, "{name:?}, dry_run={dry_run}");
            assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
        }
    }
}

fn versioned_skill(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: victim\ndescription: victim\n---\nv1 body\n",
    )
    .unwrap();
    let status = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(dir)
        .status()
        .unwrap();
    assert!(status.success());
    vcs::commit(dir, "import: v1").unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        "---\nname: victim\ndescription: victim\n---\nv2 body\n",
    )
    .unwrap();
    vcs::commit(dir, "update: v2").unwrap();
}

#[test]
fn restore_traversal_cannot_overwrite_victim_even_in_dry_run() {
    for dry_run in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let victim = root.join("victim");
        versioned_skill(&victim);
        let dirs = dirs(&root);
        std::fs::create_dir_all(&dirs.library).unwrap();
        // Both Go-style joins clean this to our own victim, irrespective of
        // the host TMPDIR depth. The unfixed test cannot write outside root.
        let name = format!("{}{}", "../".repeat(100), victim.display());
        let before = std::fs::read(victim.join("SKILL.md")).unwrap();
        let head = vcs::resolve(&victim, "HEAD").unwrap();
        let result = call(&dirs, &name, dry_run);
        assert_eq!(std::fs::read(victim.join("SKILL.md")).unwrap(), before);
        assert_eq!(vcs::resolve(&victim, "HEAD").unwrap(), head);
        assert!(
            matches!(result, Err(error) if error.code == "validation" && error.message.contains("skill name"))
        );
    }
}

#[test]
fn restore_clean_name_still_restores() {
    let root = tempfile::tempdir().unwrap();
    let dirs = dirs(root.path());
    let victim = dirs.library.join("victim");
    versioned_skill(&victim);
    let result = match call(&dirs, "victim", false) {
        Ok(text) => text,
        Err(error) => panic!("{}", error.message),
    };
    let result: Value = serde_json::from_str(&result).unwrap();
    assert_eq!(result["action"], "restored");
    assert!(
        std::fs::read_to_string(victim.join("SKILL.md"))
            .unwrap()
            .contains("v1 body")
    );
}
