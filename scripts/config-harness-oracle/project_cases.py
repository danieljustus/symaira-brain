"""`--project` path-shape corpus for install / uninstall / sync (#769 slice 3).

Each shape is a project argument plus a tiny filesystem layout built inside the
disposable fixture project (the process cwd, `<root>/project`). Every shape is
replayed against the pinned Go CLI for `install`, `uninstall` and `sync`, with
and without `--dry-run`, with the config file absent and pre-seeded. Output,
exit code and the full fixture filesystem (modes, symlinks) must match Go.

Argument tokens: `PROJECT/...` expands to the absolute fixture project path
(see `materialize` in replay.py); everything else is passed verbatim, so
relative shapes resolve against the project cwd. Symlink targets are relative so
fixtures stay root independent.
"""
from __future__ import annotations

import os
from pathlib import Path

SEED_MCP = (b'{"mcpServers":{"other":{"command":"other"},'
            b'"symbrain":{"command":"symbrain","args":["mcp","--profile","p"]}}}')
SEED_INSTRUCTIONS = b"# Project notes\n\nkeep this line\n"

# Layout ops, applied relative to the fixture project directory.
D = "dir"
F = "file"
L = "link"
M = "mode"

# (label, project argument, layout ops, directory (relative to the fixture
# project) where `.mcp.json` / instruction files are seeded, or None)
SHAPES: list[tuple[str, str | bytes, list[tuple], str | None]] = [
    ("dot", ".", [], "."),
    ("dot-slash", "./", [], "."),
    ("dot-dot-slash-dot", "././", [], "."),
    ("dot-via-missing-child", "./x/../", [], "."),
    ("dot-via-existing-child", "./x/../", [(D, "x")], "."),
    ("dot-via-symlink-child", "link/..", [(D, "real/inner"), (L, "link", "real/inner")], "."),
    ("dot-via-file-child", "f/..", [(F, "f", b"x")], "."),
    ("parent-name", "../project", [], "."),
    ("parent-name-slash", "../project/", [], "."),
    ("parent", "..", [], ".."),
    ("parent-slash", "../", [], ".."),
    ("parent-via-child", "a/b/../../..", [(D, "a/b")], ".."),
    ("parent-via-missing-child", "a/b/../../..", [], ".."),
    ("parent-then-sibling", "../sibling", [(D, "../sibling")], "../sibling"),
    ("parent-then-missing-sibling", "../sibling", [], None),
    ("parent-then-dotdot-sibling", "../a/../sibling", [(D, "../sibling")], "../sibling"),
    ("child", "child", [(D, "child")], "child"),
    ("child-slash", "child/", [(D, "child")], "child"),
    ("child-double-slash", "child//", [(D, "child")], "child"),
    ("child-dot", "child/.", [(D, "child")], "child"),
    ("dot-child-dot", "./child/.", [(D, "child")], "child"),
    ("child-dotdot-sibling", "a/../b", [(D, "b")], "b"),
    ("child-dotdot-missing-sibling", "a/../b", [(D, "a")], None),
    ("embedded-dotdot", "a/b/../c", [(D, "a/b"), (D, "a/c")], "a/c"),
    ("embedded-dotdot-missing", "a/b/../c", [], None),
    ("embedded-dotdot-through-file", "f/x/../c", [(F, "f", b"x")], None),
    ("double-slash-inside", "a//b", [(D, "a/b")], "a/b"),
    ("dot-inside", "a/./b", [(D, "a/b")], "a/b"),
    ("dot-trailing-segment", "./a/.", [(D, "a")], "a"),
    ("missing", "missing", [], None),
    ("missing-slash", "missing/", [], None),
    ("missing-deep", "m/n/o", [], None),
    ("missing-deep-dotdot", "m/n/../o", [], None),
    ("parent-of-missing", "m/..", [], "."),
    ("symlink-dir", "link", [(D, "real"), (L, "link", "real")], "real"),
    ("symlink-dir-slash", "link/", [(D, "real"), (L, "link", "real")], "real"),
    ("symlink-dir-dot", "link/.", [(D, "real"), (L, "link", "real")], "real"),
    ("symlink-chain", "l2", [(D, "real"), (L, "l1", "real"), (L, "l2", "l1")], "real"),
    ("symlink-child", "real/sub", [(D, "real/sub"), (L, "link", "real")], "real/sub"),
    ("symlink-parent-component", "link/sub", [(D, "real/sub"), (L, "link", "real")], "real/sub"),
    ("symlink-parent-then-dotdot", "link/sub/..", [(D, "real/sub"), (L, "link", "real")], "real"),
    ("symlink-up", "up", [(L, "up", "..")], ".."),
    ("symlink-self", "self", [(L, "self", "self")], None),
    ("symlink-dangling", "dangling", [(L, "dangling", "nowhere")], None),
    ("symlink-to-file", "linkf", [(F, "f", b"x"), (L, "linkf", "f")], None),
    ("symlink-dir-missing-child", "link/new", [(D, "real"), (L, "link", "real")], "real/new"),
    ("file", "f", [(F, "f", b"x")], None),
    ("file-slash", "f/", [(F, "f", b"x")], None),
    ("file-child", "f/sub", [(F, "f", b"x")], None),
    ("empty-dir-readonly", "ro", [(D, "ro"), (M, "ro", 0o500)], "ro"),
    ("no-access-dir", "noaccess", [(D, "noaccess"), (M, "noaccess", 0o000)], None),
    ("long-component", "x" * 300, [], None),
    ("space", "with space", [(D, "with space")], "with space"),
    ("quote", 'q"uote', [(D, 'q"uote')], 'q"uote'),
    ("backslash", "back\\slash", [(D, "back\\slash")], "back\\slash"),
    ("newline", "new\nline", [(D, "new\nline")], "new\nline"),
    ("html-chars", "<h&t>", [(D, "<h&t>")], "<h&t>"),
    ("unicode", "caf\u00e9/\u65e5\u672c", [(D, "caf\u00e9/\u65e5\u672c")], "caf\u00e9/\u65e5\u672c"),
    ("line-separator", "ls\u2028ps\u2029", [(D, "ls\u2028ps\u2029")], "ls\u2028ps\u2029"),
    ("tilde", "~", [(D, "~")], "~"),
    ("dash-prefixed", "./-dash", [(D, "-dash")], "-dash"),
    ("abs", "PROJECT/", [], "."),
    ("abs-plain", "PROJECT", [], "."),
    ("abs-child", "PROJECT/child", [(D, "child")], "child"),
    ("abs-missing", "PROJECT/missing", [], None),
    ("abs-dotdot", "PROJECT/../project", [], "."),
    ("abs-dotdot-child", "PROJECT/x/../child", [(D, "child")], "child"),
    ("abs-dotdot-missing", "PROJECT/x/../child", [], None),
    ("abs-leading-dotdot-root", "PROJECT/..", [], ".."),
    ("abs-double-slash", "PROJECT//child", [(D, "child")], "child"),
    ("abs-symlink", "PROJECT/link", [(D, "real"), (L, "link", "real")], "real"),
    ("abs-file", "PROJECT/f", [(F, "f", b"x")], None),
    ("non-utf8-missing", b"\xff", [], None),
    ("non-utf8-suffix", b"caf\xc3\xa9\xff", [], None),
    ("non-utf8-nested", b"d\xffir/sub", [], None),
    ("non-utf8-dotdot", b"d\xffir/../ok", [(D, "ok")], "ok"),
    ("non-utf8-abs", b"PROJECT/\xfe", [], None),
    ("empty", "", [], None),
]

def apply_layout(project: Path, ops: list[tuple], seed_dir: str | None = None, seed: bool = False) -> None:
    for op in ops:
        kind, rel = op[0], op[1]
        path = project / rel
        if kind == D:
            path.mkdir(parents=True, exist_ok=True)
        elif kind == F:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(op[2])
        elif kind == L:
            path.parent.mkdir(parents=True, exist_ok=True)
            os.symlink(op[2], path)
    if seed and seed_dir is not None:
        target = project / seed_dir
        if target.is_dir():
            (target / ".mcp.json").write_bytes(SEED_MCP)
            (target / "CLAUDE.md").write_bytes(SEED_INSTRUCTIONS)
            (target / "AGENTS.md").write_bytes(SEED_INSTRUCTIONS)
    # Restrictive modes go last so the fixture itself can still be written.
    for op in ops:
        if op[0] == M:
            os.chmod(project / op[1], op[2])


def make_setup(ops, seed_dir, seed):
    """Layout plus (optionally) a pre-existing config/instruction file."""
    def setup(_root: Path, env: dict[str, str]) -> None:
        apply_layout(Path(env["PROJECT"]), ops, seed_dir, seed)
    return setup


# Commands skipped for a shape because their Go output is not reproducible or
# is a file-permission (not project-path) wording difference: a write into a
# read-only directory names a random Go temp file (`.mcp.json.tmp-<random>`),
# and a backup failure carries Go's `harness: reserve backup ...` wrapper.
NOT_REPLAYED = {
    "empty-dir-readonly": ("install", "install-seeded", "uninstall"),
}

PROJECT_SLOT = "@project@"


def command_variants():
    """(suffix, argv template with one PROJECT_SLOT, seeded).

    `sync` takes `--project` before the positional harness names because the
    Go flag package stops at the first positional.
    """
    p = PROJECT_SLOT
    return [
        ("install", ("install", "--harness", "claude", "--profile", "p", p), False),
        ("install-dry", ("install", "--harness", "claude", "--profile", "p", "--dry-run", p), False),
        ("install-seeded", ("install", "--harness", "claude", "--profile", "p", p), True),
        ("install-seeded-dry", ("install", "--harness", "claude", "--profile", "p", "--dry-run", p), True),
        ("uninstall-fresh", ("uninstall", "--harness", "claude", p), False),
        ("uninstall", ("uninstall", "--harness", "claude", p), True),
        ("uninstall-dry", ("uninstall", "--harness", "claude", "--dry-run", p), True),
        ("sync", ("sync", p, "claude", "opencode"), False),
        ("sync-seeded", ("sync", p, "claude", "opencode"), True),
        ("sync-dry", ("sync", "--dry-run", p, "claude", "opencode"), True),
        ("sync-dry-json", ("sync", "--dry-run", "--json", p, "claude", "opencode"), True),
        ("sync-dry-json-fresh", ("sync", "--dry-run", "--json", p, "claude", "opencode"), False),
    ]


def expand(template, project):
    """Substitute the project slot with `--project VALUE` (separate argv)."""
    out = []
    for token in template:
        if token == PROJECT_SLOT:
            out.extend(("--project", project))
        else:
            out.append(token)
    return tuple(out)


# `--project=VALUE` (one argv token, value glued on) for shapes where the
# inline form can differ from the separate form: empty, `..`, trailing slash
# and raw non-UTF-8 bytes. (label, value, layout ops, seed dir)
INLINE_SHAPES: list[tuple[str, str | bytes, list[tuple], str | None]] = [
    ("empty", "", [], None),
    ("parent-name", "../project", [], "."),
    ("embedded-dotdot", "a/b/../c", [(D, "a/b"), (D, "a/c")], "a/c"),
    ("child-slash", "child/", [(D, "child")], "child"),
    ("non-utf8", b"d\xffir/sub", [], None),
    ("non-utf8-dotdot", b"d\xffir/../ok", [(D, "ok")], "ok"),
    ("dash-value", "-dash", [(D, "-dash")], "-dash"),
]
INLINE_COMMANDS = ("install", "install-dry", "uninstall-fresh", "uninstall", "sync-dry-json-fresh", "sync")


def expand_inline(template, value):
    """Substitute the project slot with one `--project=VALUE` token."""
    out = []
    for token in template:
        if token != PROJECT_SLOT:
            out.append(token)
        elif isinstance(value, bytes):
            out.append(b"--project=" + value)
        else:
            out.append("--project=" + value)
    return tuple(out)
