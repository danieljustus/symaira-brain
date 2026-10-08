#!/usr/bin/env python3
"""Actual native-output mutations enforce URI equality and the narrow E011 rule."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["go", "rust", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    mutations = [("normalized-proxy-uri", "proxy-target-port-080", "body['target']=body['target'].replace(':080','')"),
                 ("automatic-fragment-leak", "automatic-fragment", "body['headers']['referer'][0]+='#private-fragment'"),
                 ("strip-explicit-fragment", "explicit-fragment", "body['headers']['referer'][0]=body['headers']['referer'][0].split('#')[0]")]
    observations = []
    with tempfile.TemporaryDirectory(prefix="fetch773-raw-mutants-") as raw:
        for name, case, change in mutations:
            wrapper = Path(raw) / (name + ".py")
            wrapper.write_text("import json,subprocess,sys\n"
                f"p=subprocess.run([{str(args.rust.resolve())!r}],input=sys.stdin.buffer.read(),capture_output=True)\n"
                "sys.stderr.buffer.write(p.stderr)\nif p.returncode: sys.exit(p.returncode)\n"
                "rows=[json.loads(line) for line in p.stdout.splitlines()]\n"
                f"for row in rows:\n if row['id']=={case!r}:\n"
                "  body=json.loads(bytes.fromhex(row['body_hex']))\n  " + change + "\n"
                "  encoded=json.dumps(body,sort_keys=True,separators=(',',':')).encode()\n"
                "  row['body_hex']=encoded.hex(); row['headers']['content-length']=[str(len(encoded))]\n"
                "for row in rows: print(json.dumps(row))\n")
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("fetch_control_raw_proxy.py")),
                "--go", str(args.go), "--rust", str(wrapper), "--output", str(Path(raw) / (name + ".json"))],
                capture_output=True, timeout=300)  # hang guard: one full harness pass takes ~40s on macOS CI VMs
            stderr = result.stderr.decode(errors="replace")
            assert result.returncode and "AssertionError" in stderr and case in stderr, (name, result.returncode, stderr)
            observations.append(dict(name=name, case=case, exit=result.returncode, rejected=True,
                stdout=result.stdout.decode(errors="replace"), stderr=stderr,
                wrapper_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest()))
    args.output.write_text(json.dumps(dict(rejected=3, controls=observations,
        native_sha256=hashlib.sha256(args.rust.read_bytes()).hexdigest(),
        go_sha256=hashlib.sha256(args.go.read_bytes()).hexdigest()), indent=2) + "\n")
    print("Three actual native URI/automatic/explicit Referer mutants rejected")


if __name__ == "__main__":
    main()
