#!/usr/bin/env python3
"""Scoped actual Go/native standalone Guard proof; retains unresolved states."""
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
REF = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
ANCHORS = {
    "null": b"null", "array": b"[]", "number": b"1", "string": b'"text"', "bool": b"true",
    "string-null": b'{"last_entry_hash":null}', "string-number": b'{"last_entry_hash":1}',
    "string-bool": b'{"content_hash":false}', "string-object": b'{"content_hash":{}}',
    "count-null": b'{"entry_count":null}', "count-bool": b'{"entry_count":false}',
    "count-array": b'{"entry_count":[]}', "count-object": b'{"entry_count":{}}',
    "count-max": b'{"entry_count":9223372036854775807}',
    "count-min": b'{"entry_count":-9223372036854775808}',
    "count-overflow": b'{"entry_count":9223372036854775808}',
    "count-decimal": b'{"entry_count":1.0}', "count-exponent": b'{"entry_count":1e0}',
    "count-negative-zero": b'{"entry_count":-0}',
    "schema-wide": b'{"schema_version":2147483648}', "schema-null": b'{"schema_version":null}',
    "schema-string": b'{"schema_version":"wrong"}', "log-string": b'{"log_size":"wrong"}',
    "case-fold": b'{"ENTRY_COUNT":"wrong"}',
    "unicode-fold": '{"laſt_entry_haſh":false}'.encode(),
    "duplicate-first-error": b'{"entry_count":"wrong","entry_count":1}',
    "duplicate-later-error": b'{"entry_count":1,"entry_count":"wrong"}',
    "multiple-first-error": b'{"content_hash":false,"entry_count":"wrong"}',
    "unknown-overflow": b'{"unknown":1e309}',
    "unknown-depth": b'{"unknown":' + b'[' * 150 + b'0' + b']' * 150 + b'}',
}


def digest(data):
    return hashlib.sha256(data).hexdigest()


def cases():
    result = []
    def add(name, args, state="empty", contract="parity", payload=b""):
        result.append(dict(id=name, args=args, state=state, contract=contract, payload=payload))
    for name, args in [("empty", []), ("help", ["help"]), ("help-long", ["--help"]),
                       ("help-short", ["-h"]), ("unknown", ["unknown"]),
                       ("unreachable-proxy", ["proxy"]), ("unreachable-update", ["update"])]:
        add(name, args)
    for name, args in [("version", []), ("version-json", ["--json"]),
                       ("version-ignored", ["--unknown"]), ("version-json-after-stop", ["--", "--json"])]:
        add(name, ["version", *args])
    for name, flags in [("default", []), ("json", ["--format", "json"]),
                        ("table", ["--format=table"]), ("help", ["--help"]),
                        ("short-help", ["-h"]), ("missing", ["--format"]),
                        ("bad-format", ["--format=unknown"]), ("unknown", ["--unknown"]),
                        ("empty-format", ["--format="])]:
        add("scan-" + name, ["scan", *flags])
    for state in ["discovery", "jsonc", "yaml"]:
        add("scan-" + state, ["scan", "--format=json"], state)
    for name, args in [("help", []), ("explicit-help", ["help"]), ("list-empty", ["list"]),
                       ("unknown", ["unknown"]), ("missing-revoke", ["revoke"]),
                       ("all-empty", ["revoke", "--all"]), ("missing-id", ["revoke", "missing"]),
                       ("unexpected-flag", ["revoke", "--bad"]),
                       ("conflicting-all", ["revoke", "--all", "g1"]),
                       ("extra-id", ["revoke", "g1", "g2"])]:
        add("grants-" + name, ["grants", *args])
    for name, args in [("list", ["list"]), ("revoke-one", ["revoke", "g1"]),
                       ("revoke-all", ["revoke", "--all"])]:
        add("grants-seeded-" + name, ["grants", *args], "grants")
    for state in ["empty", "healthy", "missing-equals", "audit-plain", "audit-anchor", "audit-corrupt", "discovery"]:
        add("doctor-" + state, ["doctor", "--ignored"], state)
    for state in ["invalid-default", "invalid-threshold", "rule-missing-decision", "rule-invalid-decision",
                  "rule-empty-match", "rule-missing-match", "spawn-missing-path", "spawn-relative-path",
                  "sequence-zero", "sequence-negative", "sequence-disabled-negative", "quoted-default",
                  "rule-command-match"]:
        add("doctor-" + state, ["doctor"], state)
    for state in ["invalid-anchor-shape", "decode-before-validation", "multiple-invalid-defaults", "unknown-key"]:
        add("doctor-unported-" + state, ["doctor"], state,
            "parity" if state in ["invalid-anchor-shape", "unknown-key"] else "native-fail-closed")
    for state in ANCHORS:
        add("doctor-anchor-" + state, ["doctor"], "anchor:" + state)
    add("decide-help", ["decide", "--help"])
    fixture = json.loads((ROOT / "guard/scripts/guard-decide-oracle/cases.json").read_text())
    for item in fixture["cases"]:
        if item["id"] == "output-failure":
            # Platform-specific broken-output semantics remain separate; not counted.
            continue
        payload = json.dumps(item.get("request", {}), separators=(",", ":")).encode()
        if "raw" in item:
            payload = item["raw"].encode()
        if "raw_repeat_count" in item:
            payload = (item["raw_prefix"] + item["raw_repeat"] * item["raw_repeat_count"] + item["raw_suffix"]).encode()
        if item["id"] == "deadline-equality-at-launch":
            payload = b"LAUNCH_DEADLINE"
        state = "audit-failure" if item["id"] == "audit-failure" else "empty"
        contract = "audit-fail-closed" if state == "audit-failure" and os.name == "nt" else "parity"
        add("decide-" + item["id"], ["decide"], state, contract, payload)
    return result


def setup(root, state):
    home = root / "home"
    for path in [home, root / "config", root / "data", root / "cache", root / "project"]:
        path.mkdir(parents=True, exist_ok=True)
    for part in [".config/hermes", ".cursor", ".vscode", ".config/opencode", ".config/claude",
                 "Library/Application Support/Claude"]:
        (home / part).mkdir(parents=True)
    env = dict(HOME=str(home), USERPROFILE=str(home), XDG_CONFIG_HOME=str(home / ".config"),
               XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
               PATH="", TZ="UTC", LANG="C.UTF-8")
    for key in ["SystemRoot", "WINDIR", "TMP", "TEMP"]:
        if key in os.environ:
            env[key] = os.environ[key]
    config = home / ".config/symguard/config.toml"
    def write(path, text):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
    configs = {"healthy": '[defaults]\nread="allow"\n', "missing-equals": 'invalid TOML\n',
               "invalid-default": '[defaults]\nread="invalid"\n',
               "invalid-threshold": '[sequence]\nenabled=true\nthreshold=1\n',
               "rule-missing-decision": '[[rules]]\nmatch.server="fixture"\n',
               "rule-invalid-decision": '[[rules]]\ndecision="invalid"\nmatch.tool="tool"\n',
               "rule-empty-match": '[[rules]]\ndecision="allow"\nmatch.server=""\n',
               "rule-missing-match": '[[rules]]\ndecision="allow"\n',
               "spawn-missing-path": '[[spawn.allowlist]]\nargv_prefix=["fixture"]\n',
               "spawn-relative-path": '[[spawn.allowlist]]\npath="fixture"\n',
               "sequence-zero": '[sequence]\nenabled=true\nthreshold=0\n',
               "sequence-negative": '[sequence]\nenabled=true\nthreshold=-1\n',
               "sequence-disabled-negative": '[sequence]\nenabled=false\nthreshold=-1\n',
               "quoted-default": '[defaults]\n"weird\\nkey"="invalid\\tvalue"\n',
               "rule-command-match": '[[rules]]\ndecision="allow"\nmatch.command_contains=["fixture"]\n',
               "decode-before-validation": '[defaults]\nshell="invalid"\n[audit]\nencrypt="wrong"\n',
               "multiple-invalid-defaults": '[defaults]\nfirst="invalid"\nsecond="invalid"\n',
               "unknown-key": '[sequence]\nenabled=true\nthreshold=3\nunknown="fixture"\n'}
    if state in configs:
        write(config, configs[state])
    if state in ["audit-plain", "audit-anchor", "audit-corrupt", "invalid-anchor-shape"]:
        write(root / "data/symguard/audit.log", '{}\n')
        if state != "audit-plain":
            value = {"audit-anchor": '{}', "audit-corrupt": '{bad',
                     "invalid-anchor-shape": '{"entry_count":"wrong"}'}[state]
            write(root / "data/symguard/audit.log.anchor", value)
    if state == "audit-failure":
        (root / "data/symguard/audit.log").mkdir(parents=True)
    if state.startswith("anchor:"):
        write(root / "data/symguard/audit.log", '{}\n')
        (root / "data/symguard/audit.log.anchor").write_bytes(ANCHORS[state.removeprefix("anchor:")])
    if state == "grants":
        record = dict(id="g1", scope="session", subject="fixture", origin=dict(epoch=1, via="human"),
                      granted_at="2026-01-01T00:00:00Z", expires_at="2999-01-01T00:00:00Z")
        write(root / "data/symguard/grants.json", json.dumps([record]))
    if state in ["discovery", "jsonc", "yaml"]:
        value = {"mcpServers": {"fixture": {"command": "/fixture/bin", "args": ["--fixture"],
                 "env": {"TOKEN": "SYNTHETIC_SECRET_SENTINEL"}}}}
        text = json.dumps(value)
        if state == "jsonc":
            text = "{ // synthetic fixture\n" + text[1:]
        if state == "yaml":
            text = 'mcpServers:\n  fixture:\n    command: /fixture/bin\n    env:\n      TOKEN: SYNTHETIC_SECRET_SENTINEL\n'
        write(home / ".cursor/mcp.json", text)
    return env


def normalized_stream(data, root, args, native, is_stdout):
    value = data.replace(os.fsencode(root), b"<root>")
    if is_stdout and args and args[0] == "version" and "--json" not in args:
        pattern = rb"(?m)^  rust    rustc [^\n]+$" if native else rb"(?m)^  go      go1\.26\.7$"
        value, count = re.subn(pattern, b"  toolchain <actual>", value)
        assert count == 1, "actual version SDK line missing or changed"
    if is_stdout and args and args[0] == "doctor":
        pattern = rb"(?m)^  Rust:      rustc [^\n]+$" if native else rb"(?m)^  Go:        go1\.26\.7$"
        if value:
            value, count = re.subn(pattern, b"  Toolchain: <actual>", value)
            assert count == 1, "actual doctor SDK line missing or changed"
    return value


def state_files(root, started, ended, is_decide):
    files = {}
    for path in sorted(root.rglob("*")):
        info = path.lstat()
        name = str(path.relative_to(root)).replace("\\", "/")
        if not stat.S_ISREG(info.st_mode):
            continue
        raw = path.read_bytes()
        normalized = raw
        if is_decide and name == "data/symguard/audit.log":
            records = [json.loads(line) for line in raw.splitlines()]
            assert len(records) == 1, "expected exactly one real audit record"
            record = records[0]
            stamp = record["decided_at"]
            instant = datetime.datetime.fromisoformat(stamp.replace("Z", "+00:00")).timestamp()
            assert started - 2 <= instant <= ended + 2
            ident = record["id"]
            assert ident.startswith("evt_1_decide_")
            nanos = int(ident.removeprefix("evt_1_decide_")) / 1e9
            assert started - 2 <= nanos <= ended + 2
            assert abs(nanos - instant) < 1.1, "audit ID and timestamp refer to different runtime instants"
            for field, val, token in [("id", ident, "<runtime-id>"), ("decided_at", stamp, "<runtime-time>")]:
                operand = ('"' + field + '":' + json.dumps(val)).encode()
                assert normalized.count(operand) == 1
                normalized = normalized.replace(operand, ('"' + field + '":"' + token + '"').encode(), 1)
        files[name] = dict(mode=stat.S_IMODE(info.st_mode) if os.name != "nt" else None,
                           raw_hex=raw.hex(), normalized_hex=normalized.hex())
    return files


def observe(binary, case, root, native):
    env = setup(root, case["state"])
    payload = case["payload"]
    if payload == b"LAUNCH_DEADLINE":
        stamp = datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")
        payload = json.dumps(dict(command="open", risk_class="low", deadline=stamp)).encode()
    start = time.time()
    command = [str(binary), *case["args"]]
    if binary.suffix == ".py":
        command.insert(0, sys.executable)
    p = subprocess.run(command, cwd=root / "project", env=env,
                       input=payload, capture_output=True, timeout=5)
    end = time.time()
    return dict(exit_code=p.returncode, stdout_hex=p.stdout.hex(), stderr_hex=p.stderr.hex(),
                normalized_stdout_hex=normalized_stream(p.stdout, root, case["args"], native, True).hex(),
                normalized_stderr_hex=normalized_stream(p.stderr, root, case["args"], native, False).hex(),
                files=state_files(root, start, end, case["args"][0] == "decide" if case["args"] else False))


def comparable(o):
    return o["exit_code"], o["normalized_stdout_hex"], o["normalized_stderr_hex"], {
        k: (v["mode"], v["normalized_hex"]) for k, v in o["files"].items()}


def compare(case, go, rust):
    if case["contract"] == "native-fail-closed":
        assert rust["exit_code"] == 1
        if case["state"] != "unknown-key":
            assert go["exit_code"] == 1
        assert rust["stdout_hex"] == ""
        assert bytes.fromhex(rust["stderr_hex"]) == b"symguard doctor: unsupported native diagnostic state; no legacy fallback is available\n"
        return "native-fail-closed-remaining-port"
    if case["contract"] == "audit-fail-closed":
        for o in [go, rust]:
            assert o["exit_code"] == 0 and o["stderr_hex"] == ""
            reply = json.loads(bytes.fromhex(o["stdout_hex"]))
            assert reply["decision"] == "deny" and reply["reason"].startswith("audit: write decision record:")
        return "audit-fail-closed-existing-diagnostic-deviation"
    assert comparable(go) == comparable(rust), "stdout/stderr/exit/state differs"
    if case["args"] and case["args"][0] == "scan":
        assert b"SYNTHETIC_SECRET_SENTINEL" not in bytes.fromhex(rust["stdout_hex"])
    return "matched"


def main():
    go, rust, report, archive = map(Path, sys.argv[1:])
    go, rust = go.resolve(strict=True), rust.resolve(strict=True)
    names = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", REF], cwd=ROOT, text=True).splitlines()
    source_hashes = {}
    for name in names:
        if not (name.endswith(".go") or name in ["go.mod", "go.sum"]):
            continue
        expected = subprocess.check_output(["git", "show", REF + ":" + name], cwd=ROOT)
        assert (archive / name).read_bytes() == expected, "Go production source changed: " + name
        source_hashes[name] = digest(expected)
    results = []
    selected = cases()
    assert len(selected) == 94 + len(ANCHORS) and len({c["id"] for c in selected}) == len(selected)
    with tempfile.TemporaryDirectory(prefix="guard770-process-") as owned:
        for index, case in enumerate(selected):
            base = Path(owned) / str(index)
            go_out = observe(go, case, base / "go", False)
            rust_out = observe(rust, case, base / "rust", True)
            try:
                disposition = compare(case, go_out, rust_out)
            except AssertionError as error:
                disposition = "failed: " + str(error)
            results.append(dict(id=case["id"], args=case["args"], state=case["state"],
                                contract=case["contract"], disposition=disposition, go=go_out, rust=rust_out))
    output = dict(oracle_ref=REF, candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
                  candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
                  go_version=subprocess.check_output([str(go), "version"], text=True).splitlines()[1].strip(),
                  candidate_source_sha256={str(path.relative_to(ROOT)): digest(path.read_bytes())
                      for crate in ["symguard-cli", "symbrain-core", "symbrain-audit", "symbrain-guard-core"]
                      for path in sorted((ROOT / "rust" / crate).rglob("*")) if path.is_file() and path.suffix in [".rs", ".toml"]},
                  candidate_manifest_sha256={name: digest((ROOT / name).read_bytes()) for name in ["Cargo.toml", "Cargo.lock", "rust/symbrain-cli/Cargo.toml"]},
                  go_source_sha256=source_hashes, supplemental_go_entry_sha256=digest((archive / "oracle770/main.go").read_bytes()),
                  binaries_sha256=dict(go=digest(go.read_bytes()), rust=digest(rust.read_bytes())),
                  total=len(results), matched=sum(r["disposition"] == "matched" for r in results),
                  supplemental_anchor_inputs_hex={k: v.hex() for k, v in ANCHORS.items()},
                  remaining_native_diagnostic_states=[r["id"] for r in results if r["contract"] == "native-fail-closed"],
                  audit_diagnostic_deviations=[r["id"] for r in results if r["contract"] == "audit-fail-closed"], results=results,
                  explicit_limits=["unported doctor diagnostic states", "inherited decision audit failure diagnostic",
                                   "platform-specific broken output", "full malformed discovery/config error boundaries",
                                   "unshipped library capabilities are not CLI paths"])
    report.parent.mkdir(parents=True, exist_ok=True)
    report.write_text(json.dumps(output, indent=2) + "\n")
    failures = [r["id"] + ": " + r["disposition"] for r in results if r["disposition"].startswith("failed")]
    assert not failures, "\n".join(failures)
    print(f"{len(results)} actual standalone process cases; {output['matched']} full matches; {len(output['remaining_native_diagnostic_states'])} remaining doctor diagnostic states and {len(output['audit_diagnostic_deviations'])} audit diagnostic deviations explicitly retained")


if __name__ == "__main__":
    main()
