#!/usr/bin/env python3
"""Strict, bounded MCP stdio smoke validator for the native Scope binary."""

from __future__ import annotations

import argparse
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

PROTOCOL_VERSION = "2025-06-18"
EXPECTED_TOOLS = [
    "scan",
    "ports_list",
    "ports_suggest",
    "mcp_list",
    "conflicts",
    "mcp_health",
    "daemons_list",
]


class SmokeError(AssertionError):
    pass


def run_bounded(argv: list[str], payload: bytes, timeout: float) -> list[dict]:
    """Run a stdio child, killing its whole process group on deadline."""
    process = subprocess.Popen(
        argv,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(payload, timeout=timeout)
    except subprocess.TimeoutExpired as exc:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        # communicate() both drains pipes and reaps the direct child after the
        # group kill; never leave an inherited pipe or zombie behind.
        process.communicate()
        raise SmokeError(f"MCP subprocess exceeded {timeout:.1f}s timeout") from exc
    if process.returncode != 0:
        raise SmokeError(f"MCP subprocess exited {process.returncode}")
    if stderr:
        raise SmokeError(f"MCP subprocess wrote stderr: {stderr.decode(errors='replace')}")
    responses = []
    for line in stdout.splitlines():
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
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "scope-ci", "version": "1"},
        }},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    responses = run_bounded(
        [binary, "serve"],
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
        raise SmokeError("initialize result is not an object")
    if result.get("protocolVersion") != PROTOCOL_VERSION:
        raise SmokeError(f"unexpected protocolVersion: {result.get('protocolVersion')!r}")
    server_info = result.get("serverInfo")
    if not isinstance(server_info, dict) or not isinstance(server_info.get("name"), str) or not server_info["name"]:
        raise SmokeError("initialize serverInfo.name is missing or malformed")
    if not isinstance(server_info.get("version"), str) or not server_info["version"]:
        raise SmokeError("initialize serverInfo.version is missing or malformed")
    capabilities = result.get("capabilities")
    tools_capability = capabilities.get("tools") if isinstance(capabilities, dict) else None
    if not isinstance(tools_capability, dict) or tools_capability.get("listChanged") is not False:
        raise SmokeError(f"initialize capabilities.tools has wrong shape: {capabilities!r}")
    tools_result = tools_response.get("result")
    tools = tools_result.get("tools") if isinstance(tools_result, dict) else None
    if not isinstance(tools, list):
        raise SmokeError("tools/list result.tools is not an array")
    names = []
    for tool in tools:
        if not isinstance(tool, dict) or not isinstance(tool.get("name"), str):
            raise SmokeError(f"malformed tool descriptor: {tool!r}")
        schema = tool.get("inputSchema")
        if not isinstance(schema, dict) or schema.get("type") != "object" or not isinstance(schema.get("properties"), dict):
            raise SmokeError(f"malformed inputSchema for {tool['name']!r}: {schema!r}")
        names.append(tool["name"])
    if names != EXPECTED_TOOLS or len(names) != len(set(names)):
        raise SmokeError(f"tool catalog mismatch: {names!r}")
    print("MCP initialize OK; tools/list OK: " + ", ".join(names))


class SmokeValidatorTests(unittest.TestCase):
    def test_rejects_malformed_response(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            tool = Path(directory) / "malformed.py"
            tool.write_text("import sys; print('{not-json}')\n")
            with self.assertRaises(SmokeError):
                run_bounded([sys.executable, str(tool)], b"x\n", 1.0)

    def test_timeout_kills_group_and_reaps_child(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            marker = Path(directory) / "descendant-alive"
            tool = Path(directory) / "hang.py"
            tool.write_text(
                "import pathlib, subprocess, sys, time\n"
                "marker = pathlib.Path(sys.argv[1])\n"
                "subprocess.Popen([sys.executable, '-c', 'import pathlib,time; time.sleep(3); pathlib.Path(\"' + str(marker) + '\").write_text(\"alive\")'])\n"
                "while True: time.sleep(1)\n"
            )
            with self.assertRaises(SmokeError):
                run_bounded([sys.executable, str(tool), str(marker)], b"", 0.1)
            time.sleep(0.2)
            self.assertFalse(marker.exists())


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", nargs="?")
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return 0 if unittest.main(argv=[sys.argv[0]], exit=False).result.wasSuccessful() else 1
    if not args.binary:
        parser.error("binary is required unless --self-test is used")
    try:
        validate(args.binary, args.timeout)
    except SmokeError as exc:
        print(f"Scope MCP smoke failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
