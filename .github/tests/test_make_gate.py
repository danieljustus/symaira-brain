#!/usr/bin/env python3
"""Go-free policy tests for the Makefile gates and dependency policy (ORACLE-FREEZE-781).

These replace the Makefile/deny.toml assertions of the Go tests in
rust_workflow_test.go. Workflow-YAML assertions stay in the Go tests until the
CI workflows are rewritten by the coordinator.
"""
from __future__ import annotations

import re
import shutil
import subprocess
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
GATE_TARGETS = [
    "rust-check", "rust-guard-check", "rust-fast", "rust-audit", "rust-deny", "parity-smoke", "init-differential",
    "test", "test-race", "lint", "build", "build-rust", "vet", "fmt", "fmt-check",
]
GO_USE = re.compile(
    r"(?:(?<![\w./-])go\s+(?:-C\s+\S+\s+)?(?:run|build|test|vet|env|list|version|mod|install|tool|fmt|clean)\b"
    r"|(?<![\w./-])gofmt\b|run-go-oracle|GOTOOLCHAIN|symbrain-go|symbrowse-go|SYMBRAIN_GO_BINARY|GO_ORACLE|go\.mod)"
)


def make_dry_run(*targets: str) -> str:
    result = subprocess.run(
        ["make", "-n", "-s", *targets], cwd=ROOT, capture_output=True, text=True,
        env={**__import__("os").environ, "CI": "1"},
    )
    if result.returncode != 0:
        raise AssertionError(f"make -n {' '.join(targets)} failed:\n{result.stderr}")
    return result.stdout


def recipe(name: str) -> str:
    lines = (ROOT / "Makefile").read_text().splitlines()
    body: list[str] = []
    inside = False
    for line in lines:
        if line.startswith(name + ":"):
            inside = True
            continue
        if inside and line.startswith("\t"):
            body.append(line)
        elif inside and line.strip():
            break
    if not body:
        raise AssertionError(f"missing executable recipe {name}")
    return "\n".join(body)


@unittest.skipUnless(shutil.which("make"), "make is required for the gate-closure check")
class GoFreeGate(unittest.TestCase):
    def test_no_gate_target_invokes_go_even_transitively(self) -> None:
        for target in GATE_TARGETS:
            with self.subTest(target=target):
                expanded = make_dry_run(target)
                hits = [line for line in expanded.splitlines() if GO_USE.search(line)]
                self.assertEqual(hits, [], f"{target} reaches Go:\n" + "\n".join(hits))

    def test_gate_never_reaches_recording_helpers(self) -> None:
        expanded = make_dry_run("rust-check")
        for forbidden in ("record.py", "record-oracle", "legacy-go", "record-go-fallback"):
            self.assertNotIn(forbidden, expanded)

    def test_rust_check_replays_fixtures_and_runs_every_rust_step(self) -> None:
        expanded = make_dry_run("rust-check")
        for needed in (
            "scripts/oracle-freeze/freeze.py verify",
            "unittest discover -s scripts/oracle-freeze",
            "unittest discover -s .github/tests",
            "cargo fmt --all --check",
            "cargo check --workspace --all-targets --all-features --locked",
            "cargo clippy --workspace --all-targets --all-features --locked -- -D warnings",
            "cargo test --workspace --all-features --locked",
            "cargo test --workspace --doc --all-features --locked",
            "cargo audit",
            "cargo deny check",
            "cargo audit --file fuzz/Cargo.lock --deny warnings",
            "cargo deny --manifest-path fuzz/Cargo.toml --locked --all-features check",
            "production_usage_discovery_is_lazy_and_per_call",
        ):
            self.assertIn(needed, expanded)

    def test_guard_check_uses_locked_read_only_checks(self) -> None:
        parity = recipe("rust-guard-check")
        self.assertIn("cargo fmt --all --check", parity)
        for command in ("check", "clippy", "test"):
            self.assertIn(f"cargo {command} --workspace --all-targets --all-features --locked", parity)
        self.assertIn("-- -D warnings", parity)
        self.assertIn("cargo test --workspace --doc --all-features --locked", parity)
        for unsafe in ("SYMBRAIN_CAPABILITY_UPDATE=1", "cargo update", "|| true", "set +e"):
            self.assertNotIn(unsafe, parity)
        self.assertIn("rust-guard-check: guard-oracle-check", (ROOT / "Makefile").read_text())

    def test_audit_and_deny_targets_are_locked(self) -> None:
        self.assertIn("cargo audit --file Cargo.lock --deny warnings", recipe("rust-audit"))
        self.assertIn("cargo deny --locked --all-features check", recipe("rust-deny"))

    def test_default_goal_is_preserved(self) -> None:
        first = re.search(r"(?m)^([A-Za-z_][A-Za-z0-9_.-]*):([^=]|$)", (ROOT / "Makefile").read_text())
        self.assertIsNotNone(first)
        self.assertEqual(first.group(1), "coverage")

    def test_every_per_oracle_alias_names_a_registered_group(self) -> None:
        import json
        registry = json.loads((ROOT / "scripts/oracle-freeze/registry.json").read_text())
        ids = {g["id"] for g in registry["groups"]}
        used = set(re.findall(r"--group ([A-Za-z0-9_-]+)", (ROOT / "Makefile").read_text()))
        self.assertEqual(sorted(used - ids), [])


class DependencyPolicy(unittest.TestCase):
    def test_rejects_unknown_sources_and_ignored_advisories(self) -> None:
        policy = tomllib.loads((ROOT / "deny.toml").read_text())
        self.assertEqual(policy.get("advisories", {}).get("ignore", []), [], "dependency gate must not silently ignore advisories")
        self.assertTrue(policy.get("licenses", {}).get("allow"), "dependency gate requires an explicit license allowlist")
        self.assertEqual(policy["sources"]["unknown-registry"], "deny")
        self.assertEqual(policy["sources"]["unknown-git"], "deny")


if __name__ == "__main__":
    unittest.main()
