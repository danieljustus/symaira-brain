#!/usr/bin/env python3
"""Strict, bounded MCP stdio smoke validator for the native Operate binary.

Only sends `initialize` and `tools/list` — never `tools/call` — so this never
performs live desktop automation (screen capture, UI query, input). See
`operate/SOURCE_PROVENANCE.md` for why that boundary matters here.
"""

from __future__ import annotations

import argparse
import json
import os
import selectors
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from typing import BinaryIO, cast

PROTOCOL_VERSION = "2025-06-18"
EXPECTED_TOOLS = [
    "list_apps",
    "list_windows",
    "list_displays",
    "snapshot",
    "query_ui",
    "query_ui_ocr",
    "find_ui",
    "click",
    "type_text",
    "press_keys",
    "scroll",
    "drag",
    "launch_app",
    "focus_window",
    "menu_action",
    "wait_for",
    "permissions_status",
    "get_policy",
    "set_policy",
    "version",
]
OUTPUT_LIMIT = 1 << 20
CLEANUP_TIMEOUT = 1.0
READ_CHUNK = 64 * 1024


class SmokeError(AssertionError):
    pass


def _kill_group(process: subprocess.Popen[bytes]) -> None:
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    except PermissionError:
        # Some native launchers change their process-group ownership. The
        # direct child is still ours, so kill it without touching other groups.
        try:
            process.kill()
        except ProcessLookupError:
            pass


def _close_selector(selector: selectors.BaseSelector) -> None:
    for key in list(selector.get_map().values()):
        pipe = cast(BinaryIO, key.fileobj)
        try:
            selector.unregister(pipe)
        except (KeyError, ValueError):
            pass
        try:
            pipe.close()
        except OSError:
            pass
    selector.close()


def _bounded_cleanup(
    process: subprocess.Popen[bytes],
    selector: selectors.BaseSelector,
    deadline: float,
    buffers: dict[str, bytearray],
) -> None:
    """Drain only until deadline, then close inherited pipes and reap child."""
    while time.monotonic() < deadline:
        remaining = deadline - time.monotonic()
        if process.poll() is not None and not selector.get_map():
            break
        for key, _ in selector.select(max(0.0, min(remaining, 0.05))):
            pipe = cast(BinaryIO, key.fileobj)
            stream = key.data
            try:
                chunk = pipe.read(READ_CHUNK)
            except (BlockingIOError, OSError):
                continue
            if not chunk:
                try:
                    selector.unregister(pipe)
                except (KeyError, ValueError):
                    pass
                pipe.close()
                continue
            buffers[stream].extend(chunk[: max(0, OUTPUT_LIMIT - len(buffers[stream]))])
    _close_selector(selector)
    # Popen.wait() can block on a direct child, so reap with non-blocking polls
    # after the pipe deadline instead of joining a potentially leaked reader.
    while process.poll() is None and time.monotonic() < deadline:
        time.sleep(0.001)
    if process.poll() is None:
        try:
            process.kill()
        except ProcessLookupError:
            pass
        while process.poll() is None and time.monotonic() < deadline + 0.05:
            time.sleep(0.001)


def run_bounded(
    argv: list[str],
    payload: bytes,
    timeout: float,
    *,
    stop_after_responses: int | None = None,
) -> list[dict]:
    """Run a stdio child with bounded nonblocking capture and hard deadlines."""
    if timeout <= 0:
        raise ValueError("timeout must be positive")
    process = subprocess.Popen(
        argv,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    assert process.stdin is not None
    assert process.stdout is not None
    assert process.stderr is not None
    selector = selectors.DefaultSelector()
    buffers = {"stdout": bytearray(), "stderr": bytearray()}
    for stream, pipe in (("stdout", process.stdout), ("stderr", process.stderr)):
        os.set_blocking(pipe.fileno(), False)
        selector.register(pipe, selectors.EVENT_READ, stream)
    try:
        process.stdin.write(payload)
        process.stdin.close()
        execution_deadline = time.monotonic() + timeout
        failure: str | None = None
        terminated_for_smoke = False
        while selector.get_map() or process.poll() is None:
            remaining = execution_deadline - time.monotonic()
            if remaining <= 0:
                failure = f"MCP subprocess exceeded {timeout:.1f}s timeout"
                break
            for key, _ in selector.select(remaining):
                pipe = cast(BinaryIO, key.fileobj)
                stream = key.data
                try:
                    chunk = pipe.read(READ_CHUNK)
                except (BlockingIOError, OSError):
                    continue
                if not chunk:
                    selector.unregister(pipe)
                    pipe.close()
                    continue
                buffers[stream].extend(chunk)
                if (
                    stream == "stdout"
                    and stop_after_responses is not None
                    and buffers["stdout"].count(b"\n") >= stop_after_responses
                    and process.poll() is None
                ):
                    # MCP serve is a long-lived worker and does not exit on
                    # stdin EOF. Once the bounded catalog exchange is
                    # complete, stop only this owned process group.
                    _kill_group(process)
                    terminated_for_smoke = True
                if len(buffers[stream]) > OUTPUT_LIMIT:
                    failure = f"MCP subprocess {stream} exceeded {OUTPUT_LIMIT} byte output limit"
                    break
            if failure:
                break
        if failure:
            _kill_group(process)
            _bounded_cleanup(process, selector, time.monotonic() + CLEANUP_TIMEOUT, buffers)
            raise SmokeError(failure)
        returncode = process.wait()
        _close_selector(selector)
    except (BrokenPipeError, OSError) as exc:
        _kill_group(process)
        _bounded_cleanup(process, selector, time.monotonic() + CLEANUP_TIMEOUT, buffers)
        raise SmokeError(f"MCP subprocess I/O failed: {exc}") from exc
    finally:
        if selector.get_map():
            _close_selector(selector)
    if returncode != 0 and not (terminated_for_smoke and returncode == -signal.SIGKILL):
        raise SmokeError(f"MCP subprocess exited {returncode}")
    stderr = bytes(buffers["stderr"])
    stdout = bytes(buffers["stdout"])
    if stderr:
        unexpected = [
            line for line in stderr.decode(errors="replace").splitlines()
            if line
            and not line.startswith("⚠️  Update available:")
            and not line.startswith("   Use `symoperate updates skip ")
        ]
        if unexpected:
            raise SmokeError(
                "MCP subprocess wrote unexpected stderr: " + "\n".join(unexpected)
            )
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
    # Deliberately only initialize + tools/list. No tools/call: this binary's
    # catalog includes live-effect tools (click, type_text, launch_app, ...)
    # and this validator must never invoke them.
    requests = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": {},
            "clientInfo": {"name": "operate-ci", "version": "1"},
        }},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}},
    ]
    responses = run_bounded(
        [binary, "serve"],
        b"".join(json.dumps(request, separators=(",", ":")).encode() + b"\n" for request in requests),
        timeout,
        stop_after_responses=2,
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

    def test_rejects_output_overflow(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            tool = Path(directory) / "flood.py"
            tool.write_text("import sys; sys.stdout.write('x' * (2**20 + 1)); sys.stdout.flush()\n")
            started = time.monotonic()
            with self.assertRaisesRegex(SmokeError, "output limit"):
                run_bounded([sys.executable, str(tool)], b"", 5.0)
            self.assertLess(time.monotonic() - started, 2.0)

    def test_timeout_escapes_group_and_cleans_owned_descendant(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            pid_file = Path(directory) / "descendant.pid"
            tool = Path(directory) / "escape.py"
            tool.write_text(
                "import os, pathlib, sys, time\n"
                "pid_file = pathlib.Path(sys.argv[1])\n"
                "if os.fork() == 0:\n"
                "    os.setsid()\n"
                "    pid_file.write_text(str(os.getpid()))\n"
                "    while True: os.write(1, b'x' * 65536)\n"
                "while True: time.sleep(1)\n"
            )
            started = time.monotonic()
            try:
                with self.assertRaisesRegex(SmokeError, "timeout|output limit"):
                    run_bounded([sys.executable, str(tool), str(pid_file)], b"", 0.2)
                self.assertLess(time.monotonic() - started, 2.0)
            finally:
                if pid_file.exists():
                    pid = int(pid_file.read_text())
                    self.assertGreater(pid, 0)
                    self.assertNotEqual(pid, os.getpid())
                    try:
                        os.kill(pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass


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
        print(f"Operate MCP smoke failed: {exc}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
