use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;

struct Fixture {
    _temp: TempDir,
    home: PathBuf,
    project: PathBuf,
    render: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let fixture = Self {
            home: root.join("home"),
            project: root.join("project"),
            render: root.join("render"),
            _temp: temp,
        };
        for dir in [&fixture.home, &fixture.project, &fixture.render] {
            fs::create_dir_all(dir).unwrap();
        }
        fixture
    }

    fn options<'a>(&'a self, window: Option<usize>, disabled: &'a [String]) -> CatalogOptions<'a> {
        CatalogOptions {
            home_dir: &self.home,
            project_dir: &self.project,
            render_dir: &self.render,
            context_window_tokens: window,
            disabled_targets: disabled,
        }
    }

    fn report(&self, window: Option<usize>) -> CatalogReport {
        catalog_report(&self.options(window, &[]))
    }

    fn rendered(&self, target: &str, name: &str, description: &str) {
        write_skill(&self.render.join(target).join(name), name, description);
    }
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {description}\n---\nbody\n"),
    )
    .unwrap();
}

fn target<'a>(report: &'a CatalogReport, name: &str) -> &'a TargetCatalog {
    report
        .targets
        .iter()
        .find(|row| row.target == name)
        .unwrap()
}

#[test]
fn budget_is_reported_over_and_under_with_both_numbers() {
    let fixture = Fixture::new();
    // Nine 1,000-char descriptions: 9,000 chars against Claude's 8,000 default.
    for index in 0..9 {
        fixture.rendered("claude", &format!("skill-{index}"), &"d".repeat(1000));
    }
    let over = fixture.report(None);
    let claude = target(&over, "claude");
    assert_eq!(claude.skill_count, 9);
    assert_eq!(claude.description_chars, 9000);
    assert_eq!(claude.listing_budget_chars, Some(8000));
    assert_eq!(
        (claude.over_budget, claude.over_by),
        (Some(true), Some(1000))
    );
    assert_eq!(
        claude.budget_harness_version.as_deref(),
        Some("claude-code 2.1.325")
    );
    assert_eq!(over.context_window_source, "default");
    assert_eq!(over.context_window_tokens, 200_000);

    // A 1M-token window raises the budget to 40,000 chars: under budget.
    let under = fixture.report(Some(1_000_000));
    let claude = target(&under, "claude");
    assert_eq!(claude.listing_budget_chars, Some(40_000));
    assert_eq!((claude.over_budget, claude.over_by), (Some(false), Some(0)));
    assert_eq!(under.context_window_source, "flag");

    // Targets without a profiled budget report none instead of guessing.
    let antigravity = target(&over, "antigravity");
    assert_eq!(antigravity.listing_budget_chars, None);
    assert_eq!(antigravity.over_budget, None);
    // Codex uses its own 2% rule; its catalog here is empty.
    assert_eq!(target(&over, "codex").listing_budget_chars, Some(16_000));
}

#[test]
fn same_named_user_and_project_copies_are_listed_with_managed_state() {
    let fixture = Fixture::new();
    let user = fixture.home.join(".claude/skills/shared");
    let project = fixture.project.join(".claude/skills/shared");
    write_skill(&user, "shared", "user copy");
    write_skill(&project, "shared", "project copy");
    fs::write(user.join(".symskills.json"), "{}").unwrap();
    write_skill(
        &fixture.home.join(".claude/skills/unique"),
        "unique",
        "only one",
    );

    let report = fixture.report(None);
    let claude = target(&report, "claude");
    assert_eq!(claude.duplicates.len(), 1);
    let dup = &claude.duplicates[0];
    assert_eq!(dup.name, "shared");
    assert_eq!(dup.harness_uses, "unknown");
    let copies: Vec<(&str, &str, bool)> = dup
        .copies
        .iter()
        .map(|copy| (copy.scope.as_str(), copy.path.as_str(), copy.managed))
        .collect();
    assert_eq!(
        copies,
        [
            ("user", user.to_str().unwrap(), true),
            ("project", project.to_str().unwrap(), false),
        ]
    );
    // Codex does not read `.claude/skills`, so it sees no collision.
    assert!(target(&report, "codex").duplicates.is_empty());
    // OpenCode reads Claude-compatible roots as external discovery roots.
    assert_eq!(target(&report, "opencode").duplicates[0].copies.len(), 2);
}

#[test]
fn external_roots_and_symlinked_copies_are_handled() {
    let fixture = Fixture::new();
    // OpenCode also reads ~/.claude/skills: a same-named skill there collides.
    write_skill(
        &fixture.home.join(".config/opencode/skills/dup"),
        "dup",
        "native",
    );
    write_skill(&fixture.home.join(".claude/skills/dup"), "dup", "foreign");
    // A symlink to an already-seen directory is the same copy, not a duplicate.
    #[cfg(unix)]
    {
        let native = fixture.home.join(".config/opencode/skills/linked");
        write_skill(&native, "linked", "native");
        std::os::unix::fs::symlink(&native, fixture.home.join(".claude/skills/linked")).unwrap();
    }
    let report = fixture.report(None);
    let opencode = target(&report, "opencode");
    assert_eq!(opencode.duplicates.len(), 1);
    assert_eq!(opencode.duplicates[0].name, "dup");
    let scopes: Vec<&str> = opencode.duplicates[0]
        .copies
        .iter()
        .map(|copy| copy.scope.as_str())
        .collect();
    assert_eq!(scopes, ["user", "external"]);
    assert!(target(&report, "claude").duplicates.is_empty());
}

#[test]
fn hermes_categories_are_descended_and_unmanaged_copies_found() {
    let fixture = Fixture::new();
    write_skill(
        &fixture.home.join(".hermes/skills/symaira/tool"),
        "tool",
        "managed one",
    );
    fs::write(
        fixture
            .home
            .join(".hermes/skills/symaira/tool/.symskills.json"),
        "{}",
    )
    .unwrap();
    write_skill(
        &fixture.home.join(".hermes/skills/devops/tool"),
        "tool",
        "unmanaged one",
    );
    let report = fixture.report(None);
    let dup = &target(&report, "hermes").duplicates[0];
    let managed: Vec<bool> = dup.copies.iter().map(|copy| copy.managed).collect();
    assert_eq!((dup.name.as_str(), managed), ("tool", vec![true, false]));
}

#[test]
fn lookalike_descriptions_are_paired_and_unrelated_are_not() {
    let fixture = Fixture::new();
    let near = "Use when reviewing pull requests for security flaws in rust code before merge";
    fixture.rendered("codex", "review-a", near);
    fixture.rendered("codex", "review-b", &format!("{near} today"));
    fixture.rendered(
        "codex",
        "budget",
        "Draft quarterly budget spreadsheets with currency conversion",
    );
    let report = fixture.report(None);
    let pairs = &target(&report, "codex").lookalikes;
    assert_eq!(pairs.len(), 1);
    assert_eq!(
        (pairs[0].a.as_str(), pairs[0].b.as_str()),
        ("review-a", "review-b")
    );
    assert!(pairs[0].similarity_permille >= report.lookalike_threshold_permille);
    assert!(target(&report, "claude").lookalikes.is_empty());
}

#[test]
fn report_is_deterministic_and_skips_disabled_targets() {
    let fixture = Fixture::new();
    for index in 0..5 {
        fixture.rendered(
            "claude",
            &format!("s{index}"),
            &format!("Skill number {index} does work"),
        );
    }
    let first = serde_json::to_string(&fixture.report(None)).unwrap();
    let second = serde_json::to_string(&fixture.report(None)).unwrap();
    assert_eq!(first, second);

    let disabled = vec!["claude".to_owned(), "hermes".to_owned()];
    let report = catalog_report(&fixture.options(None, &disabled));
    let names: Vec<&str> = report
        .targets
        .iter()
        .map(|row| row.target.as_str())
        .collect();
    assert_eq!(names, ["opencode", "codex", "antigravity", "openclaw"]);
}
