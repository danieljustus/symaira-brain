#!/usr/bin/env python3
"""Prepared native3 Skills764 differential gate using actual owned processes."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import tempfile
import time

from cases import OMIT_ARGS, cli_cases, config_cases, mcp_cases, mcp_config_cases, raw_cli_cases, correction_cases, raw_argument_cases
from compare import matched, mcp_view, filesystem
from fixtures import setup, variant
from output_cases import run_pairs as output_pairs, required_ids as output_ids
from library_denied import run_pairs as denied_pairs, required_ids as denied_ids
from byte_cases import cases as byte_cases, variant as byte_variant, input_description, run_controls as byte_controls

ROOT = Path(__file__).resolve().parents[2]


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def terminate(child, report):
    if child.poll() is not None:
        return
    if os.name == "nt":
        taskkill = Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"
        out = subprocess.run([str(taskkill), "/PID", str(child.pid), "/T", "/F"],
                             capture_output=True, timeout=10, check=False)
        report["cleanup"] = {"exit": out.returncode, "stdout_hex": out.stdout.hex(), "stderr_hex": out.stderr.hex()}
    else:
        try:
            os.killpg(child.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    if child.poll() is None:
        child.kill()
    child.wait(timeout=10)


class InvocationFailure(RuntimeError):
    def __init__(self, row):
        super().__init__("actual child failed before completed comparison")
        self.row = row


def invoke(binary, argv, env, root, incoming=None, retained_reads=None, record=None):
    row = {} if record is None else record
    row.update(argv=[str(binary), *argv], cwd=str(root / "project"), pid=None)
    options = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {"start_new_session": True}
    start = time.time()
    try:
        child = subprocess.Popen(row["argv"], cwd=root / "project", env=env,
                                 stdin=subprocess.PIPE if incoming is not None else subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.PIPE, **options)
    except OSError as error:
        row["launch_error"] = {"type": type(error).__name__, "message": str(error)}
        raise InvocationFailure(row) from error
    row["pid"] = child.pid
    try:
        stdout, stderr = child.communicate(input=incoming, timeout=20)
    except BaseException:
        terminate(child, row)
        stdout, stderr = child.communicate(timeout=10)
        row.update(exit=child.returncode, stdout_hex=stdout.hex(), stderr_hex=stderr.hex(), timed_out=True)
        row.update(started=start, finished=time.time(), filesystem=filesystem(root, start, time.time(), retained_reads))
        raise InvocationFailure(row)
    end = time.time()
    row.update(exit=child.returncode, stdout_hex=stdout.hex(), stderr_hex=stderr.hex(),
               started=start, finished=end, filesystem=filesystem(root, start, end, retained_reads))
    return row


def incoming(name, arguments, framed=False):
    args = arguments if isinstance(arguments, str) else json.dumps(arguments, separators=(",", ":")) if arguments is not OMIT_ARGS else None
    argument_field = ",\"arguments\":" + args if args is not None else ""
    data = (json.dumps({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "skills764-oracle", "version": "1"}}}) + "\n"
            + json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n"
            + json.dumps({"jsonrpc": "2.0", "id": 2, "method": "tools/list"}) + "\n"
            + '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":'
            + json.dumps(name) + argument_field + '}}\n').encode()
    if framed:
        return b"".join(b"Content-Length: " + str(len(line)).encode() + b"\r\n\r\n" + line
                        for line in data.splitlines())
    return data


def source_map():
    names = subprocess.check_output(["git", "ls-files", "rust/symbrain-skills", "rust/symbrain-cli",
                                     "rust/symbrain-gateway", "rust/symbrain-mcp", "scripts/skills-native-oracle", "Cargo.lock", "migration/contract-matrix.csv",
                                     ".github/workflows/skills-native.yml"], cwd=ROOT, text=True).splitlines()
    return {name: {"sha256": sha(ROOT / name), "bytes": (ROOT / name).stat().st_size} for name in names}


def actual_control(go, rust, root, mcp):
    if not matched(go, rust, root, mcp):
        raise AssertionError("control needs a real matching baseline first")
    return {"baseline_matched": True, "go_exit": go["exit"], "rust_exit": rust["exit"]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--git", type=Path, required=True)
    parser.add_argument("--git-fixture", type=Path, required=True)
    parser.add_argument("--oracle-receipt", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o022)
    output = args.out.absolute()
    if output.exists() or output.is_symlink() or output.is_relative_to(ROOT):
        raise ValueError("fresh external evidence path required")
    binaries = {name: getattr(args, name).resolve(strict=True) for name in ("go", "rust", "git", "git_fixture")}
    oracle = json.loads(args.oracle_receipt.read_text())
    assert oracle["status"] == "passed" and oracle["frozen"] == "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
    assert oracle["output_sha256"] == sha(binaries["go"])
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT), "clean candidate required"
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    report = {"candidate_head": head, "candidate_sources": source_map(), "native_platform": platform.platform(),
              "binaries": {name: {"path": str(path), "sha256": sha(path)} for name, path in binaries.items()},
              "oracle_receipt_sha256": sha(args.oracle_receipt), "results": [], "controls": [],
              "status": "running", "native_surface_gate_passed": False}
    try:
        with tempfile.TemporaryDirectory(prefix="skills-native-764-") as owned_name:
            owned = Path(owned_name).resolve()
            root, retained = owned / "case", owned / "retained"
            ledger = []
            report["fixture_commands"] = ledger
            report["owned_root"] = str(owned)
            env, ledger = setup(root, str(binaries["git"]), ledger)
            env["SKILLS_ACTUAL_GIT"] = str(binaries["git"])
            shutil.copytree(root, retained, symlinks=True)
            report["owned_root"] = str(owned)
            report["fixture_commands"] = ledger
            cases = [(name, argv, None, None) for name, argv in cli_cases()]
            cases += [(name, argv, None, None) for name, argv in raw_cli_cases()]
            cases += [(name, argv, None, selected) for name, argv, selected in config_cases()]
            cases += [(name, ["mcp", "--profile-file", str(root / "profile.toml")], incoming(tool, values), None)
                      for name, tool, values in mcp_cases(root)]
            cases += [(name, ["mcp", "--profile-file", str(root / "profile.toml")], incoming(tool, values), selected)
                      for name, tool, values, selected in mcp_config_cases(root)]
            cases += [(name, ["mcp", "--profile-file", str(root / "profile.toml")], incoming(tool, values), selected)
                      for name, tool, values, selected in correction_cases(root)]
            cases += [("mcp-framed-" + name, ["mcp", "--profile-file", str(root / "profile.toml")], incoming(tool, values, True), None)
                      for name, tool, values in raw_argument_cases()]
            cases += [(f"library-{selected}-{form}", ["skills", "list", *flags], None, selected)
                      for selected in ("empty-entry", "malformed-entry") for form, flags in (("table", []), ("json", ["--json"]))]
            cases += byte_cases(root, incoming)
            assert len({name for name, _, _, _ in cases}) == len(cases)
            report["required_ids"] = [name for name, _, _, _ in cases] + denied_ids() + output_ids()
            for name, argv, data, selected in cases:
                pair = {"id": name, "mcp": data is not None, "variant": selected,
                        "stdin_hex": data.hex() if data else None, "matched": False}
                report["results"].append(pair)
                output.write_text(json.dumps(report, indent=2) + "\n")
                for flavor in ("go", "rust"):
                    shutil.rmtree(root)
                    shutil.copytree(retained, root, symlinks=True)
                    case_env = byte_variant(root, env, selected) if selected and selected.startswith("byte-") else variant(root, env, selected, binaries["git_fixture"]) if selected else env
                    if name.startswith("byte-"):
                        pair.setdefault("input_environments", {})[flavor] = input_description(case_env)
                    pair[flavor] = {}
                    try:
                        invoke(binaries[flavor], argv, case_env, root, data, record=pair[flavor])
                        pair[flavor]["expected_skills"] = 0 if selected == "skills-disabled" else 11
                        provider_log = root / "tmp/git-provider.jsonl"
                        if provider_log.exists():
                            pair[flavor]["git_provider_argv_hex"] = provider_log.read_bytes().hex()
                    except InvocationFailure as error:
                        pair[flavor] = error.row
                        pair["matched"] = False
                        raise
                    finally:
                        output.write_text(json.dumps(report, indent=2) + "\n")
                try:
                    pair["matched"] = matched(pair["go"], pair["rust"], root, data is not None)
                except Exception as error:
                    pair["comparison_error"] = {"type": type(error).__name__, "message": str(error)}
                    raise
                output.write_text(json.dumps(report, indent=2) + "\n")
            denied_pairs(report, lambda: output.write_text(json.dumps(report, indent=2) + "\n"),
                         binaries, root, retained, env, invoke, incoming)
            output_pairs(report, lambda: output.write_text(json.dumps(report, indent=2) + "\n"),
                         binaries, root, retained, env, terminate)
            assert [row["id"] for row in report["results"]] == report["required_ids"]
            report["total"] = len(report["results"])
            report["matched"] = sum(row["matched"] for row in report["results"])
            if report["matched"] != report["total"]:
                raise AssertionError("actual output or filesystem mismatches retained")
            # Real negative controls change requests supplied to the actual
            # candidate, not stored result dictionaries/comparator predicates.
            controls = [("argv-target", ["skills", "status", "--target=invalid", "--json"], None),
                        ("argv-limit", ["skills", "log", "--limit=bad", "--json"], None),
                        ("mcp-type", ["mcp", "--profile-file", str(root / "profile.toml")], incoming("skills_history", {"name": 4}))]
            selectors = ["status-opencode", "log-0-1-j", "mcp-014-skills_history"]
            for (name, argv, data), selector in zip(controls, selectors):
                baseline = next(row for row in report["results"] if row["id"] == selector)
                bound = actual_control(baseline["go"], baseline["rust"], root, data is not None)
                shutil.rmtree(root)
                shutil.copytree(retained, root, symlinks=True)
                control = {"name": name, "baseline": selector, **bound, "rejected": False}
                report["controls"].append(control)
                control["candidate"] = {}
                mutant = invoke(binaries["rust"], argv, env, root, data, record=control["candidate"])
                output.write_text(json.dumps(report, indent=2) + "\n")
                rejected = not matched(baseline["go"], mutant, root, data is not None)
                control["rejected"] = rejected
                assert rejected and mutant["exit"] == (0 if data else 2), "bootstrap failure is not a negative control"
                if data:
                    reply = mcp_view(mutant)[1][2]["result"]
                    assert reply["isError"] is True and "parse arguments" in reply["content"][0]["text"]
                else:
                    assert not bytes.fromhex(mutant["stdout_hex"])
                    assert bytes.fromhex(mutant["stderr_hex"]), "actual argument rejection diagnostic required"
            byte_controls(report, lambda: output.write_text(json.dumps(report, indent=2) + "\n"),
                          binaries, root, retained, env, invoke, incoming, matched, mcp_view)
            assert report["candidate_sources"] == source_map()
            assert report["binaries"] == {name: {"path": str(path), "sha256": sha(path)} for name, path in binaries.items()}
            report.update(status="passed", native_surface_gate_passed=True)
    except BaseException as error:
        report.update(status="failed", error={"type": type(error).__name__, "message": str(error)})
        raise
    finally:
        output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
