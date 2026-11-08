//! `symbrain skills` library operations (#905): import, validate, history and
//! restore through the real binary in a disposable HOME/XDG root.

#[path = "support/skills_sandbox.rs"]
mod skills_sandbox;

use std::fs;

use skills_sandbox::{Sandbox, git_available};

#[test]
fn import_creates_a_versioned_library_skill_and_refuses_duplicates() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    let src = source.to_str().unwrap();

    let first = sandbox.skills(&["import", src]);
    assert_eq!(first.code, 0, "{}", first.err);
    assert!(first.out.starts_with("Imported alpha to "), "{}", first.out);
    assert!(sandbox.library("alpha/SKILL.md").is_file());
    assert!(sandbox.library("alpha/references/r.md").is_file());

    let again = sandbox.skills(&["import", src]);
    assert_eq!(again.code, 1);
    assert!(
        again.err.contains("already exists in library"),
        "{}",
        again.err
    );

    fs::write(
        source.join("SKILL.md"),
        "---\nname: alpha\ndescription: Test skill\ncategory: dev\n---\n# Alpha v2\n",
    )
    .unwrap();
    let update = sandbox.skills(&["import", "--update", "--json", src]);
    assert_eq!(update.code, 0, "{}", update.err);
    assert_eq!(update.json()["name"], "alpha");
    assert!(
        fs::read_to_string(sandbox.library("alpha/SKILL.md"))
            .unwrap()
            .contains("v2")
    );

    if git_available() {
        let history = sandbox.skills(&["history", "alpha", "--json"]);
        assert_eq!(history.code, 0, "{}", history.err);
        let commits = history.json()["history"].as_array().unwrap().clone();
        let operations: Vec<&str> = commits
            .iter()
            .map(|c| c["operation"].as_str().unwrap())
            .collect();
        assert_eq!(operations, ["update", "import"]);
        let limited = sandbox.skills(&["history", "alpha", "--limit", "1", "--json"]);
        assert_eq!(limited.json()["history"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn batch_import_reports_each_skill_and_fails_when_one_fails() {
    let sandbox = Sandbox::new();
    sandbox.source("one", "# One");
    sandbox.source("two", "# Two");
    let batch = sandbox.skills(&["import", "--batch", sandbox.path("src").to_str().unwrap()]);
    assert_eq!(batch.code, 0, "{}", batch.err);
    assert!(
        batch.out.contains("Summary: 2 imported, 0 failed"),
        "{}",
        batch.out
    );
    let repeat = sandbox.skills(&[
        "import",
        "--batch",
        "--json",
        sandbox.path("src").to_str().unwrap(),
    ]);
    assert_eq!(repeat.code, 1);
    let rows = repeat.json();
    assert!(
        rows.as_array()
            .unwrap()
            .iter()
            .all(|row| row["status"] == "failed")
    );
}

#[test]
fn import_rejects_symlinks_escaping_the_source_root() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("leaky", "# Leaky");
    fs::write(sandbox.path("secret.txt"), "secret").unwrap();
    std::os::unix::fs::symlink(
        sandbox.path("secret.txt"),
        source.join("references/link.md"),
    )
    .unwrap();
    let run = sandbox.skills(&["import", source.to_str().unwrap()]);
    assert_ne!(run.code, 0);
    assert!(!sandbox.library("leaky/references/link.md").exists());
}

#[test]
fn validate_reports_errors_warnings_and_strict_mode() {
    let sandbox = Sandbox::new();
    let good = sandbox.source("good", "# Good");
    let ok = sandbox.skills(&["validate", good.to_str().unwrap()]);
    assert_eq!((ok.code, ok.out.as_str()), (0, "valid\n"));

    let bad = sandbox.path("src/bad");
    fs::create_dir_all(&bad).unwrap();
    fs::write(bad.join("SKILL.md"), "---\nname: Bad_Name\n---\nx\n").unwrap();
    let broken = sandbox.skills(&["validate", bad.to_str().unwrap()]);
    assert_eq!(broken.code, 1);
    assert!(broken.out.contains("error\tname_format"), "{}", broken.out);
    assert!(broken.err.contains("validation failed"));
    let json = sandbox.skills(&["validate", "--json", bad.to_str().unwrap()]);
    assert_eq!(json.code, 1);
    assert_eq!(json.json()["valid"], false);

    let warn = sandbox.path("src/warn");
    fs::create_dir_all(&warn).unwrap();
    fs::write(
        warn.join("SKILL.md"),
        "---\nname: warn\ndescription: d\n---\nx\n",
    )
    .unwrap();
    assert_eq!(
        sandbox.skills(&["validate", warn.to_str().unwrap()]).code,
        0
    );
    assert_eq!(
        sandbox
            .skills(&["validate", "--strict", warn.to_str().unwrap()])
            .code,
        1
    );

    let missing = sandbox.skills(&["validate", "nope"]);
    assert_eq!(missing.code, 1);
    assert!(missing.err.contains("not found locally"), "{}", missing.err);
    assert_eq!(sandbox.skills(&["validate"]).code, 2);
}

#[test]
fn restore_rolls_back_by_forward_commit_with_safety_checks() {
    if !git_available() {
        return;
    }
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    fs::write(source.join("references/r.md"), "changed\n").unwrap();
    assert_eq!(
        sandbox
            .skills(&["import", "--update", source.to_str().unwrap()])
            .code,
        0
    );

    let plan = sandbox.skills(&["restore", "alpha", "--to", "HEAD~1", "--dry-run", "--json"]);
    assert_eq!(plan.code, 0, "{}", plan.err);
    assert_eq!(plan.json()["action"], "planned");
    assert_eq!(plan.json()["changed_files"][0], "references/r.md");
    assert_eq!(
        fs::read_to_string(sandbox.library("alpha/references/r.md")).unwrap(),
        "changed\n"
    );

    fs::write(sandbox.library("alpha/references/r.md"), "dirty\n").unwrap();
    let dirty = sandbox.skills(&["restore", "alpha", "--to", "HEAD~1"]);
    assert_eq!(dirty.code, 1);
    assert!(dirty.err.contains("uncommitted changes"), "{}", dirty.err);

    let done = sandbox.skills(&["restore", "alpha", "--to", "HEAD~1", "--allow-dirty"]);
    assert_eq!(done.code, 0, "{}", done.err);
    assert!(done.out.starts_with("restored alpha to "), "{}", done.out);
    assert_eq!(
        fs::read_to_string(sandbox.library("alpha/references/r.md")).unwrap(),
        "ref\n"
    );
    let history = sandbox.skills(&["history", "alpha", "--json"]);
    let ops: Vec<String> = history.json()["history"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["operation"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ops[0], "restore");
    assert!(ops.len() >= 4, "history rewritten: {ops:?}");

    assert_eq!(sandbox.skills(&["restore", "alpha"]).code, 2);
    let unknown = sandbox.skills(&["restore", "alpha", "--to", "deadbeef0"]);
    assert_eq!(unknown.code, 1);
    assert!(unknown.err.contains("not found"), "{}", unknown.err);
}

#[test]
fn restore_reports_stale_installs_and_sync_reinstalls_them() {
    if !git_available() {
        return;
    }
    for sync in [false, true] {
        let sandbox = Sandbox::new();
        let source = sandbox.source("alpha", "# Alpha");
        assert_eq!(
            sandbox.skills(&["import", source.to_str().unwrap()]).code,
            0
        );
        fs::write(source.join("references/r.md"), "changed\n").unwrap();
        assert_eq!(
            sandbox
                .skills(&["import", "--update", source.to_str().unwrap()])
                .code,
            0
        );
        let run = sandbox.skills(&["install", "--target", "claude", "--mode", "copy", "alpha"]);
        assert_eq!(run.code, 0, "{}", run.err);
        let installed = sandbox.path("home/.claude/skills/alpha");
        assert_eq!(
            fs::read_to_string(installed.join("references/r.md")).unwrap(),
            "changed\n"
        );
        let mut args = vec!["restore", "alpha", "--to", "HEAD~1"];
        if sync {
            args.push("--sync");
        }
        let restored = sandbox.skills(&args);
        assert_eq!(restored.code, 0, "{}", restored.err);
        let expected = if sync { "ref\n" } else { "changed\n" };
        assert_eq!(
            fs::read_to_string(installed.join("references/r.md")).unwrap(),
            expected
        );
        assert_eq!(
            restored.out.contains("stale installs: claude"),
            !sync,
            "{}",
            restored.out
        );
    }
}
