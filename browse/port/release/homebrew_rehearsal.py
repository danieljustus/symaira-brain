#!/usr/bin/env python3
"""Audit an unsigned candidate formula and rehearse it in an owned prefix.

This is local packaging evidence only. It never writes a tap or installs into
Homebrew's managed prefix.
"""
from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

if str(Path(__file__).resolve().parent) not in sys.path:
    sys.path.insert(0, str(Path(__file__).resolve().parent))

import binary_smoke
import launch_candidate
import verify


class HomebrewBlocker(RuntimeError):
    """A required native Homebrew capability is unavailable."""


def render_formula(version: str, archive: Path, digest: str) -> str:
    """Render a temporary single-target Formula using only local candidate bytes."""
    name = archive.name
    binary = "symbrowse.exe" if name.endswith(".zip") else "symbrowse"
    if binary != "symbrowse":
        raise verify.GateError("Homebrew candidate rehearsal only supports native macOS tar.gz packages")
    return f'''# typed: strict
# frozen_string_literal: true

# Temporary unsigned candidate formula; not for tap publication.
class Symbrowse < Formula
  desc "Agent-operable browser automation over Chrome DevTools Protocol"
  homepage "https://github.com/danieljustus/symaira-browse"
  url "{archive.resolve().as_uri()}"
  version "{version}"
  sha256 "{digest}"
  license "Apache-2.0"

  def install
    bin.install "symbrowse"
  end

  test do
    system "#{{bin}}/symbrowse", "version", "--json"
    system "#{{bin}}/symbrowse", "--help"
  end
end
'''


def style_formula(formula: Path) -> None:
    brew = shutil.which("brew")
    if brew is None:
        raise HomebrewBlocker("HOMEBREW_UNAVAILABLE: native macOS candidate runner has no brew executable")
    environment = os.environ.copy()
    environment.update(
        {
            "HOMEBREW_NO_AUTO_UPDATE": "1",
            "HOMEBREW_NO_INSTALL_CLEANUP": "1",
            "HOMEBREW_NO_INSTALLED_DEPENDENTS_CHECK": "1",
        }
    )
    result = subprocess.run(
        [brew, "style", str(formula)],
        capture_output=True,
        text=True,
        timeout=120,
        env=environment,
        check=False,
    )
    if result.returncode:
        detail = (result.stdout + result.stderr).strip()
        raise verify.GateError(f"Homebrew formula style check failed: {detail}")


def _binary_path(prefix: Path, version: str, implementation: str) -> Path:
    return prefix / "Cellar" / "symbrowse" / f"{version}-{implementation}" / "bin" / "symbrowse"


def _switch(prefix: Path, binary: Path) -> Path:
    bindir = prefix / "bin"
    bindir.mkdir(parents=True, exist_ok=True)
    current = bindir / "symbrowse"
    replacement = bindir / ".symbrowse-next"
    replacement.symlink_to(binary)
    os.replace(replacement, current)
    return current


def _install_candidate(prefix: Path, dual: Path, version: str, target: str, implementation: str) -> Path:
    selected, archive = launch_candidate.select_archive(dual, version, target, implementation)
    if selected != implementation:
        raise verify.GateError(f"candidate selector returned {selected}, expected {implementation}")
    destination = _binary_path(prefix, version, implementation).parent
    destination.mkdir(parents=True)
    binary = launch_candidate._extract_binary(
        archive,
        destination,
        "symbrowse.exe" if target.startswith("windows-") else "symbrowse",
    )
    return binary


def rehearse(candidate: Path, version: str, target: str) -> dict[str, object]:
    if not target.startswith("darwin-") or sys.platform != "darwin":
        raise HomebrewBlocker("HOMEBREW_NATIVE_DARWIN_REQUIRED: rehearsal requires a native macOS target runner")
    dual = candidate / "dual" if (candidate / "dual").is_dir() else candidate
    expected = verify.archive_name(version, "darwin", target.removeprefix("darwin-"))
    formula_archive = dual / "rust" / expected
    if not formula_archive.is_file():
        raise verify.GateError(f"Rust Homebrew candidate archive is missing: {expected}")
    checksum = verify._read_checksum_manifest(dual / "rust" / "checksums.txt")
    digest = verify._sha256(formula_archive)
    if checksum.get(expected) != digest:
        raise verify.GateError(f"Rust Homebrew candidate archive checksum mismatch: {expected}")

    with tempfile.TemporaryDirectory(prefix="symbrowse-brew-candidate-") as raw:
        owned = Path(raw)
        formula = owned / "Formula" / "symbrowse.rb"
        formula.parent.mkdir()
        formula.write_text(render_formula(version, formula_archive, digest), encoding="utf-8")
        style_formula(formula)

        prefix = owned / "prefix"
        go_binary = _install_candidate(prefix, dual, version, target, "go")
        go_identity = binary_smoke.check_binary(go_binary, version)
        launcher = _switch(prefix, go_binary)
        binary_smoke.check_binary(launcher, version)

        rust_binary = _install_candidate(prefix, dual, version, target, "rust")
        try:
            binary_smoke.check_binary(rust_binary, f"{version}-deliberately-wrong")
        except ValueError as error:
            failure = str(error)
        else:
            raise verify.GateError("wrong-version candidate unexpectedly passed the rollback rehearsal")
        if launcher.resolve(strict=True) != go_binary.resolve(strict=True):
            raise verify.GateError("failed candidate changed the clean-prefix Go launcher")

        rust_identity = binary_smoke.check_binary(rust_binary, version)
        _switch(prefix, rust_binary)
        binary_smoke.check_binary(launcher, version)
        _switch(prefix, go_binary)
        binary_smoke.check_binary(launcher, version)

        return {
            "schema_version": 1,
            "target": target,
            "formula_name": "symbrowse",
            "formula_archive": expected,
            "formula_sha256": digest,
            "brew_formula_style": "passed",
            "prefix": "owned temporary Homebrew Cellar/bin layout",
            "install": {"implementation": "go", "identity": go_identity},
            "failed_upgrade": {"blocked": True, "reason": failure, "previous_launcher": "go"},
            "upgrade": {"implementation": "rust", "identity": rust_identity},
            "forced_rollback": {"implementation": "go", "identity": go_identity},
            "brew_install_or_tap_write": False,
            "signing_or_publication": False,
        }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--target", required=True)
    args = parser.parse_args()
    try:
        print(json.dumps(rehearse(args.candidate, args.version, args.target), sort_keys=True))
        return 0
    except (OSError, ValueError, verify.GateError, HomebrewBlocker) as error:
        parser.exit(1, f"Homebrew candidate rehearsal: {error}\n")


if __name__ == "__main__":
    raise SystemExit(main())
