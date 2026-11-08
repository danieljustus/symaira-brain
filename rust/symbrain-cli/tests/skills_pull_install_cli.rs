//! `symbrain skills` pull, install and render (#905) through the real binary
//! in a disposable HOME/XDG root.

#[path = "support/skills_sandbox.rs"]
mod skills_sandbox;

use std::fs;
use std::path::PathBuf;

use skills_sandbox::Sandbox;

fn install_copy(sandbox: &Sandbox, name: &str, target: &str) -> PathBuf {
    let run = sandbox.skills(&["install", "--target", target, "--mode", "copy", name]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(
        run.out.starts_with(&format!("installed {name} at ")),
        "{}",
        run.out
    );
    PathBuf::from(run.out.trim().split(" at ").nth(1).unwrap())
}

#[test]
fn pull_stages_harness_edits_and_apply_promotes_them() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    let installed = install_copy(&sandbox, "alpha", "hermes");
    assert!(installed.ends_with("home/.hermes/skills/symaira/alpha"));

    let current = sandbox.skills(&["pull", "alpha", "--target", "hermes"]);
    assert_eq!(current.code, 0, "{}", current.err);
    assert!(
        current.out.starts_with("current hermes/alpha"),
        "{}",
        current.out
    );
    assert!(
        !sandbox
            .path("data/symbrain/skills/pending/hermes/alpha")
            .exists()
    );

    let mut text = fs::read_to_string(installed.join("SKILL.md")).unwrap();
    text.push_str("Curator edit\n");
    fs::write(installed.join("SKILL.md"), text).unwrap();
    fs::write(installed.join("references/new.md"), "new\n").unwrap();
    fs::remove_file(installed.join("references/r.md")).unwrap();

    let plan = sandbox.skills(&["pull", "alpha", "--target", "hermes", "--dry-run"]);
    assert_eq!(plan.code, 0, "{}", plan.err);
    for line in [
        "planned hermes/alpha",
        "  modified SKILL.md",
        "  added references/new.md",
        "  removed references/r.md",
    ] {
        assert!(plan.out.contains(line), "{line} missing: {}", plan.out);
    }
    assert!(
        !sandbox
            .path("data/symbrain/skills/pending")
            .join("hermes")
            .exists()
    );
    assert!(
        fs::read_to_string(sandbox.library("alpha/SKILL.md"))
            .unwrap()
            .ends_with("# Alpha\n")
    );

    let staged = sandbox.skills(&["pull", "alpha", "--target", "hermes", "--json"]);
    assert_eq!(staged.code, 0, "{}", staged.err);
    let report = staged.json();
    assert_eq!(report["action"], "staged");
    let pending = PathBuf::from(report["stage_path"].as_str().unwrap());
    assert!(pending.join("references/new.md").is_file());
    assert!(!pending.join("references/r.md").exists());
    assert!(
        fs::read_to_string(sandbox.library("alpha/SKILL.md"))
            .unwrap()
            .ends_with("# Alpha\n")
    );

    let applied = sandbox.skills(&["pull", "alpha", "--target", "hermes", "--apply"]);
    assert_eq!(applied.code, 0, "{}", applied.err);
    assert_eq!(applied.out, "applied hermes alpha\n");
    let library = fs::read_to_string(sandbox.library("alpha/SKILL.md")).unwrap();
    assert!(library.contains("Curator edit"), "{library}");
    assert!(
        !library.contains("compatibility"),
        "overlay key leaked: {library}"
    );
    assert!(sandbox.library("alpha/references/new.md").is_file());
    assert!(!sandbox.library("alpha/references/r.md").exists());
    assert!(!pending.exists());
    assert!(!sandbox.library("alpha/.symskills-pull.json").exists());
    assert_eq!(
        sandbox
            .skills(&["pull", "alpha", "--target", "hermes", "--apply"])
            .code,
        1
    );
    assert_eq!(sandbox.skills(&["validate", "alpha"]).code, 0);
}

#[test]
fn pull_ignores_a_stale_install_and_all_target_skips_missing_installs() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    install_copy(&sandbox, "alpha", "claude");
    fs::write(
        sandbox.library("alpha/SKILL.md"),
        "---\nname: alpha\ndescription: Test skill\ncategory: dev\n---\n# Alpha changed in library\n",
    )
    .unwrap();
    let run = sandbox.skills(&["pull", "alpha"]);
    assert_eq!(run.code, 0, "{}", run.err);
    assert!(run.out.starts_with("current claude/alpha"), "{}", run.out);
    let none = sandbox.skills(&["pull", "alpha", "--target", "codex"]);
    assert_eq!(none.code, 1);
    assert!(
        none.err.contains("installed skill not found"),
        "{}",
        none.err
    );
    let unknown = sandbox.skills(&["pull", "ghost"]);
    assert_eq!(unknown.code, 1);
}

#[test]
fn pull_refuses_edits_to_overlay_regions() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    fs::create_dir_all(source.join("overlays/hermes")).unwrap();
    fs::write(source.join("overlays/hermes/prepend.md"), "Hermes only.\n").unwrap();
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    let installed = install_copy(&sandbox, "alpha", "hermes");
    let text = fs::read_to_string(installed.join("SKILL.md")).unwrap();
    assert!(text.contains("Hermes only."));
    fs::write(
        installed.join("SKILL.md"),
        text.replace("Hermes only.", "Tampered."),
    )
    .unwrap();
    let run = sandbox.skills(&["pull", "alpha", "--target", "hermes"]);
    assert_eq!(run.code, 1);
    assert!(
        run.err.contains("body prepend region changed"),
        "{}",
        run.err
    );
    assert!(
        !sandbox
            .path("data/symbrain/skills/pending/hermes/alpha")
            .exists()
    );
}

#[test]
fn pull_apply_refuses_an_invalid_pending_tree() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    let installed = install_copy(&sandbox, "alpha", "hermes");
    let text = fs::read_to_string(installed.join("SKILL.md")).unwrap();
    fs::write(installed.join("SKILL.md"), format!("{text}more\n")).unwrap();
    assert_eq!(
        sandbox
            .skills(&["pull", "alpha", "--target", "hermes"])
            .code,
        0
    );
    let pending = sandbox.path("data/symbrain/skills/pending/hermes/alpha/SKILL.md");
    fs::write(&pending, "---\nname: wrong\ndescription: d\n---\nx\n").unwrap();
    let before = fs::read_to_string(sandbox.library("alpha/SKILL.md")).unwrap();
    let run = sandbox.skills(&["pull", "alpha", "--target", "hermes", "--apply"]);
    assert_eq!(run.code, 1);
    assert!(run.err.contains("fails validation"), "{}", run.err);
    assert_eq!(
        fs::read_to_string(sandbox.library("alpha/SKILL.md")).unwrap(),
        before
    );
}

#[test]
fn install_and_render_cover_targets_modes_and_errors() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );

    let dry = sandbox.skills(&[
        "install",
        "--dry-run",
        "--target",
        "claude",
        "--json",
        "alpha",
    ]);
    assert_eq!(dry.code, 0, "{}", dry.err);
    assert_eq!(dry.json()["action"], "planned");
    assert!(!sandbox.path("home/.claude/skills/alpha").exists());

    let many = sandbox.skills(&["install", "--target", "claude,codex", "--json", "alpha"]);
    assert_eq!(many.code, 0, "{}", many.err);
    assert_eq!(many.json().as_array().unwrap().len(), 2);
    let link = sandbox.path("home/.claude/skills/alpha");
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        sandbox
            .skills(&["status", "--target", "claude", "--json"])
            .json()["summary"]["in_sync"],
        1
    );

    let log = sandbox.skills(&["log", "--skill", "alpha", "--target", "claude", "--json"]);
    assert!(log.out.contains("\"event\":\"install\""), "{}", log.out);

    let rendered = sandbox.skills(&[
        "render", "--target", "codex", "--json", "-o", "out", "alpha",
    ]);
    assert_eq!(rendered.code, 0, "{}", rendered.err);
    assert!(sandbox.path("project/out/codex/alpha/SKILL.md").is_file());
    assert_eq!(rendered.json()[0]["frontmatter"]["compatibility"], "codex");

    for (args, code) in [
        (vec!["install", "--target", "nowhere", "alpha"], 2),
        (vec!["install", "--mode", "bogus", "alpha"], 2),
        (vec!["install", "--scope", "galaxy", "alpha"], 2),
        (vec!["install", "--bogus", "alpha"], 2),
        (vec!["install"], 1),
        (vec!["install", "ghost"], 1),
        (vec!["pull"], 2),
        (vec!["history"], 2),
        (vec!["import"], 2),
        (vec!["render"], 2),
    ] {
        assert_eq!(sandbox.skills(&args).code, code, "{args:?}");
    }
    let help = sandbox.skills(&["pull", "--help"]);
    assert_eq!(help.code, 0);
    assert!(help.out.contains("--apply"));
}

#[test]
fn disabled_targets_are_skipped_by_all_target_installs() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    fs::create_dir_all(sandbox.path("config/symskills")).unwrap();
    fs::write(
        sandbox.path("config/symskills/config.toml"),
        "disabled_targets = [\"openclaw\", \"antigravity\"]\n",
    )
    .unwrap();
    let run = sandbox.skills(&["install", "--mode", "copy", "--json", "alpha"]);
    assert_eq!(run.code, 0, "{}", run.err);
    let rows = run.json();
    let targets: Vec<&str> = rows
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["target"].as_str().unwrap())
        .collect();
    assert!(
        !targets.contains(&"openclaw") && !targets.contains(&"antigravity"),
        "{targets:?}"
    );
    assert!(targets.contains(&"claude"));
}

#[test]
fn pull_carries_frontmatter_edits_and_keeps_unrendered_library_keys() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    fs::write(
        source.join("SKILL.md"),
        "---\nname: alpha\ndescription: Test skill\ncategory: dev\ntags: [one, two]\nmetadata:\n  owner: me\n  claude: only-for-claude\n---\n# Alpha\n",
    )
    .unwrap();
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    let installed = install_copy(&sandbox, "alpha", "hermes");
    let text = fs::read_to_string(installed.join("SKILL.md")).unwrap();
    assert!(
        !text.contains("tags"),
        "renderer unexpectedly emits tags: {text}"
    );
    let edited = text
        .replace("category: dev\n", "category: ops\nallowed-tools:\n- Bash\n")
        .replace("owner: me", "owner: you");
    fs::write(installed.join("SKILL.md"), edited).unwrap();

    let plan = sandbox.skills(&["pull", "alpha", "--target", "hermes", "--json", "--dry-run"]);
    assert_eq!(plan.code, 0, "{}", plan.err);
    let keys: Vec<String> = plan.json()["frontmatter_changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            format!(
                "{}:{}",
                c["key"].as_str().unwrap(),
                c["reason"].as_str().unwrap()
            )
        })
        .collect();
    assert_eq!(
        keys,
        [
            "allowed-tools:permission-relevant frontmatter",
            "category:frontmatter",
            "metadata.owner:frontmatter"
        ]
    );
    assert_eq!(
        sandbox
            .skills(&["pull", "alpha", "--target", "hermes"])
            .code,
        0
    );
    let applied = sandbox.skills(&["pull", "alpha", "--target", "hermes", "--apply"]);
    assert_eq!(applied.code, 0, "{}", applied.err);
    let library = fs::read_to_string(sandbox.library("alpha/SKILL.md")).unwrap();
    for needle in [
        "category: ops",
        "allowed-tools",
        "- Bash",
        "owner: you",
        "claude: only-for-claude",
        "tags:",
    ] {
        assert!(library.contains(needle), "{needle} missing: {library}");
    }
    assert!(!library.contains("compatibility"), "{library}");
}

#[test]
fn pull_refuses_when_library_and_harness_changed_the_same_file() {
    let sandbox = Sandbox::new();
    let source = sandbox.source("alpha", "# Alpha");
    assert_eq!(
        sandbox.skills(&["import", source.to_str().unwrap()]).code,
        0
    );
    let installed = install_copy(&sandbox, "alpha", "hermes");
    fs::write(sandbox.library("alpha/references/r.md"), "library side\n").unwrap();
    fs::write(installed.join("references/r.md"), "harness side\n").unwrap();
    let run = sandbox.skills(&["pull", "alpha", "--target", "hermes"]);
    assert_eq!(run.code, 1);
    assert!(
        run.err.contains("conflict in references/r.md"),
        "{}",
        run.err
    );
    assert_eq!(
        fs::read_to_string(sandbox.library("alpha/references/r.md")).unwrap(),
        "library side\n"
    );
}

#[test]
fn install_profile_installs_every_linked_skill_with_alias() {
    let sandbox = Sandbox::new();
    for name in ["alpha", "beta"] {
        let source = sandbox.source(name, "# Body");
        assert_eq!(
            sandbox.skills(&["import", source.to_str().unwrap()]).code,
            0
        );
    }
    fs::create_dir_all(sandbox.path("config/symskills/profiles")).unwrap();
    fs::write(
        sandbox.path("config/symskills/profiles/dev.toml"),
        "[links.first]\nskill = \"alpha\"\nalias = \"alpha-renamed\"\n\n[links.second]\nskill = \"beta\"\n",
    )
    .unwrap();
    let run = sandbox.skills(&[
        "install",
        "--profile",
        "dev",
        "--target",
        "codex",
        "--mode",
        "copy",
    ]);
    assert_eq!(run.code, 0, "{} {}", run.out, run.err);
    assert!(
        sandbox
            .path("home/.agents/skills/alpha-renamed/SKILL.md")
            .is_file()
    );
    assert!(sandbox.path("home/.agents/skills/beta/SKILL.md").is_file());
    let missing = sandbox.skills(&["install", "--profile", "nope", "--target", "codex"]);
    assert_eq!(missing.code, 1);
    assert!(
        missing.err.contains("not found or has no links"),
        "{}",
        missing.err
    );
}
