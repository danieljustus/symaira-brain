"""Store/directory creation parity: Go and native processes, effects and modes.

Every scenario runs both CLIs in an identical, fresh disposable root with the
Go fallback path absent, then compares stdout/stderr/exit (disposable root and
random IDs normalised) and the complete filesystem tree: kind and permission
bits of every created entry. Error-text scenarios depend on POSIX symlink and
permission semantics and run on Unix only; Windows keeps the creation
scenarios and compares tree kinds without POSIX mode bits.
"""
import os
from pathlib import Path
import re
import stat
import subprocess
import tempfile

UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
POSIX = os.name == "posix"
SET = ["set", "hello", "--kind", "fact", "--staged"]


def _setup(root, kind):
    if kind == "file":
        (root / "file").write_text("x")
    elif kind == "readonly":
        (root / "ro").mkdir()
        (root / "ro").chmod(0o500)
    elif kind == "dir":
        (root / "dbdir").mkdir()
    elif kind == "symlink":
        (root / "real").mkdir()
        (root / "link").symlink_to(root / "real")
    elif kind == "dangling":
        (root / "link").symlink_to(root / "nowhere")
    elif kind == "dir0755":
        (root / "pub").mkdir()
        (root / "pub").chmod(0o755)
    elif kind == "dir0777":
        (root / "wide").mkdir()
        (root / "wide").chmod(0o777)


def scenarios():
    """(name, setup, args builder, unix_only)."""
    def with_db(args, *parts, tail=()):
        return lambda root: [*args, "--db", str(root.joinpath(*parts)), *tail]

    items = [
        ("set-new-nested", None, with_db(SET, "a", "b", "c.db", tail=("--json",)), False),
        ("set-new-flat", None, with_db(SET, "c.db", tail=("--json",)), False),
        ("set-new-default", None, lambda root: [*SET, "--json"], False),
        ("set-new-table-output", None, with_db(SET, "a", "c.db"), False),
        ("set-entities-new-store", None, with_db([*SET, "--entities", "alpha, beta"], "e", "c.db"), False),
        ("set-new-relative", None, lambda root: [*SET, "--db", "rel/sub/c.db", "--json"], False),
        ("delete-new-nested", None, with_db(["delete", "nope"], "a", "b", "c.db"), False),
        ("delete-new-json", None, with_db(["delete", "nope"], "a", "b", "c.db", tail=("--json",)), False),
        ("delete-new-default", None, lambda root: ["delete", "nope"], False),
        ("list-new-nested", None, with_db(["list"], "a", "b", "c.db"), False),
        ("rules-new-nested", None, with_db(["rules"], "a", "b", "c.db"), False),
        ("query-log-new-nested", None, with_db(["query-log"], "a", "b", "c.db"), False),
        ("set-dot-segments", None, lambda root: [*SET, "--db", str(root / "a/../b/c.db")], True),
        ("set-trailing-slash", None, lambda root: [*SET, "--db", str(root / "d") + "/"], True),
        ("list-parent-is-file", "file", with_db(["list"], "file", "x.db"), True),
        ("set-parent-is-file", "file", with_db(SET, "file", "x.db"), True),
        ("delete-parent-is-file", "file", with_db(["delete", "a"], "file", "x.db"), True),
        ("set-grandparent-is-file", "file", with_db(SET, "file", "y", "x.db"), True),
        ("list-readonly-parent-new-subdir", "readonly", with_db(["list"], "ro", "sub", "x.db"), True),
        ("set-readonly-parent-new-subdir", "readonly", with_db(SET, "ro", "sub", "x.db"), True),
        ("delete-readonly-parent-new-subdir", "readonly", with_db(["delete", "a"], "ro", "sub", "x.db"), True),
        ("list-readonly-parent-new-file", "readonly", with_db(["list"], "ro", "x.db"), True),
        ("set-readonly-parent-new-file", "readonly", with_db(SET, "ro", "x.db"), True),
        ("delete-readonly-parent-new-file", "readonly", with_db(["delete", "a"], "ro", "x.db"), True),
        ("list-database-is-directory", "dir", with_db(["list"], "dbdir"), True),
        ("set-database-is-directory", "dir", with_db(SET, "dbdir"), True),
        ("delete-database-is-directory", "dir", with_db(["delete", "a"], "dbdir"), True),
        ("list-symlinked-parent", "symlink", with_db(["list"], "link", "x.db"), True),
        ("set-symlinked-parent", "symlink", with_db(SET, "link", "x.db"), True),
        ("set-dangling-symlinked-parent", "dangling", with_db(SET, "link", "x.db"), True),
        ("list-dangling-symlinked-parent", "dangling", with_db(["list"], "link", "x.db"), True),
        ("set-existing-parent-0755-kept", "dir0755", with_db(SET, "pub", "x.db"), True),
        ("set-existing-parent-0777-new-child-0700", "dir0777", with_db(SET, "wide", "sub", "x.db"), True),
        ("list-existing-parent-0777-new-child-0700", "dir0777", with_db(["list"], "wide", "sub", "x.db"), True),
    ]
    return [item for item in items if POSIX or not item[3]]


def _tree(root):
    entries = {}
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        if relative.split("/")[0] in ("config", "cache", "state", "absent-go"):
            continue
        status = path.lstat()
        kind = "l" if stat.S_ISLNK(status.st_mode) else "d" if stat.S_ISDIR(status.st_mode) else "f"
        # A symlink's own mode is filesystem noise (0755 on APFS, 0777 on ext4).
        mode = oct(stat.S_IMODE(status.st_mode)) if POSIX and kind != "l" else None
        entries[relative] = [kind, mode]
    return entries


def _observe(binary, args, root, env):
    result = subprocess.run([str(binary), "memory", *args], cwd=root, env=env,
                            capture_output=True, timeout=30)
    text = lambda data: UUID.sub("<uuid>", data.decode("utf-8", "replace").replace(str(root), "<root>"))
    return dict(exit=result.returncode, stdout=text(result.stdout), stderr=text(result.stderr),
                tree=_tree(root))


def _restore(root):
    for path in root.rglob("*"):
        if path.is_dir() and not path.is_symlink():
            path.chmod(0o755)


def run(go, rust, output, snapshot, isolated_env):
    del output, snapshot
    previous = os.umask(0o022) if POSIX else None
    records = []
    try:
        for index, (name, setup, build, _unix_only) in enumerate(scenarios()):
            if setup == "readonly" and POSIX and os.geteuid() == 0:
                continue  # permission denial is meaningless for root
            observed = []
            for binary in (go, rust):
                with tempfile.TemporaryDirectory(prefix="memory-cli-create-") as temporary:
                    root = Path(temporary).resolve()
                    if setup:
                        _setup(root, setup)
                    observed.append(_observe(binary, build(root), root, isolated_env(root)))
                    if POSIX:
                        _restore(root)
            first, second = observed
            records.append(dict(family="store_creation", index=index, name=name,
                                match=first == second, go=first, rust=second))
    finally:
        if previous is not None:
            os.umask(previous)
    return records
