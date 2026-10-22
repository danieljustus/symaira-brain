#!/usr/bin/env python3
"""Record or verify the Go oracle for the five versioning/profile Skills tools (#764 slice b).

Covers `skills_profile_list`, `skills_profile_resolve`, `skills_discover_sources`,
`skills_history` and `skills_restore`.  `record` drives the real Go `symbrain mcp`
binary (built from the pinned ref's *unmodified* Go sources) over every case in an
isolated HOME/XDG tree and stores, per case:

  * the exact JSON-RPC reply lines (byte-exact text, with the temp root replaced by
    `<ROOT>` and discovery `source_id` hashes replaced by `<sid:REL>`), and
  * the filesystem side effects: a snapshot of every file, symlink and directory under
    the isolated root after the calls, plus `git log`/`status` of every skill repository
    (history/restore write state!).

The committed fixture (`rust/symbrain-cli/tests/fixtures/skills_tools_go_oracle.json`)
is replayed by the Rust integration test `skills_tools_oracle`, which materializes the
same pre-state, runs the native `symbrain mcp` and compares replies and snapshots.
Nothing in the fixture is hand-written: every expected value is a Go observation.

    python3 scripts/skills-native-oracle/tools_oracle.py record --ref <commit> [--go-binary PATH]
    python3 scripts/skills-native-oracle/tools_oracle.py check  [--go-binary PATH]
"""
import argparse
import base64
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
FIXTURE = ROOT / "rust/symbrain-cli/tests/fixtures/skills_tools_go_oracle.json"
GO_SOURCES = ("cmd", "internal", "go.mod", "go.sum")
# The oracle is toolchain-sensitive (go.mod pins 1.26.7; ambient 1.26.10 changes behaviour): pin it everywhere.
GOTOOLCHAIN = "go1.26.7"
os.environ["GOTOOLCHAIN"] = GOTOOLCHAIN

BOOT = [
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"tools-oracle","version":"0"}}}',
    '{"jsonrpc":"2.0","method":"notifications/initialized"}',
    '{"jsonrpc":"2.0","id":2,"method":"tools/list"}',
]
TOOLS = ("skills_profile_list", "skills_profile_resolve", "skills_discover_sources",
         "skills_history", "skills_restore")
# Pinned so commit hashes, %aI timestamps and Go-made restore commits are reproducible. A non-zero offset keeps
# `%aI` identical across git versions (older git prints +00:00 where newer git prints Z for UTC).
GIT_ENV = {
    "GIT_AUTHOR_NAME": "Oracle Author", "GIT_AUTHOR_EMAIL": "author@example.invalid",
    "GIT_COMMITTER_NAME": "Oracle Author", "GIT_COMMITTER_EMAIL": "author@example.invalid",
    "GIT_AUTHOR_DATE": "2026-01-02T03:04:05+0200", "GIT_COMMITTER_DATE": "2026-01-02T03:04:05+0200",
    "GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": "/dev/null",
}
# Tool-level PATH: `git` must resolve (the slice-(a) corpus runs with PATH="").
PATH_WITH_GIT = "/usr/bin:/bin"

# Gateway-level state every `symbrain mcp` run creates regardless of the tool called (memory store, JWT
# secret, audit and recipe logs). It is not a Skills-tool effect, so it is excluded from side-effect snapshots.
# Parent directories those files imply (created by Go even when nothing else is written there).
GATEWAY_PARENTS = ("config/symbrain", "data/symbrain")
# Lock files the install transaction leaves behind. Go and the shared Rust install layer name them
# differently (`.symskills-lock-<name>` vs `.symskills-lock-<sha256>`); documented residual, not compared.
LOCK_PREFIX = ".symskills-lock-"
GATEWAY_STATE = ("data/symbrain/memory", "config/symbrain/memory", "data/symbrain/audit", "data/symbrain/recipes")

LIB = "data/symbrain/skills/library"
PROFILES = "config/symskills/profiles"


def skill_md(name, description="A demo skill.", extra="", body="# Body\n"):
    return f"---\nname: {name}\ndescription: {description}\n{extra}---\n{body}"


def call(i, name, arguments=None, raw_arguments=None):
    args = "" if arguments is None and raw_arguments is None else (
        ',"arguments":' + (raw_arguments if raw_arguments is not None else json.dumps(arguments, separators=(",", ":"))))
    return f'{{"jsonrpc":"2.0","id":{i},"method":"tools/call","params":{{"name":"{name}"{args}}}}}'


def toml_profile(name, description="", inherits=(), links=()):
    lines = [f'name = "{name}"']
    if description:
        lines.append(f'description = "{description}"')
    if inherits:
        lines.append("inherits = [" + ", ".join(f'"{i}"' for i in inherits) + "]")
    for link, skill, alias in links:
        lines.append(f"[links.{link}]")
        lines.append(f'skill = "{skill}"')
        if alias:
            lines.append(f'alias = "{alias}"')
    return "\n".join(lines) + "\n"


def versioned_alpha():
    """Three-commit history: import, update, hand-made; files differ between revisions."""
    return {
        "dir": f"{LIB}/alpha",
        "files": {"SKILL.md": skill_md("alpha", "Alpha v1."), "notes.md": "first\n"},
        "commits": [
            ("import: initial library import", {}),
            ("update: alpha description", {"SKILL.md": skill_md("alpha", "Alpha v2."), "extra.txt": "x\n"}),
            ("manual tweak without operation prefix", {"notes.md": "second\n"}),
        ],
    }


def cases():
    out = []

    def case(cid, requests, files=None, repos=(), env=None, pre_go=None, symlinks=(), dirs=(), install=()):
        out.append({"id": cid, "requests": requests, "files": files or {}, "repos": list(repos),
                    "env": env or {}, "pre_go": pre_go or [], "symlinks": list(symlinks), "dirs": list(dirs),
                    "install": list(install)})

    # --- skills_profile_list ---
    case("profile-list-no-dir", [call(3, "skills_profile_list", {})])
    case("profile-list-absent-arguments", [call(3, "skills_profile_list")])
    case("profile-list-null-arguments", [call(3, "skills_profile_list", raw_arguments="null")])
    case("profile-list-empty-dir", [call(3, "skills_profile_list", {})], dirs=[PROFILES])
    prof_files = {
        f"{PROFILES}/work.toml": toml_profile("work", "Work context", links=[("a", "alpha", ""), ("b", "beta", "bee")]),
        f"{PROFILES}/base.toml": toml_profile("base", links=[("a", "alpha", "")]),
        f"{PROFILES}/child.toml": toml_profile("child", "Child <&> context", inherits=["base"], links=[("c", "beta", "")]),
        f"{PROFILES}/ignored.txt": "not a profile\n",
        f"{PROFILES}/subdir.toml/keep": "directory with .toml name\n",
    }
    case("profile-list-populated", [call(3, "skills_profile_list", {})], files=prof_files)
    case("profile-list-ignores-arguments", [call(3, "skills_profile_list", {"anything": [1, 2]})], files=prof_files)
    case("profile-list-malformed-toml", [call(3, "skills_profile_list", {})],
         files={f"{PROFILES}/aaa.toml": toml_profile("aaa"), f"{PROFILES}/zbad.toml": "name = \n"})
    case("profile-list-description-only-unicode", [call(3, "skills_profile_list", {})],
         files={f"{PROFILES}/uni.toml": toml_profile("uni", "Größe — 日本語")})
    case("profile-list-wrong-type-field", [call(3, "skills_profile_list", {})],
         files={f"{PROFILES}/typed.toml": 'name = 4\n'})

    # --- skills_profile_resolve ---
    lib = {f"{LIB}/alpha/SKILL.md": skill_md("alpha"), f"{LIB}/beta/SKILL.md": skill_md("beta")}
    rfiles = {**prof_files, **lib,
              f"{PROFILES}/missing.toml": toml_profile("missing", links=[("a", "alpha", ""), ("g", "ghost", "")]),
              f"{PROFILES}/emptyskill.toml": toml_profile("emptyskill", links=[("e", "", "")]),
              f"{PROFILES}/cycle-a.toml": toml_profile("cycle-a", inherits=["cycle-b"]),
              f"{PROFILES}/cycle-b.toml": toml_profile("cycle-b", inherits=["cycle-a"]),
              f"{PROFILES}/orphan.toml": toml_profile("orphan", inherits=["nonexistent"]),
              f"{PROFILES}/emptyinh.toml": 'name = "emptyinh"\ninherits = [""]\n',
              f"{PROFILES}/badinh.toml": 'name = "badinh"\ninherits = ["Bad Name"]\n',
              f"{PROFILES}/diffname.toml": toml_profile("declared", links=[("x", "alpha", "")]),
              f"{PROFILES}/broken.toml": "name = \n"}
    for n in ("work", "child", "base", "missing", "emptyskill", "cycle-a", "orphan", "emptyinh", "badinh", "diffname", "broken"):
        case(f"profile-resolve-{n}", [call(3, "skills_profile_resolve", {"name": n})], files=rfiles)
    case("profile-resolve-unknown", [call(3, "skills_profile_resolve", {"name": "nope"})], files=rfiles)
    case("profile-resolve-no-profiles-dir", [call(3, "skills_profile_resolve", {"name": "work"})])
    case("profile-resolve-invalid-name-space", [call(3, "skills_profile_resolve", {"name": "bad name"})], files=rfiles)
    case("profile-resolve-invalid-name-upper", [call(3, "skills_profile_resolve", {"name": "Work"})], files=rfiles)
    case("profile-resolve-invalid-name-traversal", [call(3, "skills_profile_resolve", {"name": "../work"})], files=rfiles)
    case("profile-resolve-empty-name", [call(3, "skills_profile_resolve", {"name": ""})], files=rfiles)
    case("profile-resolve-missing-name", [call(3, "skills_profile_resolve", {})], files=rfiles)
    case("profile-resolve-null-name", [call(3, "skills_profile_resolve", {"name": None})], files=rfiles)
    case("profile-resolve-name-number", [call(3, "skills_profile_resolve", {"name": 4})], files=rfiles)
    case("profile-resolve-name-array", [call(3, "skills_profile_resolve", {"name": ["work"]})], files=rfiles)
    case("profile-resolve-case-folded-key", [call(3, "skills_profile_resolve", {"NAME": "work"})], files=rfiles)
    case("profile-resolve-extra-key", [call(3, "skills_profile_resolve", {"name": "work", "extra": 1})], files=rfiles)
    case("profile-resolve-arguments-array", [call(3, "skills_profile_resolve", raw_arguments="[]")], files=rfiles)
    case("profile-resolve-arguments-string", [call(3, "skills_profile_resolve", raw_arguments='"work"')], files=rfiles)
    case("profile-resolve-arguments-null", [call(3, "skills_profile_resolve", raw_arguments="null")], files=rfiles)
    case("profile-resolve-absent-arguments", [call(3, "skills_profile_resolve")], files=rfiles)
    case("profile-resolve-duplicate-key-last-wins", [call(3, "skills_profile_resolve", raw_arguments='{"name":"nope","name":"work"}')], files=rfiles)

    # --- skills_history ---
    alpha = versioned_alpha()
    beta_plain = {f"{LIB}/beta/SKILL.md": skill_md("beta")}
    case("history-default-limit", [call(3, "skills_history", {"name": "alpha"})], repos=[alpha])
    case("history-limit-1", [call(3, "skills_history", {"name": "alpha", "limit": 1})], repos=[alpha])
    case("history-limit-2", [call(3, "skills_history", {"name": "alpha", "limit": 2})], repos=[alpha])
    case("history-limit-zero", [call(3, "skills_history", {"name": "alpha", "limit": 0})], repos=[alpha])
    case("history-limit-negative", [call(3, "skills_history", {"name": "alpha", "limit": -5})], repos=[alpha])
    case("history-limit-huge", [call(3, "skills_history", {"name": "alpha", "limit": 1000000})], repos=[alpha])
    case("history-limit-float", [call(3, "skills_history", {"name": "alpha", "limit": 1.5})], repos=[alpha])
    case("history-limit-string", [call(3, "skills_history", {"name": "alpha", "limit": "2"})], repos=[alpha])
    case("history-name-missing", [call(3, "skills_history", {})], repos=[alpha])
    case("history-name-empty", [call(3, "skills_history", {"name": ""})], repos=[alpha])
    case("history-name-number", [call(3, "skills_history", {"name": 7})], repos=[alpha])
    case("history-not-versioned", [call(3, "skills_history", {"name": "beta"})], repos=[alpha], files=beta_plain)
    case("history-unknown-skill", [call(3, "skills_history", {"name": "ghost"})], repos=[alpha])
    case("history-name-dot-library-root", [call(3, "skills_history", {"name": "."})], repos=[alpha])
    case("history-name-traversal", [call(3, "skills_history", {"name": "../library/alpha"})], repos=[alpha])
    case("history-empty-repo-no-commits", [call(3, "skills_history", {"name": "fresh"})],
         repos=[{"dir": f"{LIB}/fresh", "files": {}, "commits": []}])
    case("history-after-dirty-tree", [call(3, "skills_history", {"name": "alpha"})], repos=[alpha],
         files={f"{LIB}/alpha/untracked.md": "dirty\n"})
    case("history-git-unavailable", [call(3, "skills_history", {"name": "alpha"})], repos=[alpha], env={"PATH": ""})
    case("history-unicode-subject-and-file", [call(3, "skills_history", {"name": "uni"})],
         repos=[{"dir": f"{LIB}/uni", "files": {"SKILL.md": skill_md("uni"), "größe & <tag>.md": "x\n"},
                 "commits": [("import: Größe & <tag> — 日本語", {})]}])
    case("history-hand-commit-colon-forms", [call(3, "skills_history", {"name": "ops"})],
         repos=[{"dir": f"{LIB}/ops", "files": {"SKILL.md": skill_md("ops")},
                 "commits": [("import: a", {}), ("update:b", {"a": "1"}), ("restore: c", {"b": "1"}),
                             ("Update: d", {"c": "1"}), ("nocolon", {"d": "1"}), (": e", {"e": "1"})]}])

    # --- skills_restore ---
    def restore(rev, **kw):
        return call(3, "skills_restore", {"name": "alpha", "rev": rev, **kw})

    case("restore-dry-run-default-head1", [restore("HEAD~1")], repos=[alpha])
    case("restore-dry-run-explicit", [restore("HEAD~2", dry_run=True)], repos=[alpha])
    case("restore-dry-run-head-noop", [restore("HEAD")], repos=[alpha])
    case("restore-real-head2", [restore("HEAD~2", dry_run=False), call(4, "skills_history", {"name": "alpha"})], repos=[alpha])
    case("restore-real-head1", [restore("HEAD~1", dry_run=False)], repos=[alpha])
    case("restore-real-noop-same-tree", [restore("HEAD", dry_run=False)], repos=[alpha])
    case("restore-real-then-restore-back", [restore("HEAD~2", dry_run=False), restore("HEAD~1", dry_run=False),
                                            call(5, "skills_history", {"name": "alpha", "limit": 10})], repos=[alpha])
    case("restore-rev-unknown", [restore("deadbeef")], repos=[alpha])
    case("restore-rev-injection-looking", [restore("--help")], repos=[alpha])
    case("restore-rev-empty", [restore("")], repos=[alpha])
    case("restore-name-empty", [call(3, "skills_restore", {"name": "", "rev": "HEAD"})], repos=[alpha])
    case("restore-missing-args", [call(3, "skills_restore", {})], repos=[alpha])
    case("restore-not-versioned", [call(3, "skills_restore", {"name": "beta", "rev": "HEAD"})], repos=[alpha], files=beta_plain)
    case("restore-dirty-refused", [restore("HEAD~1", dry_run=False)], repos=[alpha], files={f"{LIB}/alpha/notes.md": "dirty edit\n"})
    case("restore-dirty-refused-dry-run", [restore("HEAD~1")], repos=[alpha], files={f"{LIB}/alpha/notes.md": "dirty edit\n"})
    case("restore-dirty-untracked-refused", [restore("HEAD~1", dry_run=False)], repos=[alpha], files={f"{LIB}/alpha/new.md": "new\n"})
    case("restore-dirty-allow-dry-run", [restore("HEAD~1", allow_dirty=True)], repos=[alpha], files={f"{LIB}/alpha/notes.md": "dirty edit\n"})
    case("restore-dirty-allow-real", [restore("HEAD~2", dry_run=False, allow_dirty=True), call(4, "skills_history", {"name": "alpha", "limit": 10})],
         repos=[alpha], files={f"{LIB}/alpha/notes.md": "dirty edit\n", f"{LIB}/alpha/new.md": "new\n"})
    case("restore-dry-run-false-null", [call(3, "skills_restore", {"name": "alpha", "rev": "HEAD~1", "dry_run": None})], repos=[alpha])
    case("restore-dry-run-wrong-type", [call(3, "skills_restore", {"name": "alpha", "rev": "HEAD~1", "dry_run": "false"})], repos=[alpha])
    case("restore-allow-dirty-wrong-type", [call(3, "skills_restore", {"name": "alpha", "rev": "HEAD", "allow_dirty": 1})], repos=[alpha])
    case("restore-rev-wrong-type", [call(3, "skills_restore", {"name": "alpha", "rev": 5})], repos=[alpha])
    case("restore-name-traversal", [call(3, "skills_restore", {"name": "../library/alpha", "rev": "HEAD~1", "dry_run": False})], repos=[alpha])
    case("restore-git-unavailable", [restore("HEAD~1", dry_run=False)], repos=[alpha], env={"PATH": ""})
    # Restore to a revision whose tree is not a valid skill.
    bad_rev = {"dir": f"{LIB}/frag", "files": {"SKILL.md": skill_md("frag")},
               "commits": [("import: ok", {}), ("update: break frontmatter", {"SKILL.md": "no frontmatter at all\n"}),
                           ("update: fixed again", {"SKILL.md": skill_md("frag", "fixed")})]}
    case("restore-to-invalid-frontmatter", [call(3, "skills_restore", {"name": "frag", "rev": "HEAD~1", "dry_run": False})], repos=[bad_rev])
    name_mismatch = {"dir": f"{LIB}/mism", "files": {"SKILL.md": skill_md("other-name")},
                     "commits": [("import: wrong name", {}), ("update: right name", {"SKILL.md": skill_md("mism")})]}
    case("restore-to-name-mismatch", [call(3, "skills_restore", {"name": "mism", "rev": "HEAD~1", "dry_run": False})], repos=[name_mismatch])
    case("restore-with-subdir-and-exec-mode", [call(3, "skills_restore", {"name": "tree", "rev": "HEAD~1", "dry_run": False})],
         repos=[{"dir": f"{LIB}/tree", "files": {"SKILL.md": skill_md("tree"), "refs/guide.md": "g\n"},
                 "commits": [("import: with refs", {}), ("update: drop refs", {"refs/guide.md": None, "gone.md": "g\n"})]}])
    case("restore-remove-extra-files", [call(3, "skills_restore", {"name": "alpha", "rev": "HEAD~2", "dry_run": False})], repos=[alpha])

    # --- skills_discover_sources ---
    home_skills = {
        "home/.claude/skills/dsk-one/SKILL.md": skill_md("dsk-one"),
        "home/.claude/skills/dsk-managed/SKILL.md": skill_md("dsk-managed"),
        "home/.claude/skills/dsk-managed/.symskills.json": "{}\n",
        "home/.claude/skills/dsk-bad/SKILL.md": "no frontmatter\n",
        "home/.claude/skills/not-a-skill/readme.txt": "x\n",
        "home/.agents/skills/dsk-codex/SKILL.md": skill_md("dsk-codex"),
    }
    case("discover-empty-home", [call(3, "skills_discover_sources", {})])
    case("discover-harness-roots", [call(3, "skills_discover_sources", {})], files=home_skills)
    case("discover-null-paths", [call(3, "skills_discover_sources", {"paths": None})], files=home_skills)
    case("discover-explicit-skill-dir", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/one"]})],
         files={"ext/one/SKILL.md": skill_md("one", "ext one")})
    case("discover-explicit-parent-dir", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext"]})],
         files={"ext/one/SKILL.md": skill_md("one", "ext one"), "ext/two/SKILL.md": skill_md("two", "ext two"),
                "ext/plain/readme": "x\n"})
    case("discover-explicit-missing", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/does-not-exist"]})])
    case("discover-explicit-file", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/file.txt"]})], files={"ext/file.txt": "x\n"})
    case("discover-explicit-empty-string", [call(3, "skills_discover_sources", {"paths": [""]})])
    case("discover-explicit-relative", [call(3, "skills_discover_sources", {"paths": ["rel/skill"]})],
         files={"project/rel/skill/SKILL.md": skill_md("skill", "relative")})
    case("discover-explicit-duplicate-same-dir", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/one", "<ROOT>/ext/one"]})],
         files={"ext/one/SKILL.md": skill_md("one")})
    case("discover-explicit-invalid-validation-error", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/nm"]})],
         files={"ext/nm/SKILL.md": skill_md("Bad Name!", "x")})
    case("discover-explicit-validation-warning", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/warn"]})],
         files={"ext/warn/SKILL.md": skill_md("warn", "desc", extra="category: Tools\n")})
    case("discover-explicit-garbage-frontmatter", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/g"]})],
         files={"ext/g/SKILL.md": "---\nname: [unclosed\n---\n"})
    case("discover-explicit-symlinked-skill", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/link"]})],
         files={"real/s/SKILL.md": skill_md("s", "real")}, symlinks=[("ext/link", "../real/s")])
    case("discover-explicit-managed-marker", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/m"]})],
         files={"ext/m/SKILL.md": skill_md("m"), "ext/m/.symskills.json": "{}\n"})
    case("discover-paths-wrong-type", [call(3, "skills_discover_sources", {"paths": "x"})])
    case("discover-paths-element-wrong-type", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext", 5]})], files={"ext/a/SKILL.md": skill_md("a")})
    case("discover-paths-null-element", [call(3, "skills_discover_sources", {"paths": [None, "<ROOT>/ext"]})], files={"ext/a/SKILL.md": skill_md("a")})
    case("discover-scope-wrong-type", [call(3, "skills_discover_sources", {"scope": 3})])
    case("discover-scope-project", [call(3, "skills_discover_sources", {"scope": "project"})], files=home_skills)
    case("discover-scope-other-string", [call(3, "skills_discover_sources", {"scope": "global"})], files=home_skills)
    case("discover-explicit-and-harness-overlap", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/home/.claude/skills"]})], files=home_skills)
    case("discover-explicit-unicode-dir", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/größe"]})],
         files={"ext/größe/SKILL.md": skill_md("grosse", "Größe — 日本語 & <tag>")})
    case("discover-unreadable-dir", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/locked"]})],
         files={"ext/locked/keep": "x\n"}, pre_go=[("chmod", "ext/locked", 0)])

    # --- argument decoding corners (typed json.Unmarshal into anonymous structs) ---
    case("history-name-html-chars", [call(3, "skills_history", {"name": "a<b>&c"})], repos=[alpha])
    case("history-arguments-array", [call(3, "skills_history", raw_arguments="[]")], repos=[alpha])
    case("history-arguments-string", [call(3, "skills_history", raw_arguments='"x"')], repos=[alpha])
    case("history-arguments-null", [call(3, "skills_history", raw_arguments="null")], repos=[alpha])
    case("history-absent-arguments", [call(3, "skills_history")], repos=[alpha])
    case("history-first-error-wins-input-order-a", [call(3, "skills_history", raw_arguments='{"limit":"x","name":1}')], repos=[alpha])
    case("history-first-error-wins-input-order-b", [call(3, "skills_history", raw_arguments='{"name":1,"limit":"x"}')], repos=[alpha])
    case("history-duplicate-invalid-first", [call(3, "skills_history", raw_arguments='{"name":4,"name":"alpha"}')], repos=[alpha])
    case("history-duplicate-valid-first", [call(3, "skills_history", raw_arguments='{"name":"alpha","name":"ghost"}')], repos=[alpha])
    case("history-case-folded-keys", [call(3, "skills_history", raw_arguments='{"NAME":"alpha","Limit":1}')], repos=[alpha])
    case("history-limit-int-overflow", [call(3, "skills_history", raw_arguments='{"name":"alpha","limit":99999999999999999999}')], repos=[alpha])
    case("history-limit-exponent", [call(3, "skills_history", raw_arguments='{"name":"alpha","limit":1e2}')], repos=[alpha])
    case("history-lone-surrogate-name", [call(3, "skills_history", raw_arguments='{"name":"\\ud800"}')], repos=[alpha])
    case("restore-arguments-number", [call(3, "skills_restore", raw_arguments="5")])
    case("restore-arguments-bool", [call(3, "skills_restore", raw_arguments="true")])
    case("restore-absent-arguments", [call(3, "skills_restore")])
    case("restore-multiple-type-errors-first-wins", [call(3, "skills_restore", raw_arguments='{"sync":1,"dry_run":"no","name":"alpha","rev":"HEAD"}')], repos=[alpha])
    case("restore-sync-wrong-type", [call(3, "skills_restore", {"name": "alpha", "rev": "HEAD", "sync": "yes"})], repos=[alpha])
    case("discover-arguments-number", [call(3, "skills_discover_sources", raw_arguments="5")])
    case("discover-arguments-bool", [call(3, "skills_discover_sources", raw_arguments="true")])
    case("discover-absent-arguments", [call(3, "skills_discover_sources")])
    case("discover-arguments-null", [call(3, "skills_discover_sources", raw_arguments="null")], files=home_skills)
    case("discover-paths-nested-array-element", [call(3, "skills_discover_sources", raw_arguments='{"paths":[["x"]]}')])
    case("discover-paths-empty-array", [call(3, "skills_discover_sources", {"paths": []})], files=home_skills)
    case("discover-explicit-dot-dot-path", [call(3, "skills_discover_sources", {"paths": ["<ROOT>/ext/../ext/one/"]})],
         files={"ext/one/SKILL.md": skill_md("one")})
    case("profile-list-arguments-invalid-type", [call(3, "skills_profile_list", raw_arguments="5")], files=prof_files)

    # --- skills_restore sync=true: re-install stale targets (side effects in the harness root) ---
    case("restore-sync-no-installs", [restore("HEAD~2", dry_run=False, sync=True)], repos=[alpha])
    case("restore-sync-stale-target", [restore("HEAD~2", dry_run=False, sync=True)], repos=[alpha], install=[("alpha", "opencode")])
    case("restore-sync-dry-run-does-not-sync", [restore("HEAD~2", sync=True)], repos=[alpha], install=[("alpha", "opencode")])
    case("restore-no-sync-leaves-target-stale", [restore("HEAD~2", dry_run=False)], repos=[alpha], install=[("alpha", "opencode")])
    case("restore-sync-two-targets", [restore("HEAD~2", dry_run=False, sync=True)], repos=[alpha],
         install=[("alpha", "opencode"), ("alpha", "claude")])
    case("restore-sync-harness-changed-skipped", [restore("HEAD~2", dry_run=False, sync=True)], repos=[alpha],
         install=[("alpha", "opencode")], pre_go=[("edit", "home/.config/opencode/skills/alpha/SKILL.md", "hand edited in harness\n")])
    return out


# ---------------------------------------------------------------- materialization

def _decode(value):
    return value if isinstance(value, bytes) else value.encode()


def git(args, cwd, env):
    return subprocess.run(["git", *args], cwd=cwd, env=env, capture_output=True, check=True).stdout


def materialize(root, case):
    """Create the case's pre-state under `root`.  Mirrors rust/symbrain-cli/tests/skills_tools_oracle.rs."""
    root = Path(root)
    for name in ("home", "config", "data", "cache", "project", "tmp"):
        (root / name).mkdir(exist_ok=True)
    for rel in case["dirs"]:
        (root / rel).mkdir(parents=True, exist_ok=True)
    env = {"PATH": PATH_WITH_GIT, "HOME": str(root / "home"), **GIT_ENV}
    for repo in case["repos"]:
        base = root / repo["dir"]
        base.mkdir(parents=True, exist_ok=True)
        git(["init", "-q", "-b", "main"], base, env)
        for rel, text in repo["files"].items():
            (base / rel).parent.mkdir(parents=True, exist_ok=True)
            (base / rel).write_text(text)
        if repo["commits"]:
            git(["add", "-A"], base, env)
            git(["commit", "-q", "-m", repo["commits"][0][0]], base, env)
        for message, changes in repo["commits"][1:]:
            for rel, text in changes.items():
                target = base / rel
                if text is None:
                    target.unlink()
                else:
                    target.parent.mkdir(parents=True, exist_ok=True)
                    target.write_text(text)
            git(["add", "-A"], base, env)
            git(["commit", "-q", "-m", message], base, env)
    for rel, text in case["files"].items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text.replace("<ROOT>", str(root)))
    for rel, target in case["symlinks"]:
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        os.symlink(target.replace("<ROOT>", str(root)), path)
    for step in case["pre_go"]:
        if step[0] == "chmod":
            os.chmod(root / step[1], step[2])
        else:
            assert step[0] == "edit"
            (root / step[1]).write_text(step[2])


def fix_requests(requests, root):
    return [r.replace("<ROOT>", str(root)) for r in requests]


def server_env(root, case):
    env = {"PATH": PATH_WITH_GIT, "HOME": root / "home", "USERPROFILE": root / "home", "XDG_CONFIG_HOME": root / "config",
           "XDG_DATA_HOME": root / "data", "XDG_CACHE_HOME": root / "cache", "TMPDIR": root / "tmp", **GIT_ENV}
    env.update(case["env"])
    return {k: str(v) for k, v in env.items()}


def profile_file(path):
    path.write_text('[profile]\nname = "tools-oracle"\n[servers.vault]\nenabled = false\n[servers.memory]\n'
                    'enabled = false\n[servers.usage]\nenabled = false\n[servers.skills]\nenabled = true\n'
                    '[audit]\nenabled = false\n')


def run_server(binary, root, case):
    """One server process per request: the Go server dispatches concurrently, so sequencing needs sessions."""
    profile = root / "profile.toml"
    profile_file(profile)
    replies, tools_list = [], None
    for request in fix_requests(case["requests"], root):
        stdin = ("\n".join([*BOOT, request]) + "\n").encode()
        done = subprocess.run([str(binary), "mcp", "--profile-file", str(profile)], input=stdin, capture_output=True,
                              cwd=root / "project", env=server_env(root, case), timeout=60, check=False)
        assert done.returncode == 0, (case["id"], done)
        assert done.stderr == b"", (case["id"], done.stderr)
        lines = [l for l in done.stdout.decode().split("\n") if l]
        assert len(lines) == 3, (case["id"], lines)
        tools_list = lines[1] if tools_list is None else tools_list
        assert tools_list == lines[1]
        replies.append(lines[2])
    return tools_list, replies


def snapshot(root):
    """Every path under root except the profile file and git internals, plus git history of each repo."""
    root = Path(root)
    entries = {}
    repos = []
    for path in sorted(root.rglob("*")):
        rel = path.relative_to(root).as_posix()
        if rel.startswith(GATEWAY_STATE) or rel in GATEWAY_PARENTS or path.name.startswith(LOCK_PREFIX):
            continue
        if rel == "profile.toml" or "/.git/" in f"/{rel}/" or rel.endswith("/.git"):
            if rel.endswith("/.git") and path.is_dir():
                repos.append(rel[: -len("/.git")])
            continue
        if path.is_symlink():
            entries[rel] = {"type": "symlink", "target": os.readlink(path)}
        elif path.is_dir():
            entries[rel] = {"type": "dir"}
        else:
            data = path.read_bytes().replace(str(root).encode(), b"<ROOT>")
            if path.name in MASKED_NAMES:
                data = TIMESTAMP.sub(b"<TS>", data)
            entry = {"type": "file", "mode": path.stat().st_mode & 0o777, "sha256": hashlib.sha256(data).hexdigest(),
                     "size": len(data)}
            if path.name == ".symskills.json":
                # The shared Rust install layer also records `output_digest`/`output_manifest` in the marker
                # (documented residual); compare every other field structurally instead of by hash.
                marker = json.loads(data)
                for extra in MARKER_RUST_ONLY:
                    marker.pop(extra, None)
                entry = {"type": "file", "mode": entry["mode"], "marker": marker}
            entries[rel] = entry
    env = {"PATH": PATH_WITH_GIT, "HOME": str(root / "home"), **GIT_ENV}
    histories = {}
    for repo in repos:
        base = root / repo
        log = subprocess.run(["git", "log", "--all", "--format=%H|%P|%an|%ae|%aI|%cI|%s", "--name-status"], cwd=base, env=env,
                             capture_output=True).stdout.decode(errors="replace")
        status = subprocess.run(["git", "status", "--porcelain"], cwd=base, env=env, capture_output=True).stdout.decode(errors="replace")
        histories[repo] = {"log": log, "status": status}
    return {"entries": entries, "git": histories}


# Install markers/manifests embed the wall-clock install time; mask it before hashing so the snapshot is
# comparable (the Rust replay applies the identical mask).
TIMESTAMP = re.compile(rb"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z")
MASKED_NAMES = (".symskills.json", "manifest.json")
MARKER_RUST_ONLY = ("output_digest", "output_manifest")

SID = re.compile(r'"source_id":"sha256:([0-9a-f]{16})"')


def normalize(text, root, root_real):
    text = text.replace(str(root_real), "<ROOT>").replace(str(root), "<ROOT>")

    def sid(match):
        return match.group(0)  # replaced later, per line, once locations are known
    return SID.sub(sid, text)


def source_id(path_real, skill_md_bytes):
    h = hashlib.sha256()
    h.update(path_real.encode())
    h.update(b"\0")
    h.update(skill_md_bytes)
    return "sha256:" + h.hexdigest()[:16]


def sid_placeholders(line, root_real):
    """Replace each discovery source_id by <sid:REL>, verifying the Go formula against the recorded hash."""
    try:
        reply = json.loads(line)
        text = reply["result"]["content"][0]["text"]
        payload = json.loads(text)
    except Exception:  # not a discovery payload
        return line
    if not isinstance(payload, dict) or "candidates" not in payload:
        return line
    for cand in payload["candidates"] or []:
        location = Path(cand["location"])
        real = Path(os.path.realpath(location))
        skill = (real / "SKILL.md")
        content = skill.read_bytes() if skill.exists() else b""
        expect = source_id(str(real), content)
        assert cand["source_id"] == expect, (cand, expect)
        rel = os.path.relpath(real, root_real)
        cand["_sid_rel"] = rel
    # Rewrite the raw text textually so the byte-exact comparison still covers the rest.
    out = line
    for cand in payload["candidates"] or []:
        out = out.replace(cand["source_id"], "<sid:" + cand["_sid_rel"].replace("\\", "\\\\") + ">", 1)
    return out


def tree_files(root):
    """Regular files and symlinks (as ("link", target)) outside git internals and gateway state."""
    found = {}
    for path in sorted(Path(root).rglob("*")):
        rel = path.relative_to(root).as_posix()
        if rel == "profile.toml" or rel.startswith("tmp") or rel.startswith(GATEWAY_STATE) or "/.git/" in f"/{rel}/":
            continue
        if path.is_symlink():
            found[rel] = ("link", os.readlink(path))
        elif path.is_file():
            found[rel] = path.read_bytes()
    return found


def capture_installs(binary, case):
    """Run the case's `install` steps with the real Go binary and fold the files they create into the
    case's pre-state, so replay never needs the (not yet native) install tool."""
    if not case["install"]:
        return case
    with tempfile.TemporaryDirectory(prefix="toolsoracle-install-") as tmp:
        root = Path(tmp).resolve()
        materialize(root, {**case, "pre_go": []})
        before = tree_files(root)
        for name, target in case["install"]:
            install = {**case, "requests": [call(9, "skills_install", {"name": name, "target": target, "dry_run": False})], "env": {}}
            _, replies = run_server(binary, root, install)
            body = json.loads(json.loads(replies[0])["result"]["content"][0]["text"])
            assert body["action"] in ("installed", "updated"), body
        captured, links = {}, []
        for rel, data in tree_files(root).items():
            if rel in before:
                continue
            if isinstance(data, tuple):
                links.append([rel, data[1].replace(str(root), "<ROOT>")])
            else:
                text = data.decode().replace(str(root), "<ROOT>")
                # Install time is wall-clock; pin it so the recorded pre-state (and fixture) is reproducible.
                captured[rel] = TIMESTAMP.sub(b"2026-01-02T03:04:05Z", text.encode()).decode()
    return {**case, "files": {**case["files"], **captured}, "symlinks": [*case["symlinks"], *links], "install": [list(i) for i in case["install"]]}


def produce_case(binary, case):
    with tempfile.TemporaryDirectory(prefix="toolsoracle-") as tmp:
        root = Path(tmp).resolve()
        materialize(root, case)
        tools_list, raw = run_server(binary, root, case)
        # chmod'd directories must be restorable for cleanup
        for step in case["pre_go"]:
            if step[0] == "chmod":
                os.chmod(root / step[1], 0o755)
        after = snapshot(root)
        replies = [normalize(sid_placeholders(line, root), root, root) for line in raw]
        state = json.loads(normalize(json.dumps(after, ensure_ascii=False), root, root))
        return tools_list, {"replies": replies, "after": state}


def git_tree(ref, source):
    return subprocess.run(["git", "-C", str(ROOT), "rev-parse", f"{ref}:{source}"], capture_output=True, text=True, check=True).stdout.strip()


def build(ref):
    out = Path(tempfile.mkdtemp(prefix="toolsoracle-go-")) / "symbrain"
    subprocess.run(["go", "build", "-o", str(out), "./cmd/symbrain"], cwd=ROOT, check=True)
    return out


def produce(binary, ref):
    for source in GO_SOURCES:
        if subprocess.run(["git", "-C", str(ROOT), "diff", "--quiet", ref, "--", source]).returncode != 0:
            sys.exit(f"Go sources differ from pinned ref {ref}: {source}")
    go_version = subprocess.run(["go", "version"], capture_output=True, text=True, check=True).stdout.strip()
    assert GOTOOLCHAIN.replace("go", "go version go", 1) in go_version, go_version
    rows, listing = [], None
    for case in cases():
        case = capture_installs(binary, case)
        tools_list, observed = produce_case(binary, case)
        listing = tools_list if listing is None else listing
        assert listing == tools_list, "tools/list must not depend on the case"
        rows.append({k: case[k] for k in ("id", "requests", "files", "repos", "env", "symlinks", "dirs", "pre_go", "install")} | {"go": observed})
    ids = [r["id"] for r in rows]
    assert len(ids) == len(set(ids)), "duplicate case ids"
    return {"source": "real Go `symbrain mcp` replies and filesystem effects, never hand-written", "pinned_ref": ref,
            "go_trees": {s: git_tree(ref, s) for s in GO_SOURCES}, "go_version": go_version, "bootstrap": BOOT, "tools_list": listing,
            "git_env": GIT_ENV, "path_with_git": PATH_WITH_GIT, "tools": list(TOOLS), "cases": rows}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("record", "check"))
    parser.add_argument("--go-binary")
    parser.add_argument("--ref")
    args = parser.parse_args()
    ref = args.ref
    if ref is None and args.mode == "check":
        ref = json.loads(FIXTURE.read_text())["pinned_ref"]
    if not ref:
        sys.exit("--ref <pinned commit> is required to record")
    ref = subprocess.run(["git", "-C", str(ROOT), "rev-parse", ref], capture_output=True, text=True, check=True).stdout.strip()
    binary = Path(args.go_binary) if args.go_binary else build(ref)
    data = produce(binary, ref)
    text = json.dumps(data, indent=1, sort_keys=True, ensure_ascii=False) + "\n"
    if args.mode == "record":
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(text)
        print(f"recorded {len(data['cases'])} cases from {ref} ({data['go_version']})")
    elif FIXTURE.read_text() != text:
        old = {c["id"]: c for c in json.loads(FIXTURE.read_text())["cases"]}
        drift = [c["id"] for c in data["cases"] if old.get(c["id"]) != json.loads(json.dumps(c))]
        sys.exit(f"fixture differs from the real Go behaviour (cases: {drift[:10] or 'top-level fields'})")
    else:
        print(f"fixture matches real Go: {len(data['cases'])} cases")


if __name__ == "__main__":
    main()
