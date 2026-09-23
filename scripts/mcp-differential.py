#!/usr/bin/env python3
"""Compare the Go and Rust MCP executables with isolated embedded memory."""
from __future__ import annotations

import json
import os
import pathlib
import selectors
import signal
import subprocess
import sys
import tempfile


def request(request_id: int | None, method: str, params: dict | None = None) -> dict:
    value = {"jsonrpc": "2.0", "method": method}
    if request_id is not None:
        value["id"] = request_id
    if params is not None:
        value["params"] = params
    return value


REQUESTS = (
    request(1, "initialize"),
    request(None, "notifications/initialized"),
    request(2, "tools/list"),
    request(3, "tools/call", {"name": "memory_get", "arguments": {"id": "mcp-oracle-absent"}}),
    request(4, "tools/call", {"name": "memory_search", "arguments": {"query": "mcp-oracle-never-present"}}),
)
PROFILE = '''[profile]
name = "mcp-oracle"

[servers.vault]
enabled = false

[servers.memory]
enabled = true
mode = "read_only"

[servers.skills]
enabled = false

[servers.usage]
enabled = false

[audit]
enabled = false
'''
DEPRECATED_SERVE = b"symbrain: 'serve' is deprecated; use 'symbrain mcp' instead (serve will be removed in a future release)\n"


def input_bytes(framing: str) -> bytes:
    output = bytearray()
    for value in REQUESTS:
        body = json.dumps(value, separators=(",", ":")).encode()
        if framing == "line":
            output.extend(body + b"\n")
        else:
            output.extend(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
    return bytes(output)


def frames(data: bytes, framing: str) -> list[tuple[bytes, dict]]:
    result = []
    while data:
        if framing == "line":
            end = data.find(b"\n")
            assert end >= 0, f"unterminated JSON-RPC line: {data!r}"
            raw, data = data[:end], data[end + 1 :]
            body = raw
        else:
            end = data.find(b"\r\n\r\n")
            assert end >= 0, f"incomplete Content-Length header: {data!r}"
            header, rest = data[:end], data[end + 4 :]
            lengths = [line.split(b":", 1)[1].strip() for line in header.split(b"\r\n") if line.lower().startswith(b"content-length:")]
            assert len(lengths) == 1, f"expected one Content-Length header: {header!r}"
            size = int(lengths[0])
            assert len(rest) >= size, f"truncated Content-Length body: {header!r}"
            body, data = rest[:size], rest[size:]
            raw = header + b"\r\n\r\n" + body
        result.append((raw, json.loads(body)))
    return result


def environment(root: pathlib.Path) -> dict[str, str]:
    home = root / "home"
    paths = [home, root / "config", root / "data", root / "cache", root / "state", root / "runtime", root / "empty-path"]
    for path in paths:
        path.mkdir(parents=True, exist_ok=True)
    env = {
        "HOME": str(home),
        "USERPROFILE": str(home),
        "XDG_CONFIG_HOME": str(root / "config"),
        "XDG_DATA_HOME": str(root / "data"),
        "XDG_CACHE_HOME": str(root / "cache"),
        "XDG_STATE_HOME": str(root / "state"),
        "XDG_RUNTIME_DIR": str(root / "runtime"),
        "TMPDIR": str(root),
        "PATH": str(root / "empty-path"),
    }
    if os.name == "nt":
        for key in ("SystemRoot", "windir", "ComSpec", "PATHEXT", "SystemDrive"):
            if key in os.environ:
                env[key] = os.environ[key]
    return env


def run(binary: pathlib.Path, root: pathlib.Path, profile: pathlib.Path, framing: str, args: tuple[str, ...] = ("mcp",)) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        [str(binary), *args, "--profile-file", str(profile)],
        input=input_bytes(framing),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=root,
        env=environment(root),
        timeout=15,
        check=False,
    )


def assert_session(go: subprocess.CompletedProcess[bytes], rust: subprocess.CompletedProcess[bytes], framing: str) -> None:
    assert go.returncode == rust.returncode == 0, (go.returncode, rust.returncode, go.stderr, rust.stderr)
    assert go.stderr == rust.stderr == b"", (go.stderr, rust.stderr)
    go_frames, rust_frames = frames(go.stdout, framing), frames(rust.stdout, framing)
    assert [frame[1].get("id") for frame in go_frames] == [1, 2, 3, 4]
    assert [frame[1].get("id") for frame in rust_frames] == [1, 2, 3, 4]
    assert all(frame[1].get("jsonrpc") == "2.0" for frame in go_frames + rust_frames)
    assert go_frames[0][0] == rust_frames[0][0], (go_frames[0], rust_frames[0])
    go_tools = {tool["name"] for tool in go_frames[1][1]["result"]["tools"]}
    rust_tools = {tool["name"] for tool in rust_frames[1][1]["result"]["tools"]}
    assert go_tools == rust_tools, (sorted(go_tools), sorted(rust_tools))
    assert {"bootstrap", "patterns", "memory_get", "memory_search"} <= go_tools
    for index in (2, 3):
        assert go_frames[index][0] == rust_frames[index][0], (go_frames[index], rust_frames[index])
        assert go_frames[index][1]["result"]["isError"] is False
    assert go_frames[2][1]["result"]["content"][0]["text"] == "memory not found: mcp-oracle-absent"
    assert go_frames[3][1]["result"]["content"][0]["text"] == "No relevant memories found."


def assert_sigterm(binary: pathlib.Path, root: pathlib.Path, profile: pathlib.Path) -> tuple[bytes, int, bytes, bytes]:
    env = environment(root)
    child = subprocess.Popen(
        [str(binary), "mcp", "--profile-file", str(profile)],
        cwd=root,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert child.stdin and child.stdout
    child.stdin.write(json.dumps(REQUESTS[0], separators=(",", ":")).encode() + b"\n")
    child.stdin.flush()
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            assert selector.select(timeout=10), "MCP initialize timed out before SIGTERM"
            response = child.stdout.readline()
        parsed = frames(response, "line")
        assert parsed[0][1].get("id") == 1, response
        child.send_signal(signal.SIGTERM)
        stdout, stderr = child.communicate(timeout=5)
    except BaseException:
        if child.poll() is None:
            child.kill()
        child.communicate()
        raise
    assert child.returncode == 0 and not stdout and not stderr, (child.returncode, stdout, stderr)
    return response, child.returncode, stdout, stderr


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: mcp-differential.py GO_BINARY RUST_BINARY", file=sys.stderr)
        return 2
    go_binary, rust_binary = (pathlib.Path(arg).resolve() for arg in sys.argv[1:])
    with tempfile.TemporaryDirectory(prefix="symbrain-mcp-diff-") as temp:
        root = pathlib.Path(temp).resolve()
        profile = root / "profile.toml"
        profile.write_text(PROFILE, encoding="utf-8")
        for framing in ("line", "content-length"):
            go_root, rust_root = root / f"go-{framing}", root / f"rust-{framing}"
            go_root.mkdir()
            rust_root.mkdir()
            go = run(go_binary, go_root, profile, framing)
            rust = run(rust_binary, rust_root, profile, framing)
            assert_session(go, rust, framing)
            legacy = run(go_binary, go_root, profile, framing, ("serve",))
            assert legacy.returncode == go.returncode and legacy.stdout == go.stdout
            assert legacy.stderr == DEPRECATED_SERVE
            print(f"PASS Go/Rust MCP {framing} frames, embedded memory calls, status and stderr")
        if os.name != "nt":
            go_lifecycle = assert_sigterm(go_binary, root / "sigterm-go", profile)
            rust_lifecycle = assert_sigterm(rust_binary, root / "sigterm-rust", profile)
            assert go_lifecycle == rust_lifecycle, (go_lifecycle, rust_lifecycle)
            print("PASS Go/Rust MCP SIGTERM lifecycle with stdin open")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
