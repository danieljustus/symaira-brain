#!/usr/bin/env python3
"""Real owned Go/native diagnostic paths; Unix bytes and native Unicode separately."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import replay
import kernel_admission
from config_path_journal import Journal

KINDS = ["config-invalid-default", "config-missing-equals", "anchor-overflow",
         "audit-directory-low", "audit-directory-medium", "audit-directory-high",
         "audit-directory-malformed"]
COMPONENTS = ["owned-\ufffd".encode(), "owned-\u2028\u2029".encode()]
UNIX_COMPONENTS = [b"owned-\xe2\x82", b"owned-\xff", b"owned-\xf0\x80\x80",
                   b"owned-\xed\xa0\x80", b"owned-\xc0\xaf", b"owned-<&>",
                   b'owned-"\\']


def observe(binary, kind, component, root, native, journal=None):
    env = replay.setup(root, "empty")
    part = root / os.fsdecode(component)
    args = ["decide"] if kind.startswith("audit-directory") else ["doctor"]
    if kind.startswith("config"):
        path = part / "config.toml"
        path.parent.mkdir()
        path.write_bytes(b'[defaults]\nread="bad"\n' if kind.endswith("default") else b"invalid TOML\n")
        env["SYMGUARD_CONFIG"] = str(path)
    else:
        env["XDG_DATA_HOME"] = str(part)
        log = part / "symguard/audit.log"
        log.parent.mkdir(parents=True)
        if args == ["decide"]:
            log.mkdir()
        else:
            log.write_bytes(b"{}\n")
            Path(str(log) + ".anchor").write_bytes(b'{"entry_count":9223372036854775808}')
    payload = b""
    if args == ["decide"]:
        risk = kind.removeprefix("audit-directory-")
        payload = b"not json" if risk == "malformed" else json.dumps(
            dict(command="owned", risk_class=risk), separators=(",", ":")).encode()
    command = [str(binary), *args]
    if binary.suffix == ".py":
        command.insert(0, sys.executable)
    start = time.time()
    if journal:
        journal.event('child-start', binary=str(binary), native=native, kind=kind,
                      component_hex=component.hex(), argv=command, input_hex=payload.hex(),
                      cwd_bytes_hex=os.fsencode(root/'project').hex(),
                      environment_bytes={key:os.fsencode(value).hex() for key,value in env.items()})
    try:
        process = subprocess.run(command, cwd=root / "project", env=env, input=payload,
                                 capture_output=True, timeout=5)
    except BaseException as error:
        if journal:
            journal.event('child-exception', binary=str(binary), native=native, kind=kind,
                          component_hex=component.hex(), exception_type=type(error).__name__, exception_repr=repr(error),
                          stdout_hex=None if getattr(error,'stdout',None) is None else error.stdout.hex(),
                          stderr_hex=None if getattr(error,'stderr',None) is None else error.stderr.hex())
        raise
    if journal:
        journal.event('child-returned', binary=str(binary), native=native, kind=kind,
                      component_hex=component.hex(), exit_code=process.returncode,
                      stdout_hex=process.stdout.hex(), stderr_hex=process.stderr.hex())
    end = time.time()
    return dict(exit_code=process.returncode, stdout_hex=process.stdout.hex(), stderr_hex=process.stderr.hex(),
                normalized_stdout_hex=replay.normalized_stream(process.stdout, root, args, native, True).hex(),
                normalized_stderr_hex=replay.normalized_stream(process.stderr, root, args, native, False).hex(),
                files=replay.state_files(root, start, end, False))


def compare(kind, go, rust):
    if os.name == "nt" and kind in ["audit-directory-low", "audit-directory-medium", "audit-directory-high"]:
        # The inherited Windows syscall spelling remains an explicit deviation.
        return replay.compare(dict(contract="audit-fail-closed"), go, rust)
    assert replay.comparable(go) == replay.comparable(rust), "raw diagnostic stdout/stderr/exit/state differs"
    assert rust["stderr_hex"] == "" and rust["exit_code"] == (0 if kind.startswith("audit-directory") else 1)
    return "matched"


def controls(go, rust, root, admission, journal=None):
    if os.name == "nt":
        return [], "Unix byte-path mutations cannot be created with native UTF-16 paths"
    if not admission['admitted']:
        return [], 'Actual owned Darwin EILSEQ92; both original filename controls UNEXECUTED'
    rows = []
    for mode, kind in [("lossy-human", "config-invalid-default"), ("lossy-json", "audit-directory-low")]:
        wrapper = root / (mode + ".py")
        wrapper.write_text("""import subprocess,sys
p=subprocess.run([NATIVE,*sys.argv[1:]],input=sys.stdin.buffer.read(),capture_output=True)
assert not p.stderr
raw=p.stdout
if MODE=='lossy-human':
    assert p.returncode==1 and b'\\xe2\\x82' in raw
    raw=raw.decode('utf-8',errors='replace').encode('utf-8')
else:
    assert p.returncode==0 and b'\\\\ufffd\\\\ufffd' in raw
    raw=raw.replace(b'\\\\ufffd\\\\ufffd','\\ufffd'.encode('utf-8'))
sys.stdout.buffer.write(raw);sys.exit(p.returncode)
""".replace("NATIVE", repr(str(rust))).replace("MODE", repr(mode)), encoding="utf-8")
        left = observe(go, kind, b"owned-\xe2\x82", root / mode / "go", False, journal)
        right = observe(wrapper, kind, b"owned-\xe2\x82", root / mode / "rust", True, journal)
        assert not right["stderr_hex"] and right["stdout_hex"], "mutant must produce intended response"
        try:
            compare(kind, left, right)
        except AssertionError:
            rows.append(dict(id=mode, rejected=True, go=left, mutated_native=right,
                             wrapper_sha256=replay.digest(wrapper.read_bytes())))
        else:
            raise AssertionError("accepted raw-path mutant: " + mode)
    return rows, None


def run(go, rust, report, journal):
    results, unexecuted, admissions = [], [], []
    components = COMPONENTS + (UNIX_COMPONENTS if os.name != "nt" else [])
    with tempfile.TemporaryDirectory(prefix="guard770-raw-paths-") as owned:
        root = Path(owned)
        admitted = {}
        for component in components:
            admission = kernel_admission.probe(component, root, journal)
            admissions.append(admission)
            admitted[component] = admission
        requested = [kind+':'+component.hex() for component in components for kind in KINDS]
        for index, (component, kind) in enumerate((c, k) for c in components for k in KINDS):
            case_id = kind+':'+component.hex()
            if not admitted[component]['admitted']:
                row = kernel_admission.unavailable(case_id, admitted[component])
                unexecuted.append(row)
                journal.event('case-unexecuted', observation=row)
                continue
            journal.event('case-start', id=case_id, component_hex=component.hex(), kind=kind)
            pair = {side: observe(binary, kind, component, root / str(index) / side, native, journal)
                    for side, binary, native in [("go", go, False), ("rust", rust, True)]}
            try:
                disposition = compare(kind, pair["go"], pair["rust"])
            except AssertionError as error:
                disposition = "failed: " + str(error)
            results.append(dict(id=case_id, kind=kind, component_hex=component.hex(), disposition=disposition, **pair))
            journal.event('pair-complete', observation=results[-1])
        control_ids = [] if os.name == 'nt' else ['lossy-human', 'lossy-json']
        control_admission = admitted.get(b'owned-\xe2\x82')
        mutations, skipped_controls = controls(go, rust, root, control_admission, journal)
        unavailable_controls = [kernel_admission.unavailable(name, control_admission) for name in control_ids
                                if not control_admission['admitted']]
        ledger = kernel_admission.accounting(requested, results, unexecuted, control_ids, mutations, unavailable_controls)
        account_control = kernel_admission.accounting_control(requested, results, unexecuted, control_ids, mutations, unavailable_controls)
        journal.event('domain-accounted', accounting=ledger, accounting_control=account_control)
    output = dict(candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=replay.ROOT, text=True).strip(),
                  candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=replay.ROOT)),
                  binaries_sha256={"go": replay.digest(go.read_bytes()), "rust": replay.digest(rust.read_bytes())},
                  runner_sha256=replay.digest(Path(__file__).read_bytes()),
                  admission_source_sha256=replay.digest(Path(__file__).with_name('kernel_admission.py').read_bytes()),
                  total=len(results), results=results, kernel_admissions=admissions,
                  unavailable_results=unexecuted, unavailable_controls=unavailable_controls,
                  accounting=ledger, accounting_control=account_control,
                  matched=sum(row["disposition"] == "matched" for row in results), controls=mutations,
                  unix_raw_paths_skipped=os.name == "nt", skipped_controls=skipped_controls,
                  limits=["native Windows UTF-16 projection requires its own runner", "inherited Windows directory-open spelling remains explicitly scoped"])
    report.write_text(json.dumps(output, indent=2) + "\n", encoding="utf-8")
    failures = [r for r in results if r["disposition"].startswith("failed")]
    assert not failures, [(r["kind"], r["component_hex"]) for r in failures]
    journal.complete(output)
    print(f"raw-path process cases {len(results)} passed; actual controls rejected {len(mutations)}")


def main():
    go, rust, report = map(Path, sys.argv[1:])
    go, rust = go.resolve(strict=True), rust.resolve(strict=True)
    journal = Journal(report, dict(runner_sha256=replay.digest(Path(__file__).read_bytes()),
                                  binaries_sha256=dict(go=replay.digest(go.read_bytes()), rust=replay.digest(rust.read_bytes()))))
    try:
        run(go, rust, report, journal)
    except BaseException as error:
        journal.failed(error)
        raise


if __name__ == "__main__":
    main()
