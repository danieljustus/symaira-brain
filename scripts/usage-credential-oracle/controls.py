"""Actual native replays must reject corrupt, missing or incomplete evidence."""
import json
import os
import pathlib
import subprocess
import sys

scratch = pathlib.Path(sys.argv[1])
rust = pathlib.Path(sys.argv[2])
script = pathlib.Path(__file__).with_name("cli.py")
fake = scratch / "bin" / ("symvault.exe" if os.name == "nt" else "symvault")
controls = []
for control, diagnostic in [("exit", "CLI byte/exit mismatch"), ("missing-case", "missing CLI case")]:
    command = [sys.executable, str(script), str(scratch / "go-usage"), str(rust), str(fake), str(scratch / ("cli-control-" + control + ".json")), "--control", control]
    result = subprocess.run(command, capture_output=True, timeout=120, check=False)
    assert result.returncode != 0 and diagnostic.encode() in result.stderr, (control, result.stderr.decode())
    controls.append({"id": "cli-" + control, "exit": result.returncode, "intended_diagnostic": diagnostic, "stderr": result.stderr.decode()})
fixture = scratch / "go.json"
original = fixture.read_bytes()
for control, diagnostic in [("missing-oracle", "Go evidence"), ("mutated-report", "full report"), ("duplicate-case", "exact source/reference corpus")]:
    try:
        if control == "missing-oracle":
            fixture.unlink()
        else:
            records = json.loads(original)
            if control == "duplicate-case":
                records[0]["id"] = records[1]["id"]
            else:
                records[0]["report"]["providers"][0]["auth_status"]["detail"] = "intentional-768-control"
            fixture.write_text(json.dumps(records))
        result = subprocess.run(["cargo", "test", "--locked", "-p", "symbrain-usage", "--test", "credential_reference_tests", "--", "--ignored", "--nocapture"], capture_output=True, timeout=120, check=False)
        output = result.stdout + result.stderr
        assert result.returncode == 101 and diagnostic.encode() in output and b"1 failed" in output, (control, output.decode())
        controls.append({"id": control, "exit": result.returncode, "intended_diagnostic": diagnostic, "output": output.decode()})
    finally:
        fixture.write_bytes(original)
(scratch / "controls.json").write_text(json.dumps(controls, indent=2) + "\n")
assert len(controls) == 5
