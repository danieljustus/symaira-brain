"""PREPARED actual processes, full reports/state and targeted corruption controls.

Compiler/timeout/disk/binary archival ownership is a separate Root allocation.
This driver never compiles and never substitutes expected importer algorithms.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys

import retention_cases
import state


def run(argv, destination, env):
    child = subprocess.run(list(map(str, argv)), env=env, capture_output=True, timeout=120)
    destination.with_suffix(".stdout").write_bytes(child.stdout)
    destination.with_suffix(".stderr").write_bytes(child.stderr)
    destination.with_suffix(".exit").write_text(str(child.returncode)+"\n")
    assert child.returncode == 0, f"process/bootstrap failure is not a valid mutation: {argv}"
    return child.stdout


def database(path):
    def value(v):
        if isinstance(v, bytes): return ["blob", v.hex()]
        if v is None: return ["null", None]
        if isinstance(v, str): return ["text", v.encode().hex()]
        return [type(v).__name__, v]
    with sqlite3.connect(f"file:{path}?mode=ro", uri=True) as conn:
        tables = conn.execute("SELECT name,sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name").fetchall()
        result = []
        for name, schema in tables:
            quoted = '"'+name.replace('"', '""')+'"'
            rows = [[value(v) for v in row] for row in conn.execute("SELECT * FROM "+quoted)]
            result.append({"table": name, "schema": schema, "rows": sorted(rows, key=lambda row:json.dumps(row,sort_keys=True))})
        return result


def main():
    assert os.environ.get("IMPORTER761_RUNTIME_ALLOCATED") == "1", "explicit Root runtime allocation required"
    parser = argparse.ArgumentParser()
    for key in ("go-importer", "native-importer", "go-retention", "native-retention", "work-root"):
        parser.add_argument("--"+key, required=True, type=Path)
    args = parser.parse_args()
    root = args.work_root.resolve()
    assert root.is_relative_to('/workspace') and not root.exists()
    root.mkdir(mode=0o700)
    env = {k:v for k,v in os.environ.items() if not k.startswith(("SYMBRAIN_", "SYMMEMORY_", "OPENAI_", "ANTHROPIC_"))}
    env.update(HOME=str(root/"home"), XDG_CONFIG_HOME=str(root/"config"), XDG_DATA_HOME=str(root/"data"), TZ="UTC")
    for name in ("home", "config", "data"):(root/name).mkdir()
    here = Path(__file__).resolve().parent
    run([sys.executable, here/"corrections.py", root/"fixtures"], root/"materialize", env)
    packets = json.loads((root/"fixtures/input.json").read_bytes())
    assert len(packets) == 93
    before = state.snapshot(root/"fixtures")
    original_input = root/"fixtures/input.json"
    go = run([args.go_importer, original_input], root/"go-importer", env)
    native = run([args.native_importer, original_input], root/"native-importer", env)
    assert state.snapshot(root/"fixtures") == before, "constructor peers wrote owned source state"
    (root/"readonly-state.json").write_text(json.dumps(before,indent=2)+"\n")
    run([sys.executable, here/"compare.py", original_input, root/"go-importer.stdout", root/"native-importer.stdout"], root/"compare-importers", env)
    # Actual current native process with corrupted root spelling; expected Go
    # input/output remain unchanged. Generated child paths are identical here.
    mutated = copy.deepcopy(packets)
    dot = next(p for p in mutated if p["Id"].endswith("-dot-root"))
    raw = bytes.fromhex(dot["RootHex"]);assert raw.endswith(b"/.")
    dot["RootHex"] = raw[:-2].hex()
    altered = root/"root-mutant.json";altered.write_text(json.dumps(mutated,indent=2)+"\n")
    bad = run([args.native_importer, altered], root/"root-mutant", env)
    assert [r["id"] for r in map(json.loads,bad.splitlines())] == [p["Id"] for p in packets]
    rejected = subprocess.run([sys.executable, str(here/"compare.py"), str(original_input), str(root/"go-importer.stdout"), str(root/"root-mutant.stdout")],env=env,capture_output=True)
    (root/"root-control.stdout").write_bytes(rejected.stdout);(root/"root-control.stderr").write_bytes(rejected.stderr)
    assert rejected.returncode != 0 and b"full constructor report mismatch" in rejected.stderr
    # Mutate only real process-produced embedded metadata bytes, retaining all
    # bootstrap facts, IDs and source files. Literal U+FFFD is not normalized.
    records = list(map(json.loads,native.splitlines()));changed = 0
    target = next(r for r in records if r["id"].endswith("-curated-memory-invalid-frontmatter"))
    for item in target["imports"]:
        for fact in item["facts"] or []:
            for pair in fact["metadata_hex"] or []:
                if pair[0] == b"frontmatter".hex():
                    raw = bytes.fromhex(pair[1]);assert b"\\ufffd" in raw
                    pair[1] = raw.replace(b"\\ufffd", b"\xef\xbf\xbd").hex();changed += 1
    assert changed == 2
    rendered = root/"json-mutant.jsonl";rendered.write_text(''.join(json.dumps(r)+"\n" for r in records))
    rejected = subprocess.run([sys.executable,str(here/"compare.py"),str(original_input),str(root/"go-importer.stdout"),str(rendered)],env=env,capture_output=True)
    (root/"json-control.stdout").write_bytes(rejected.stdout);(root/"json-control.stderr").write_bytes(rejected.stderr)
    assert rejected.returncode != 0 and b"full constructor report mismatch" in rejected.stderr
    retention = []
    for packet in retention_cases.cases():
        owned = root/packet["ID"];owned.mkdir()
        input_path = owned/"input.json";input_path.write_text(json.dumps(packet,indent=2)+"\n")
        seed = owned/"seed.db"
        run([args.go_retention,"seed",input_path,seed],owned/"seed",env)
        original = database(seed)
        peers = []
        for name, binary in [("go", args.go_retention), ("native", args.native_retention)]:
            path = owned/(name+".db");shutil.copyfile(seed,path)
            assert database(path) == original
            argv = [binary,"apply",input_path,path] if name == "go" else [binary,input_path,path]
            report = json.loads(run(argv,owned/name,env));peers.append((report,database(path)))
        assert peers[0] == peers[1], f"full retention report/state mismatch: {packet['ID']}"
        (owned/"full-state.json").write_text(json.dumps({"before":original,"go":peers[0],"native":peers[1]},indent=2)+"\n")
        retention.append(packet["ID"])
        if packet["ID"] == "R761-expire-600":
            corrupted = dict(packet,At="2026-01-02T00:00:00Z")
            bad_input = owned/"mutant-input.json";bad_input.write_text(json.dumps(corrupted,indent=2)+"\n")
            path = owned/"mutant.db";shutil.copyfile(seed,path)
            bad_report = json.loads(run([args.native_retention,bad_input,path],owned/"mutant",env))
            assert bad_report["id"] == packet["ID"] and not bad_report["error"]
            assert (bad_report,database(path)) != peers[0], "whole-second query corruption was undetected"
    assert state.snapshot(root/"fixtures") == before
    binaries = {str(p.resolve()):hashlib.sha256(p.read_bytes()).hexdigest() for p in
                (args.go_importer,args.native_importer,args.go_retention,args.native_retention)}
    (root/"receipt.json").write_text(json.dumps({"status":"actual-full-reports-and-state-pass", "constructor_cases":93,
        "original_constructor_cases_retained":70,"retention_cases":retention,"actual_input_controls":2,
        "actual_report_byte_corruption_controls":1,"binaries_sha256":binaries,"native_three_os":"pending"},indent=2)+"\n")


if __name__ == "__main__": main()
