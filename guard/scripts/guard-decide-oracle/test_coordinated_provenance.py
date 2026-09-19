#!/usr/bin/env python3
"""Run the real native gate against a coordinated disposable mutation."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

SCRIPTS = Path(__file__).resolve().parents[3] / "scripts"
sys.path.insert(0, str(SCRIPTS))
from external_env import ensure_external_environment
from trust_anchor import file_sha256, load_anchor

ensure_external_environment(__file__)


HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
FIXTURE_RELATIVE = Path("rust/symbrain-guard-core/tests/fixtures/external_repair_oracle.json")
GENERATOR_RELATIVE = Path("guard/scripts/guard-decide-oracle/repair_parity.py")
VALIDATOR_RELATIVE = Path("guard/scripts/guard-decide-oracle/native_repair.py")
ANCHOR_RELATIVE = Path(
    "rust/symbrain-guard-core/tests/fixtures/external_repair_trust_anchor.rs"
)
MUTATED_COMMIT = "6f13a19f89828b829f7b5f4d7a26c674148db68f"


def replace_once(path: Path, old: str, new: str) -> None:
    text = path.read_text(encoding="utf-8")
    if text.count(old) != 1:
        raise AssertionError(f"expected one mutation anchor in {path}: {old!r}")
    path.write_text(text.replace(old, new, 1), encoding="utf-8")


def mutate_candidate(candidate: Path, anchor) -> None:
    generator = candidate / GENERATOR_RELATIVE
    replace_once(
        generator,
        '        "oracle_commit": oracle_commit,',
        f'        "oracle_commit": "{MUTATED_COMMIT}",',
    )

    fixture = candidate / FIXTURE_RELATIVE
    document = json.loads(fixture.read_bytes())
    document["oracle_commit"] = MUTATED_COMMIT
    document["cases"][0]["response"]["reason"] = "coordinated fixture mutation"
    fixture.write_bytes((json.dumps(document, indent=2) + "\n").encode())

    # Deliberately remove the mutable Python validator's first trust check. The
    # Rust acceptance test must still reject the copied candidate independently.
    validator = candidate / VALIDATOR_RELATIVE
    replace_once(
        validator,
        "    verify_trust_anchor()\n",
        "    # disposable mutation bypasses the Python trust check\n",
    )

    if (candidate / ANCHOR_RELATIVE).read_bytes() != (ROOT / ANCHOR_RELATIVE).read_bytes():
        raise AssertionError("coordinated mutation changed the independent Rust anchor")
    if MUTATED_COMMIT not in generator.read_text(encoding="utf-8"):
        raise AssertionError("generator source-pin mutation did not land")
    if json.loads(fixture.read_bytes())["oracle_commit"] != MUTATED_COMMIT:
        raise AssertionError("fixture source-pin mutation did not land")
    if file_sha256(fixture) == anchor.fixture_sha256:
        raise AssertionError("fixture mutation did not change its bytes")
    if file_sha256(generator, normalize_crlf=True) == anchor.generator_sha256:
        raise AssertionError("generator mutation did not change its bytes")
    if file_sha256(validator, normalize_crlf=True) == anchor.validator_sha256:
        raise AssertionError("validator mutation did not change its bytes")


def main() -> None:
    anchor = load_anchor()
    source_ignore = shutil.ignore_patterns("target", "__pycache__")
    with tempfile.TemporaryDirectory(prefix="guard-coordinated-") as directory:
        candidate = Path(directory) / "candidate"
        shutil.copytree(ROOT, candidate, ignore=source_ignore)
        if not (candidate / ".git").exists():
            raise AssertionError("copied candidate lost its Git worktree metadata")
        mutate_candidate(candidate, anchor)

        output = Path(directory) / "native-output"
        env = dict(os.environ)
        env["GUARD_REPAIR_OUTPUT"] = str(output)
        env["GUARD_REPAIR_SKIP_COORDINATED"] = "1"
        env["PYTHONDONTWRITEBYTECODE"] = "1"
        result = subprocess.run(
            [sys.executable, str(candidate / VALIDATOR_RELATIVE)],
            cwd=candidate,
            env=env,
            capture_output=True,
            timeout=300,
        )
        combined = (result.stdout + result.stderr).decode("utf-8", errors="replace")
        if result.returncode == 0:
            raise AssertionError("coordinated generator/fixture/validator mutation was accepted")
        if "rust-trust-anchor" not in combined:
            raise AssertionError(
                "copied native gate did not reach the independent Rust trust-anchor gate\n"
                + combined[-6000:]
            )
        if "replay_f01_f05_production_go_observations" not in combined:
            raise AssertionError(
                "copied native gate failed without exercising the Rust fixture replay\n"
                + combined[-6000:]
            )
    print("PASS coordinated source-pin/fixture/generator/validator mutation rejected")


if __name__ == "__main__":
    main()
