#!/usr/bin/env python3
"""Five executable mutants of actual native process output must fail the gate."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("go", "rust", "go-source", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    runner = Path(__file__).with_name("fetch_control_process.py")
    changes = {
        "missing-case": "records = records[:-1]",
        "wrong-body": "\nfor r in records:\n if r.get('body_hex'): r['body_hex']='00'; break",
        "wrong-header": "\nfor r in records:\n if 'headers' in r: r['headers']['x-mutant']=['wrong']; break",
        "wrong-error": "\nfor r in records:\n if r.get('error') == 'too_large': r['error']='timeout'; break",
        "wrong-proxy-route": "\nfor r in records:\n if r.get('route'): r['route']='http://127.0.0.1:1'; break",
    }
    evidence = []
    with tempfile.TemporaryDirectory(prefix="fetch773-mutants-") as raw:
        for name, change in changes.items():
            wrapper = Path(raw) / (name + ".py")
            wrapper.write_bytes(("import json,subprocess,sys\n"
                f"p=subprocess.run([{str(args.rust.resolve())!r}],input=sys.stdin.buffer.read(),capture_output=True)\n"
                "sys.stderr.buffer.write(p.stderr)\n"
                "if p.returncode: sys.exit(p.returncode)\n"
                "records=[json.loads(line) for line in p.stdout.splitlines()]\n" + change +
                "\nfor r in records: print(json.dumps(r))\n").encode())
            result = subprocess.run([sys.executable, str(runner), "--go", str(args.go),
                "--rust", str(wrapper), "--go-source", str(args.go_source),
                "--output", str(Path(raw) / (name + ".json"))], capture_output=True, timeout=60)
            evidence.append(dict(name=name, rejected=result.returncode != 0, exit=result.returncode,
                stdout=result.stdout.decode(errors="replace"), stderr=result.stderr.decode(errors="replace"),
                mutant_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest()))
    assert len(evidence) == 5 and all(item["rejected"] for item in evidence), evidence
    repo = Path(__file__).resolve().parents[3]
    report = dict(controls=evidence, rejected=5,
        candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
        candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)),
        controls_script_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        native_binary_sha256=hashlib.sha256(args.rust.read_bytes()).hexdigest(),
        go_binary_sha256=hashlib.sha256(args.go.read_bytes()).hexdigest(),
        runner_sha256=hashlib.sha256(runner.read_bytes()).hexdigest())
    args.output.write_bytes((json.dumps(report, indent=2) + "\n").encode())
    print("Five native-output executable mutants rejected")


if __name__ == "__main__":
    main()
