#!/usr/bin/env python3
"""Real native output mutations must fail the byte/ordering/auth-acceptance gate."""
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
    mutations = [
        ("drop-password-octet", "http-auth-080-6-False", "body['headers']['proxy-authorization']=['Basic dXNlcjo=']"),
        ("reverse-caller-order", "http-auth-81-6-True", "body['headers']['proxy-authorization'].reverse()"),
        ("proxy-auth-rejected", "enforced-username-080", "row['status']=407"),
    ]
    controls = []
    with tempfile.TemporaryDirectory(prefix="fetch773-auth-mutants-") as temporary:
        root = Path(temporary)
        for name, case, change in mutations:
            wrapper = root / (name + ".py")
            wrapper.write_text("import json,subprocess,sys\n"
                f"p=subprocess.run([{str(args.rust.resolve())!r}],input=sys.stdin.buffer.read(),capture_output=True)\n"
                "sys.stderr.buffer.write(p.stderr)\nif p.returncode: sys.exit(p.returncode)\n"
                "rows=[json.loads(line) for line in p.stdout.splitlines()]\n"
                f"for row in rows:\n if row['id']=={case!r}:\n"
                "  body=json.loads(bytes.fromhex(row['body_hex']))\n  " + change + "\n"
                "  encoded=json.dumps(body,sort_keys=True,separators=(',',':')).encode()\n"
                "  row['body_hex']=encoded.hex(); row['headers']['content-length']=[str(len(encoded))]\n"
                "for row in rows: print(json.dumps(row))\n")
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("fetch_control_proxy_auth.py")),
                "--go", str(args.go), "--rust", str(wrapper), "--output", str(root / (name + ".json"))],
                capture_output=True, timeout=45)
            error = result.stderr.decode(errors="replace")
            assert result.returncode and "AssertionError" in error and case in error, (name, result.returncode, error)
            controls.append(dict(name=name, case=case, rejected=True, exit=result.returncode,
                                 stdout=result.stdout.decode(errors="replace"), stderr=error,
                                 wrapper_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest()))
    args.output.write_text(json.dumps(dict(rejected=len(controls), controls=controls,
        binaries_sha256={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [("go", args.go), ("rust", args.rust)]}), indent=2) + "\n")
    print("Three actual native proxy-byte/order/acceptance mutants rejected")


if __name__ == "__main__":
    main()
