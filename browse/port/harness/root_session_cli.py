"""Additive root-session process corpus; expectations come from real Go capture.

Called by daemon_registry_cli in the existing source-bound registry producer.
No dispatch/MCP/flow runtime, daemon start, key access or replacement fixture.
The original CLI family and its counts remain separate.
"""
import base64
import copy
import hashlib
import json
import os
from pathlib import Path
import stat

COMMANDS = [(b"daemon", b"status"), (b"session", b"list"),
            (b"session", b"info"), (b"state", b"list")]
FORMATS = [[], [b"--json"], [b"--output", b"yaml"]]
# (id, prefix, between group/verb, suffix, kind, selected session)
# These are argv and branch assertions, not handwritten Go output bytes.
EXTRA = [
    ("other-session", [b"--session=omega"], [], [b"--json"], "missing", "omega"),
    ("last-between", [b"--session", b"first"], [b"--session=alpha"], [b"--json"], "missing", "alpha"),
    ("last-after", [b"--session=first"], [b"--session", b"second"], [b"--session=alpha", b"--json"], "missing", "alpha"),
    ("invalid-then-valid", [b"--session", b"bad session"], [], [b"--session=alpha", b"--json"], "missing", "alpha"),
    ("valid-then-invalid", [b"--session=alpha"], [], [b"--session", b"bad session"], "invalid", None),
    ("json-wins", [b"--json", b"--session=alpha"], [b"--output=yaml"], [], "missing", "alpha"),
    ("json-false", [b"--json", b"--session=alpha"], [], [b"--output=yaml", b"--json=false"], "missing", "alpha"),
    ("output-last", [b"--output=invalid", b"--session=alpha"], [], [b"--output=text"], "missing", "alpha"),
    ("json-invalid-output", [b"--output=invalid", b"--session=alpha"], [], [b"--json"], "missing", "alpha"),
    ("invalid-output", [b"--session=alpha"], [], [b"--output=invalid"], "error", None),
    ("empty-split", [b"--session", b""], [], [], "invalid", None),
    ("empty-equals", [b"--session="], [], [b"--json"], "invalid", None),
    ("invalid-path", [b"--session=../escape"], [], [b"--output=yaml"], "invalid", None),
    ("help-value", [b"--session", b"--help"], [], [], "invalid", None),
    ("version-value", [b"--session", b"--version"], [], [], "invalid", None),
    ("delimiter-value", [b"--session", b"--"], [], [], "invalid", None),
    ("command-value", [b"--session", b"state"], [], [b"--json"], "missing", "state"),
    ("missing-value", [b"--session=alpha"], [], [b"--session"], "error", None),
    ("trailing-delimiter", [b"--session=alpha"], [], [b"--"], "missing", "alpha"),
    ("delimiter-argument", [b"--session=alpha"], [], [b"--", b"--session=omega"], "error", None),
    ("unknown-prefix", [b"--session=alpha", b"--bogus=x"], [], [], "error", None),
    ("unknown-after", [b"--session=alpha"], [], [b"--bogus=x"], "error", None),
    ("json-before-error", [b"--json", b"--session=alpha"], [], [b"--bogus=x"], "error", None),
    ("json-after-error", [b"--session=alpha"], [], [b"--bogus=x", b"--json"], "error", None),
    ("yaml-before-error", [b"--output=yaml", b"--session=alpha"], [], [b"--bogus=x"], "error", None),
    ("json-missing-value", [b"--session=alpha"], [], [b"--json", b"--session"], "error", None),
    ("json-delimiter-argument", [b"--json", b"--session=alpha"], [], [b"--", b"--session=omega"], "error", None),
    ("help-prefix", [b"--session=alpha"], [], [b"--help"], "help", None),
    ("help-between", [], [b"--session", b"alpha"], [b"--help"], "help", None),
    ("help-after", [], [], [b"--session=alpha", b"--help"], "help", None),
    ("help-value-then-help", [b"--session", b"--help"], [], [b"--help"], "help", None),
    ("review160/help-true", [b"--session=alpha"], [], [b"--help=true"], "help", None),
    ("review160/help-short-true", [b"--session=alpha"], [], [b"-h=true"], "help", None),
    ("review160/help-one", [b"--session=alpha"], [], [b"--help=1"], "help", None),
    ("review160/help-false", [b"--session=alpha"], [], [b"--help=false", b"--json"], "missing", "alpha"),
    ("review160/help-last-false", [b"--session=alpha"], [], [b"--help", b"--help=false", b"--json"], "missing", "alpha"),
    ("review160/help-short-last-false", [b"--session=alpha"], [], [b"-h", b"-h=false", b"--json"], "missing", "alpha"),
    ("review160/help-cluster-false", [b"--session=alpha"], [], [b"-hh=false", b"--json"], "missing", "alpha"),
    ("review160/help-last-true", [b"--session=alpha"], [], [b"--help=false", b"--help"], "help", None),
    ("review160/help-lone-hyphen", [b"--session=alpha"], [], [b"-", b"--help"], "help", None),
    ("review160/no-session-hyphen", [], [], [b"-", b"--help"], "help", None),
    ("review160/hyphen-after-help", [b"--session=alpha"], [], [b"--help", b"-"], "help", None),
    ("review160/help-bad-bool", [b"--session=alpha"], [], [b"--help=maybe"], "error", None),
    ("review160/json-before-bad-help", [b"--session=alpha"], [], [b"--json", b"--help=maybe"], "error", None),
    ("review160/json-after-bad-help", [b"--session=alpha"], [], [b"--help=maybe", b"--json"], "error", None),
    ("review160/short-before-help", [b"--session=alpha"], [], [b"-x", b"value", b"--help"], "error", None),
    ("review160/short-after-help", [b"--session=alpha"], [], [b"--help", b"-x", b"value"], "error", None),
    ("review160/json-before-short", [b"--session=alpha"], [], [b"--json", b"-x", b"value", b"--help"], "error", None),
    ("review160/json-after-short", [b"--session=alpha"], [], [b"-x", b"value", b"--json", b"--help"], "error", None),
    ("review160/help-value-false", [b"--session", b"--help=false"], [], [], "invalid", None),
    ("review160/help-after-delimiter", [b"--session=alpha"], [], [b"--", b"--help"], "error", None),
    ("review162/json-invalid-only", [b"--session=alpha"], [], [b"--json=not-bool"], "error", None),
    ("review162/json-invalid-repeat", [b"--session=alpha"], [], [b"--json", b"--json=not-bool"], "error", None),
    ("review162/json-invalid-repeat-equals", [b"--session=alpha"], [], [b"--json=true", b"--json=not-bool"], "error", None),
    ("review162/json-invalid-yaml", [b"--output=yaml", b"--session=alpha"], [], [b"--json", b"--json=not-bool"], "error", None),
    ("review162/json-invalid-text", [b"--output=text", b"--session=alpha"], [], [b"--json=true", b"--json=not-bool"], "error", None),
    ("review162/json-invalid-output-json", [b"--output=json", b"--session=alpha"], [], [b"--json=true", b"--json=not-bool"], "error", None),
    ("review162/json-invalid-output-invalid", [b"--output=invalid", b"--session=alpha"], [], [b"--json", b"--json=not-bool"], "error", None),
    ("review162/json-invalid-later-flags", [b"--session=alpha"], [], [b"--json", b"--json=not-bool", b"--json", b"--output=yaml", b"--help"], "error", None),
    ("review162/json-first-error", [b"--session=alpha"], [], [b"--json", b"--bogus=x", b"--json=not-bool", b"--help"], "error", None),
    ("review162/help-before-json-invalid", [b"--session=alpha"], [], [b"--help", b"--json", b"--json=not-bool"], "error", None),
]
NEGATIVES = [
    [b"--session=alpha", b"version"],
    [b"--session", b"alpha", b"version", b"--help"],
    [b"--session=alpha", b"config", b"show"],
    [b"--session=alpha", b"profiles"],
    [b"--session=alpha", b"batch", b"--dry-run", b"version"],
    [b"--session=alpha", b"completion", b"bash"],
    [b"--session=alpha", b"flow", b"list"],
    [b"flow", b"--session", b"alpha", b"validate", b"not-read.yaml"],
    [b"flow", b"list", b"--session=alpha", b"--help"],
    [b"--session=alpha", b"help", b"session", b"list"],
    [b"--session=alpha", b"not-a-command"],
    [b"--session=alpha"],
    [b"--session"],
    [b"--session=alpha", b"--version"],
    [b"--session=alpha", b"--", b"session", b"list"],
    [b"--json", b"--session=alpha", b"version"],
    [b"--session=alpha", b"version", b"--json"],
    [b"--output=yaml", b"--session=alpha", b"version"],
]
UNIX_VALUES = [b"bad\xffsession", b"bad\xe2\x82session", b"bad\xc0\xafsession"]
DAEMON_BOOLEANS = [b"--ssrf", b"--allow-private", b"--headless", b"--mcp-mode"]
REVIEW_CASES = [
    ("first-bogus", [b"--bogus=x", b"--json", b"--session=alpha", b"version"], "error", None),
    ("first-bogus-json", [b"--json", b"--bogus=x", b"--session=alpha", b"version"], "error", None),
    ("first-bogus-yaml", [b"--output=yaml", b"--bogus=x", b"--session=alpha", b"version"], "error", None),
    ("first-short", [b"-x", b"value", b"--json", b"--session=alpha", b"version"], "error", None),
    ("restore-missing", [b"--session=alpha", b"daemon", b"status", b"--help", b"--restore"], "error", None),
    ("profile-missing-json", [b"--session=alpha", b"daemon", b"status", b"--json", b"--help", b"--profile"], "error", None),
    ("restore-help-value", [b"--session=alpha", b"daemon", b"--restore", b"--help", b"status", b"--json"], "missing", "alpha"),
    ("profile-help-value", [b"--session=alpha", b"daemon", b"--profile", b"--help", b"status", b"--json"], "missing", "alpha"),
    ("restore-equals", [b"--session=alpha", b"daemon", b"status", b"--restore=--help", b"--json"], "missing", "alpha"),
    ("profile-equals", [b"--session=alpha", b"daemon", b"status", b"--profile=--help", b"--json"], "missing", "alpha"),
]
REVIEW162_GROUPS = [b"daemon", b"session", b"state"]
REVIEW162_GROUP_CASES = [
    ("bogus-before-help", [b"--bogus=x", b"--help"], "error"),
    ("bogus-after-help", [b"--help", b"--bogus=x"], "error"),
    ("json-before-bogus", [b"--json", b"--bogus=x", b"--help"], "error"),
    ("json-after-bogus", [b"--bogus=x", b"--json", b"--help"], "error"),
    ("yaml-before-bogus", [b"--output=yaml", b"--bogus=x", b"--help"], "error"),
    ("missing-session", [b"--help", b"--session"], "error"),
    ("missing-output", [b"--json", b"--help", b"--output"], "error"),
    ("unknown-short", [b"--help", b"-x", b"value"], "error"),
    ("json-invalid-repeat", [b"--json", b"--json=not-bool", b"--help"], "error"),
    ("json-invalid-yaml", [b"--output=yaml", b"--json=true", b"--json=not-bool", b"--help"], "error"),
    ("help", [b"--help"], "help"),
    ("help-equals", [b"--help=true"], "help"),
    ("help-json", [b"--json", b"--help"], "help"),
    ("help-yaml", [b"--output=yaml", b"--help"], "help"),
    ("help-consumed-json", [b"--help", b"--session", b"--json"], "help"),
    ("help-delimiter", [b"--help", b"--", b"--bogus=x"], "help"),
]
REVIEW162_DAEMON_CASES = [
    ("restore-missing", [b"--help", b"--restore"], "error"),
    ("restore-missing-json", [b"--json", b"--help", b"--restore"], "error"),
    ("restore-missing-yaml", [b"--output=yaml", b"--help", b"--restore"], "error"),
    ("restore-value-help", [b"--help", b"--restore", b"--json"], "help"),
    ("profile-missing", [b"--help", b"--profile"], "error"),
    ("profile-value-help", [b"--help", b"--profile", b"--json"], "help"),
    ("domains-missing", [b"--help", b"--allowed-domains"], "error"),
    ("domains-value-help", [b"--allowed-domains", b"--json", b"--help"], "help"),
    ("engine-missing", [b"--help", b"--engine"], "error"),
    ("engine-value-help", [b"--engine", b"--json", b"--help"], "help"),
    ("cdp-missing", [b"--help", b"--cdp-endpoint"], "error"),
    ("cdp-value-help", [b"--cdp-endpoint", b"--json", b"--help"], "help"),
]


REVIEW164_GROUP_CASES = [
    ("unknown-before-help", [b"not-a-command", b"--help"], "help"),
    ("unknown-after-help-equals", [b"--help=true", b"not-a-command"], "help"),
    ("unknown-after-help", [b"--help", b"not-a-command"], "help"),
    ("unknown-before-real-child", [b"not-a-command", b"status", b"--help"], "help"),
    ("json-unknown-help", [b"--json", b"not-a-command", b"--help"], "help"),
    ("yaml-unknown-help", [b"--output=yaml", b"--help=true", b"not-a-command"], "help"),
    ("unknown-bogus-help", [b"not-a-command", b"--bogus=x", b"--help"], "error"),
    ("help-unknown-bogus", [b"--help=true", b"not-a-command", b"--bogus=x"], "error"),
    ("json-unknown-bogus", [b"--json", b"not-a-command", b"--bogus=x", b"--help"], "error"),
    ("unknown-bogus-json", [b"not-a-command", b"--bogus=x", b"--json", b"--help"], "error"),
    ("unknown-help-missing", [b"not-a-command", b"--help", b"--session"], "error"),
]
REVIEW164_LEAVES = [(b"daemon", b"status"), (b"daemon", b"stop"),
                    (b"session", b"list"), (b"session", b"info"),
                    (b"state", b"list"), (b"state", b"show")]


def cases(raw_unix):
    result = []
    for group, verb in COMMANDS:
        label = (group + b"-" + verb).decode()
        for position in range(3):
            for inline in (False, True):
                selection = [b"--session=alpha"] if inline else [b"--session", b"alpha"]
                parts = [[], [], []]
                parts[position] = selection
                for fmt, output in enumerate(FORMATS):
                    result.append((f"{label}/place-{position}-{int(inline)}-{fmt}",
                        [*parts[0], group, *parts[1], verb, *parts[2], *output], "missing", "alpha"))
        for name, prefix, middle, suffix, kind, selected in EXTRA:
            result.append((f"{label}/{name}", [*prefix, group, *middle, verb, *suffix], kind, selected))
        if raw_unix:
            for number, value in enumerate(UNIX_VALUES):
                for inline in (False, True):
                    selection = [b"--session=" + value] if inline else [b"--session", value]
                    result.append((f"{label}/unix-{number}-{int(inline)}",
                                   [*selection, group, verb, b"--json"], "invalid", None))
    result.extend((f"unsupported/{i}", argv, "error", None) for i, argv in enumerate(NEGATIVES))
    result.extend((f"review160/{name}", argv, kind, selected) for name, argv, kind, selected in REVIEW_CASES)
    for flag in DAEMON_BOOLEANS:
        for inline in (False, True):
            spelling = flag + b"=true" if inline else flag
            for between in (False, True):
                command = [b"daemon", spelling, b"status"] if between else [b"daemon", b"status", spelling]
                for help_flag in (False, True):
                    suffix = [b"--help"] if help_flag else []
                    name = f"review160/daemon-{flag.decode()}-{int(inline)}-{int(between)}-{int(help_flag)}"
                    result.append((name, [b"--session=alpha", *command, *suffix], "error", None))
    for group in REVIEW162_GROUPS:
        for prefixed in (False, True):
            prefix = [b"--session=alpha"] if prefixed else []
            for name, suffix, kind in REVIEW162_GROUP_CASES:
                result.append((f"review162/{group.decode()}-{int(prefixed)}-{name}",
                               [*prefix, group, *suffix], kind, None))
    for name, suffix, kind in REVIEW162_DAEMON_CASES:
        result.append((f"review162/daemon-local-{name}",
                       [b"--session=alpha", b"daemon", *suffix], kind, None))
    for flag in DAEMON_BOOLEANS:
        for inline in (False, True):
            spelling = flag + b"=true" if inline else flag
            result.append((f"review162/daemon-local-{flag.decode()}-{int(inline)}",
                           [b"--session=alpha", b"daemon", spelling, b"--help"], "help", None))
    for group in REVIEW162_GROUPS:
        for selection, prefix in enumerate(([], [b"--session=alpha"], [b"--session", b"alpha"])):
            for name, suffix, kind in REVIEW164_GROUP_CASES:
                result.append((f"review164/{group.decode()}-{selection}-{name}",
                               [*prefix, group, *suffix], kind, None))
    for group, verb in REVIEW164_LEAVES:
        for prefixed in (False, True):
            prefix = [b"--session=alpha"] if prefixed else []
            result.append((f"review164/leaf-{group.decode()}-{verb.decode()}-{int(prefixed)}",
                           [*prefix, group, verb, b"not-a-command", b"--help"], "help", None))
    for flag in DAEMON_BOOLEANS:
        for inline in (False, True):
            spelling = flag + b"=true" if inline else flag
            result.append((f"review164/daemon-local-{flag.decode()}-{int(inline)}",
                           [b"--session=alpha", b"daemon", spelling, b"not-a-command", b"--help"], "help", None))
    result.append(("review164/daemon-unknown-missing-restore",
                   [b"--session=alpha", b"daemon", b"not-a-command", b"--help", b"--restore"], "error", None))
    # Only strict Go inspection/help rejection controls belong here. Native
    # DaemonProxy --mode startup success is covered by parser-only Rust source.
    for inline in (False, True):
        mode = [b"--mode=browser"] if inline else [b"--mode", b"browser"]
        for verb in (b"status", b"stop"):
            for between in (False, True):
                middle, after = (mode, []) if between else ([], mode)
                for help_flag in (False, True):
                    suffix = [b"--help"] if help_flag else []
                    result.append((f"review164/mode-{verb.decode()}-{int(inline)}-{int(between)}-{int(help_flag)}",
                                   [b"--session=alpha", b"daemon", *middle, verb, *after, *suffix], "error", None))
        result.append((f"review164/mode-group-help-{int(inline)}",
                       [b"--session=alpha", b"daemon", *mode, b"--help"], "error", None))
    assert len({item[0] for item in result}) == len(result)
    return result


def snapshot(root):
    """No following symlinks or reading product state outside the owned root."""
    entries = {}
    for parent, directories, files in os.walk(root, followlinks=False):
        for name in directories + files:
            path = Path(parent) / name
            info = path.lstat()
            item: dict[str, int | str] = {"mode": stat.S_IMODE(info.st_mode), "type": stat.S_IFMT(info.st_mode)}
            if stat.S_ISREG(info.st_mode):
                item["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
                item["bytes"] = info.st_size
            elif stat.S_ISLNK(info.st_mode):
                item["link_base64"] = base64.b64encode(os.fsencode(os.readlink(path))).decode()
            entries[path.relative_to(root).as_posix()] = item
    return entries


def observe(run, root, env):
    assert env["SYMBROWSE_NO_AUTOSTART"] == "1"
    assert Path(env["HOME"]).is_relative_to(root)
    before = snapshot(root)
    records = []
    inventory = cases(os.name == "posix")
    for name, arguments, kind, selected in inventory:
        observed = run(arguments)  # existing real registry_cli_process.capture
        records.append(dict(observed, id=name, kind=kind, selected_session=selected))
        assert snapshot(root) == before, f"root-session case changed protected files: {name}"
    return {"records": records, "count": len(inventory), "raw_unix_bytes": os.name == "posix",
            "root": str(root), "before": before, "after": snapshot(root),
            "no_files_changed": True, "autostart_disabled": True,
            "source_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}


def comparable(observed, *, go=False):
    assert type(observed["raw_unix_bytes"]) is bool
    inventory = cases(observed["raw_unix_bytes"])
    assert type(observed["count"]) is int and observed["count"] == len(inventory)
    assert observed["no_files_changed"] is True and observed["autostart_disabled"] is True
    assert observed["before"] == observed["after"], "root-session protected state changed"
    assert len(observed["records"]) == len(inventory)
    result = []
    # Same existing registry root-only normalization, applied to complete raw
    # streams. Do not remove error details, selected sessions or other bytes.
    root = observed["root"]
    roots = {os.fsencode(root), root.replace("\\", "/").encode(),
             json.dumps(root, ensure_ascii=False)[1:-1].encode()}
    for actual, (name, argv, kind, selected) in zip(observed["records"], inventory):
        assert actual["id"] == name and actual["kind"] == kind and actual["selected_session"] == selected
        assert actual["arguments_base64"] == [base64.b64encode(arg).decode() for arg in argv]
        assert type(actual["exit"]) is int
        if kind == "help":
            assert actual["exit"] == 0 and actual["stderr_base64"] == ""
        elif kind in ("missing", "invalid"):
            assert actual["exit"] == 1, actual
        else:
            assert actual["exit"] in (1, 2), "unsupported/malformed flags were accepted"
        item = copy.deepcopy(actual)
        if kind == "help" and name.startswith(("review162/session-", "review164/session-")):
            # Reuse the original family's exact, asserted session-id help-row
            # exception. Only the comparison copy gets a canonical help argv;
            # the captured argv and every other stdout/stderr byte stay intact.
            from daemon_registry_cli import comparable_help
            canonical = dict(item, arguments_base64=[base64.b64encode(arg).decode()
                                                     for arg in (b"session", b"--help")])
            item["stdout_base64"] = comparable_help(canonical, go=go)["stdout_base64"]
        for field in ("stdout_base64", "stderr_base64"):
            raw = base64.b64decode(item[field], validate=True)
            for prefix in sorted(roots, key=len, reverse=True):
                raw = raw.replace(prefix, b"<owned-root>")
            item[field] = base64.b64encode(raw).decode()
        if kind == "missing" and b"--json" in argv and b"--json=false" not in argv:
            payload = json.loads(base64.b64decode(actual["stdout_base64"]))
            assert payload["success"] is False
            assert payload["error"]["code"] == "daemon_unavailable"
            assert payload["error"]["details"]["session"] == selected, "wrong session selected"
        result.append(item)
    return result


def compare(left, right):
    assert left["raw_unix_bytes"] == right["raw_unix_bytes"]
    assert left["source_sha256"] == right["source_sha256"] == hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    assert comparable(left, go=True) == comparable(right), "root-session Go/Rust argv/exit/stdout/stderr differ"


def controls(left, right):
    compare(left, right)  # genuine positive pair first, never self-baseline
    rejected = []
    for name in ("wrong-session", "unsupported-accepted", "missing-case", "filesystem-drift"):
        bad = copy.deepcopy(right)
        if name == "wrong-session":
            selected = next(r for r in bad["records"] if r["id"] == "daemon-status/place-0-1-1")
            other = next(r for r in bad["records"] if r["id"] == "daemon-status/other-session")
            for field in ("stdout_base64", "stderr_base64"):
                selected[field] = other[field]  # actual other-session output
        elif name == "unsupported-accepted":
            next(r for r in bad["records"] if r["id"] == "unsupported/0")["exit"] = 0
        elif name == "missing-case":
            bad["records"].pop()
        else:
            bad["after"]["unexpected-daemon-file"] = {"type": stat.S_IFREG, "mode": 0o600}
        try:
            compare(left, bad)
        except AssertionError:
            rejected.append(name)
        else:
            raise AssertionError(f"root-session negative control accepted: {name}")
    return rejected
