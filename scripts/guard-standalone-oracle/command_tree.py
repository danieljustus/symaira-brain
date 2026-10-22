#!/usr/bin/env python3
"""Go-derived `symguard` command-tree inventory and byte-exact Rust surface proof (#770).

Usage: command_tree.py GO_BINARY RUST_BINARY REPORT_JSON GO_SOURCE_ARCHIVE [--write-fixture]

The inventory is derived from the actual Go oracle process (help-tree walk) and the
frozen Go source (dispatcher cases, flag literals, import graph), never from the Rust
implementation.  Every probed path/flag form is then executed by both binaries in
disposable HOME/XDG roots and compared on stdout, stderr, exit code and file state.
A probe that differs (including a silent success or an explicit "not ported" error)
makes its node `unported`; the gate requires zero.  Verification is non-writing;
the committed fixture is only rewritten with --write-fixture.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

import replay

ROOT, REF = replay.ROOT, replay.REF
FIXTURE = ROOT / "migration/evidence/guard-command-tree-770/inventory.json"
HELP = ("--help", "-h")
UNKNOWN_WORD, UNKNOWN_FLAG, POSITIONAL = "no-such-word", "--no-such-flag", "positional-x"
PKG_PREFIX = "github.com/danieljustus/symaira-brain/guard/internal/"
# Hand-annotated Rust owners of the Go library packages; existence is verified.
RUST_OWNERS = {
    "approval": ["rust/symbrain-guard-core/src/approval.rs"],
    "archguard": [],
    "audit": ["rust/symbrain-guard-core/src/audit.rs", "rust/symbrain-audit"],
    "capability": ["rust/symbrain-guard-core/src/capability.rs"],
    "config": ["rust/symguard-cli/src/doctor/config.rs", "rust/symguard-cli/src/guard_scan_config.rs"],
    "discovery": ["rust/symguard-cli/src/guard_scan.rs"],
    "grant": ["rust/symbrain-guard-core/src/grant.rs", "rust/symguard-cli/src/guard_grants_store.rs"],
    "model": ["rust/symbrain-guard-core/src/model.rs"],
    "output": ["rust/symguard-cli/src/guard_scan.rs"],
    "policy": ["rust/symbrain-guard-core/src/policy.rs"],
    "proposal": [],
    "sequence": ["rust/symbrain-guard-core/src/sequence.rs"],
    "spawn": ["rust/symguard-cli/src/guard_doctor.rs"],
    "update": [],
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def text_of(hex_value):
    data = bytes.fromhex(hex_value)
    data = re.sub(rb"os/arch \S+", b"os/arch <os-arch>", data)
    data = re.sub(rb"OS/Arch:   \S+", b"OS/Arch:   <os-arch>", data)
    return data.decode("utf-8", "backslashreplace")


def observe(binary, args, base, native):
    case = dict(id="tree", args=list(args), state="empty", contract="parity", payload=b"")
    return replay.observe(binary, case, base, native)


class Runner:
    def __init__(self, owned):
        self.owned, self.count = Path(owned), 0

    def run(self, binary, args, native):
        self.count += 1
        return observe(binary, args, self.owned / str(self.count), native)


# ---------------------------------------------------------------- Go-derived discovery
def parse_commands(text):
    names, active = [], False
    for line in text.splitlines():
        if line.strip() == "Commands:":
            active = True
        elif active and not line.strip():
            break
        elif active:
            match = re.match(r"^  ([a-z][\w-]*)\s{2,}\S", line)
            if match:
                names.append(match.group(1))
    return names


def walk_help_tree(runner, go):
    """Walk `--help` output of the Go oracle; returns {path tuple: {"help_text", "children"}}."""
    nodes = {}

    def visit(path, parent_text):
        out = runner.run(go, [*path, "--help"], False)
        text = text_of(out["normalized_stdout_hex"])
        children = [c for c in parse_commands(text) if c != "help"]
        if parent_text is not None and (text == parent_text or not text.startswith("Usage:")):
            children = []
        # Only the root and Usage-style output is help text. A command that runs on
        # `--help` (doctor, version, grants list) ignores the flag, which is recorded.
        is_help = not path or text.startswith("Usage:")
        nodes[path] = dict(help_text=text if is_help else None, help_flag_ignored=not is_help,
                           children=children, help_exit=out["exit_code"],
                           builtin_help=("help" in parse_commands(text)))
        if len(path) < 3:
            for child in children:
                visit((*path, child), text)

    visit((), None)
    return nodes


def source_literals(src):
    main_text = (src / "oracle770/main.go").read_text(encoding="utf-8")
    packages = sorted(set(re.findall(r'symguard/(\w+)"', main_text)))
    code = lambda text: "\n".join(re.sub(r"//.*$", "", line) for line in text.splitlines())
    case_re = re.compile(r'case\s+((?:"[^"]+"\s*,\s*)*"[^"]+")\s*:')

    def cases(text):
        found = []
        for group in case_re.findall(text):
            found.extend(re.findall(r'"([^"]+)"', group))
        return found

    dispatcher_words = set(cases(code(main_text))) if "switch args[0]" in main_text else set()
    flags, sub_words = {}, {}
    for package in packages:
        source = code((src / "guard/cmd/symguard" / package / "command.go").read_text(encoding="utf-8"))
        found = {}
        for name, equals in re.findall(r'"(-{1,2}[A-Za-z][A-Za-z0-9-]*)(=?)"', source):
            found[name] = found.get(name, False) or bool(equals)
        flags[package] = found
        sub_words[package] = set(cases(source)) if "switch args[0]" in source else set()
    return packages, dispatcher_words, flags, sub_words


def package_users(src, every):
    """Command packages importing each guard/internal package (non-test Go source)."""
    users = {name: set() for name in every}
    for path in sorted((src / "guard/cmd/symguard").rglob("*.go")):
        if path.name.endswith("_test.go"):
            continue
        for name in re.findall(r'"' + re.escape(PKG_PREFIX) + r'(\w+)"', path.read_text(encoding="utf-8")):
            users.setdefault(name, set()).add(path.parent.name)
    return {name: sorted(value) for name, value in users.items()}


def update_check_invoked(src):
    """True when any non-test Go source outside its definition calls version.CheckLatest."""
    calls = []
    for path in sorted(src.joinpath("guard").rglob("*.go")):
        if path.name.endswith("_test.go"):
            continue
        for line in path.read_text(encoding="utf-8").splitlines():
            code = line.split("//")[0]
            if "CheckLatest(" in code and "func CheckLatest" not in code:
                calls.append(str(path))
    return bool(calls) or "CheckLatest" in (src / "oracle770/main.go").read_text(encoding="utf-8")


def internal_packages(src):
    env = dict(os.environ, GOTOOLCHAIN="go1.26.7", CGO_ENABLED="0", GOFLAGS="-mod=readonly")
    deps = subprocess.check_output(["go", "list", "-deps", "-f", "{{.ImportPath}}", "./oracle770"],
                                   cwd=src, env=env, text=True).split()
    reachable = {d.removeprefix(PKG_PREFIX) for d in deps if d.startswith(PKG_PREFIX)}
    every = sorted(p.name for p in (src / "guard/internal").iterdir() if p.is_dir())
    return every, reachable, package_users(src, every), update_check_invoked(src)


# ---------------------------------------------------------------- probes
def probe_plan(nodes, packages, dispatcher_words, flags):
    """Probe list [(owner path, args)] — every form is run against Go and Rust."""
    plan = []

    def add(owner, args):
        if (owner, tuple(args)) not in plan:
            plan.append((owner, tuple(args)))

    for args in [[], ["--help"], ["-h"], [UNKNOWN_WORD], [UNKNOWN_FLAG], ["--help", POSITIONAL], ["-"], ["--"]]:
        add((), args)
    for args in [["help"], ["help", POSITIONAL], ["help", "--help"]]:
        add(("help",), args)
    for path in sorted(nodes):
        if not path:
            continue
        package = path[0]
        add(path, path)
        for token in HELP:
            add(path, [*path, token])
        add(path, [*path, "help"])
        add(path, [*path, UNKNOWN_WORD])
        add(path, [*path, UNKNOWN_FLAG])
        add(path, [*path, "--"])
        for flag, takes_value in sorted(flags[package].items()):
            if flag in HELP:
                continue
            forms = [[flag], [flag, POSITIONAL], [POSITIONAL, flag]]
            if takes_value:
                forms += [[flag + "="], [flag + "=json"], [flag + "=table"], [flag, "json"], [flag + "=" + UNKNOWN_WORD]]
            for form in forms:
                add(path, [*path, *form])
    return plan


def build_inventory(runner, go, src):
    packages, dispatcher_words, flags, sub_words = source_literals(src)
    nodes = walk_help_tree(runner, go)
    # Completeness: every dispatcher / sub-command literal in the Go source is a discovered path.
    advertised = {p[0] for p in nodes if len(p) == 1} | {"help"}
    assert set(packages) <= dispatcher_words, f"command package without dispatcher case: {set(packages) - dispatcher_words}"
    extra = dispatcher_words - advertised - set(HELP)
    assert not extra, f"dispatcher cases not reachable through the help tree: {sorted(extra)}"
    for package, words in sub_words.items():
        discovered = {p[1] for p in nodes if len(p) == 2 and p[0] == package}
        hidden = words - discovered - {"help", *HELP}
        assert not hidden, f"{package}: source sub-command cases not in help tree: {sorted(hidden)}"
    plan = probe_plan(nodes, packages, dispatcher_words, flags)
    return packages, dispatcher_words, flags, sub_words, nodes, plan


def run_probes(runner, go, rust, plan):
    probes = []
    for owner, args in plan:
        left = runner.run(go, list(args), False)
        right = runner.run(rust, list(args), True)
        try:
            assert replay.comparable(left) == replay.comparable(right), "stdout/stderr/exit/state differs"
            status = "native"
        except AssertionError as error:
            status = "unported: " + str(error)
        probes.append(dict(owner=list(owner), args=list(args), go=left, rust=right, status=status))
    return probes


def deterministic(probes, nodes, flags, packages, dispatcher_words, sub_words, every, reachable, users, update_invoked):
    entries = []
    for probe in probes:
        stdout, stderr = text_of(probe["go"]["normalized_stdout_hex"]), text_of(probe["go"]["normalized_stderr_hex"])
        entries.append(dict(args=probe["args"], owner=probe["owner"], exit_code=probe["go"]["exit_code"],
                            stdout=stdout, stderr=stderr, root_dependent="<root>" in stdout + stderr))
    tree = []
    for path in sorted(nodes):
        info = nodes[path]
        package = path[0] if path else None
        node_flags = [] if package is None else [
            dict(name=n, takes_value=v, advertised=n in (nodes[(package,)]["help_text"] or ""))
            for n, v in sorted(flags[package].items())]
        tree.append(dict(path=list(path), kind="root" if not path else "command" if len(path) == 1 else "subcommand",
                         children=info["children"], builtin_help=info["builtin_help"], help_exit=info["help_exit"],
                         help_flag_ignored=info["help_flag_ignored"], help_text=info["help_text"],
                         package_flags=node_flags))
    tree.append(dict(path=["help"], kind="builtin", children=[], builtin_help=False, help_exit=0,
                     help_flag_ignored=False, help_text=entries_text(entries, ["help"]), package_flags=[]))
    library = [dict(package=name, import_reachable_from_dispatcher=name in reachable,
                    imported_by_command_packages=users[name], command_path=False,
                    rust_owners=RUST_OWNERS[name]) for name in every]
    return dict(oracle_ref=REF, go_toolchain="go1.26.7", dispatcher_words=sorted(dispatcher_words),
                command_packages=packages, update_check_invoked_by_any_command=update_invoked, tree=tree, library_packages=library, probes=entries)


def entries_text(entries, args):
    for entry in entries:
        if entry["args"] == args:
            return entry["stdout"]
    raise AssertionError("missing probe " + str(args))


def fixture_diff(actual, fixture):
    """Raise AssertionError unless the Go-derived inventory equals the committed fixture."""
    for key in ["oracle_ref", "go_toolchain", "dispatcher_words", "command_packages", "update_check_invoked_by_any_command",
                "tree", "library_packages"]:
        assert actual[key] == fixture[key], f"fixture drift: {key}"
    left, right = {tuple(e["args"]): e for e in actual["probes"]}, {tuple(e["args"]): e for e in fixture["probes"]}
    assert left.keys() == right.keys(), "fixture drift: probe set"
    for key, entry in left.items():
        other = right[key]
        assert entry["exit_code"] == other["exit_code"], f"fixture drift: exit code of {list(key)}"
        if not entry["root_dependent"] and not other["root_dependent"]:
            assert entry["stdout"] == other["stdout"] and entry["stderr"] == other["stderr"], \
                f"fixture drift: output of {list(key)}"


def summarize(probes, tree):
    by_owner = {}
    for probe in probes:
        by_owner.setdefault(tuple(probe["owner"]), []).append(probe["status"])
    states = {owner: ("native" if all(s == "native" for s in statuses) else "unported")
              for owner, statuses in by_owner.items()}
    # The root dispatcher is itself a path (usage, unknown command, no arguments).
    paths = [list(n["path"]) for n in tree]
    unported = [p for p in paths if states.get(tuple(p)) != "native"]
    return dict(paths_total=len(paths), paths_native=len(paths) - len(unported), paths_unported=unported,
                paths_fallback=0, probes=len(probes), probes_native=sum(p["status"] == "native" for p in probes))


def assert_no_fallback():
    manifest = (ROOT / "rust/symguard-cli/Cargo.toml").read_text(encoding="utf-8")
    assert "symbrain-cli" not in manifest and "symbrain-gateway" not in manifest, "standalone Guard depends on the Brain CLI"


# ---------------------------------------------------------------- negative controls
MUTANTS = {
    "help-text-drift": ([["scan", "--help"]], "out = out.replace(b'Discovers', b'Finds', 1)"),
    "silent-success-unported": ([["grants", "revoke"]], "out, err, code = b'', b'', 0"),
    "usage-exit-drift": ([[]], "code = 0 if len(sys.argv) == 1 else code"),
    "flag-output-drift": ([["version", "--json"]], "out = out.replace(b'\"tool\"', b'\"Tool\"', 1)"),
}


def run_controls(runner, go, rust, probe_index, tmp):
    results = []
    for name, (targets, body) in MUTANTS.items():
        wrapper = Path(tmp) / f"{name}.py"
        wrapper.write_text("import subprocess, sys\n"
                           f"p = subprocess.run([{str(rust)!r}, *sys.argv[1:]], input=sys.stdin.buffer.read(), capture_output=True)\n"
                           "out, err, code = p.stdout, p.stderr, p.returncode\n"
                           f"{body}\n"
                           "sys.stdout.buffer.write(out); sys.stderr.buffer.write(err); sys.exit(code)\n",
                           encoding="utf-8")
        for target in targets:
            left = probe_index[tuple(target)]["go"]
            right = runner.run(wrapper, target, True)
            try:
                assert replay.comparable(left) == replay.comparable(right)
            except AssertionError:
                results.append(dict(control=name, args=target, rejected=True, wrapper_sha256=digest(wrapper.read_bytes())))
            else:
                raise AssertionError("command-tree mutant accepted: " + name)
    return results


def run_fixture_control(inventory, fixture):
    """The fixture check itself must reject an altered help line and a dropped path."""
    rejected = []
    for name in ["help-line", "dropped-path", "exit-code"]:
        mutated = json.loads(json.dumps(fixture))
        if name == "help-line":
            node = next(n for n in mutated["tree"] if n["path"] == ["scan"])
            node["help_text"] = node["help_text"].replace("Discovers", "Finds", 1)
        elif name == "dropped-path":
            mutated["tree"] = [n for n in mutated["tree"] if n["path"] != ["grants", "revoke"]]
        else:
            next(e for e in mutated["probes"] if e["args"] == [])["exit_code"] = 0
        try:
            fixture_diff(inventory, mutated)
        except AssertionError:
            rejected.append(name)
        else:
            raise AssertionError("fixture mutant accepted: " + name)
    return rejected


def main():
    write = "--write-fixture" in sys.argv
    go, rust, report, src = map(Path, [a for a in sys.argv[1:] if not a.startswith("--")])
    go, rust = go.resolve(strict=True), rust.resolve(strict=True)
    src = src.resolve(strict=True)
    assert_no_fallback()
    with tempfile.TemporaryDirectory(prefix="guard770-tree-") as owned:
        runner = Runner(owned)
        packages, dispatcher_words, flags, sub_words, nodes, plan = build_inventory(runner, go, src)
        every, reachable, users, update_invoked = internal_packages(src)
        assert set(every) == set(RUST_OWNERS), "guard/internal package set changed; update RUST_OWNERS"
        for owners in RUST_OWNERS.values():
            for owner in owners:
                assert (ROOT / owner).exists(), "missing Rust owner " + owner
        probes = run_probes(runner, go, rust, plan)
        inventory = deterministic(probes, nodes, flags, packages, dispatcher_words, sub_words, every, reachable, users, update_invoked)
        summary = summarize(probes, inventory["tree"])
        if write:
            FIXTURE.parent.mkdir(parents=True, exist_ok=True)
            FIXTURE.write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        fixture = json.loads(FIXTURE.read_text(encoding="utf-8"))
        fixture_diff(inventory, fixture)
        fixture_rejected = run_fixture_control(inventory, fixture)
        index = {tuple(p["args"]): p for p in probes}
        controls = run_controls(runner, go, rust, index, owned)
    output = dict(oracle_ref=REF, candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                  candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                  fixture_sha256=digest(FIXTURE.read_bytes()), summary=summary,
                  binaries_sha256=dict(go=digest(go.read_bytes()), rust=digest(rust.read_bytes())),
                  script_sha256=digest(Path(__file__).read_bytes()),
                  library_only_packages=[l["package"] for l in inventory["library_packages"] if not l["import_reachable_from_dispatcher"]],
                  library_without_rust_owner=[l["package"] for l in inventory["library_packages"] if not l["rust_owners"]],
                  control_mutants_rejected=controls, fixture_mutants_rejected=fixture_rejected,
                  probes=[dict(owner=p["owner"], args=p["args"], status=p["status"]) for p in probes])
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    assert not summary["paths_unported"], f"unported symguard paths: {summary['paths_unported']}"
    assert summary["probes_native"] == summary["probes"], "probe mismatch: " + str(
        [p["args"] for p in probes if p["status"] != "native"])
    print(f"symguard command tree: {summary['paths_total']} paths, {summary['paths_native']} native, "
          f"{len(summary['paths_unported'])} unported, {summary['paths_fallback']} fallback; "
          f"{summary['probes']} Go/native probes matched; {len(controls)} process mutants and "
          f"{len(fixture_rejected)} fixture mutants rejected")


if __name__ == "__main__":
    main()
