#!/usr/bin/env python3
"""Strict, bounded MCP stdio smoke validator for the native Browse binary.

Only sends `initialize` and `tools/list` — never `tools/call` — so this never
drives a real browser (no Chrome/network I/O), matching the CI-safety
rationale documented in `browse/SOURCE_PROVENANCE.md`: Browse's own test
suite touches live Chrome/network and is not safe to run unattended in CI.

Modeled on `operate-mcp-smoke.py`'s bounded-timeout approach, simplified: the
`mcp` subcommand here does not autostart a browser daemon for `initialize`/
`tools/list` (see `internal/mcp/server.go`'s `verifyRunningDaemon`, which
only *checks* for an already-running daemon over its Unix socket without
starting one), so a plain bounded `subprocess.run` is sufficient — no need
for operate-mcp-smoke.py's process-group/selector hardening, which defends
against a script that can hang or fork on a live-effect tool surface.
"""

from __future__ import annotations

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

# Matches the protocolVersion symbrowse's own test suite exercises
# (internal/mcp/server_test.go, TestZeroStdoutPollution).
PROTOCOL_VERSION = "2024-11-05"


class SmokeError(AssertionError):
    pass


def run_bounded(argv: list[str], payload: bytes, timeout: float) -> list[dict]:
    """Run a stdio child, write payload, close stdin, and wait with a hard deadline."""
    if timeout <= 0:
        raise ValueError("timeout must be positive")
    isolated_home = tempfile.mkdtemp(prefix="pb-browse-smoke-home-")
    isolated_path = Path(isolated_home) / "bin"
    isolated_path.mkdir()
    isolated_env = {
        "HOME": isolated_home,
        "PATH": str(isolated_path),
        "XDG_CONFIG_HOME": str(Path(isolated_home) / ".config"),
        "XDG_DATA_HOME": str(Path(isolated_home) / ".local" / "share"),
        "XDG_CACHE_HOME": str(Path(isolated_home) / ".cache"),
    }
    try:
        completed = subprocess.run(
            argv,
            input=payload,
            capture_output=True,
            timeout=timeout,
            env=isolated_env,
        )
    except subprocess.TimeoutExpired as exc:
        raise SmokeError(f"MCP subprocess exceeded {timeout:.1f}s timeout") from exc
    except OSError as exc:
        raise SmokeError(f"MCP subprocess failed to start: {exc}") from exc
    finally:
        shutil.rmtree(isolated_home, ignore_errors=True)
    if completed.returncode != 0:
        raise SmokeError(
            f"MCP subprocess exited {completed.returncode}; stderr: "
            f"{completed.stderr.decode(errors='replace')}"
        )
    if completed.stderr:
        raise SmokeError(
            f"MCP subprocess wrote stderr (zero-stdout-pollution violation): "
            f"{completed.stderr.decode(errors='replace')}"
        )
    responses = []
    for line in completed.stdout.splitlines():
        if not line.strip():
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError as exc:
            raise SmokeError(f"malformed JSON-RPC response: {line!r}") from exc
        if not isinstance(value, dict):
            raise SmokeError("JSON-RPC response is not an object")
        responses.append(value)
    return responses


def validate(binary: str, timeout: float) -> None:
    # Deliberately only initialize + tools/list. No tools/call: the tool
    # catalog includes live-effect tools (navigate, click, type, ...) and
    # this validator must never invoke them.
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "browse-ci", "version": "1"},
        }},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    responses = run_bounded(
        [binary, "mcp", "--tools", "core"],
        b"".join(json.dumps(request, separators=(",", ":")).encode() + b"\n" for request in requests),
        timeout,
    )
    if len(responses) != 2:
        raise SmokeError(f"expected 2 responses, got {len(responses)}")
    initialize, tools_response = responses
    for response, request_id in zip(responses, (1, 2)):
        if response.get("jsonrpc") != "2.0" or response.get("id") != request_id:
            raise SmokeError(f"invalid JSON-RPC envelope: {response!r}")

    result = initialize.get("result")
    if not isinstance(result, dict):
        raise SmokeError(f"initialize result is not an object: {initialize!r}")
    if result.get("protocolVersion") != PROTOCOL_VERSION:
        raise SmokeError(f"unexpected protocolVersion: {result.get('protocolVersion')!r}")
    server_info = result.get("serverInfo")
    if not isinstance(server_info, dict) or server_info.get("name") != "symbrowse":
        raise SmokeError(f"initialize serverInfo.name is missing or wrong: {server_info!r}")
    if not isinstance(server_info.get("version"), str) or not server_info["version"]:
        raise SmokeError("initialize serverInfo.version is missing or malformed")

    tools_result = tools_response.get("result")
    tools = tools_result.get("tools") if isinstance(tools_result, dict) else None
    if not isinstance(tools, list) or not tools:
        raise SmokeError(f"tools/list result.tools is missing or empty: {tools_response!r}")
    names = []
    for tool in tools:
        if not isinstance(tool, dict) or not isinstance(tool.get("name"), str):
            raise SmokeError(f"malformed tool descriptor: {tool!r}")
        schema = tool.get("inputSchema")
        if not isinstance(schema, dict) or schema.get("type") != "object" or not isinstance(schema.get("properties"), dict):
            raise SmokeError(f"malformed inputSchema for {tool['name']!r}: {schema!r}")
        names.append(tool["name"])
    if len(names) != len(set(names)):
        raise SmokeError(f"duplicate tool names in catalog: {names!r}")
    print(f"MCP initialize OK; tools/list OK ({len(names)} tools): " + ", ".join(sorted(names)))


class SmokeValidatorTests(unittest.TestCase):
    def test_rejects_malformed_response(self) -> None:
        with self.assertRaises(SmokeError):
            run_bounded([sys.executable, "-c", "print('{not-json}')"], b"x\n", 5.0)

    def test_rejects_nonzero_exit(self) -> None:
        with self.assertRaises(SmokeError):
            run_bounded([sys.executable, "-c", "import sys; sys.exit(1)"], b"", 5.0)

    def test_rejects_stderr_output(self) -> None:
        with self.assertRaises(SmokeError):
            run_bounded(
                [sys.executable, "-c", "import sys; print('noise', file=sys.stderr)"],
                b"",
                5.0,
            )

    def test_rejects_timeout(self) -> None:
        with self.assertRaises(SmokeError):
            run_bounded([sys.executable, "-c", "import time; time.sleep(5)"], b"", 0.2)

    def test_accepts_well_formed_lines(self) -> None:
        script = (
            "import sys\n"
            "print('{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}')\n"
            "print('{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}')\n"
        )
        responses = run_bounded([sys.executable, "-c", script], b"", 5.0)
        self.assertEqual(len(responses), 2)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", nargs="?", help="path to the symbrowse binary")
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--self-test", action="store_true", help="run this validator's own unit tests")
    args = parser.parse_args()

    if args.self_test:
        suite = unittest.TestLoader().loadTestsFromTestCase(SmokeValidatorTests)
        result = unittest.TextTestRunner(verbosity=2).run(suite)
        return 0 if result.wasSuccessful() else 1

    if not args.binary:
        parser.error("binary is required unless --self-test is given")

    try:
        validate(args.binary, args.timeout)
    except SmokeError as exc:
        print(f"browse-mcp-smoke: FAIL: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
