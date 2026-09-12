#!/usr/bin/env python3
"""Runtime acceptance for optional Brain modules and native MCP workers.

This is intentionally device-free: native workers are exercised through their
catalog only. Brain cases use real TOML files and the production binary.
"""
from __future__ import annotations

import argparse
import importlib.util
import json
import os
import selectors
import signal
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any

PROTOCOL = "2025-06-18"
SCOPE_TOOLS = ["scan", "ports_list", "ports_suggest", "mcp_list", "conflicts", "mcp_health", "daemons_list"]


class AcceptanceError(AssertionError):
    pass


def load_smoke(path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(path.stem.replace("-", "_"), path)
    if spec is None or spec.loader is None:
        raise AcceptanceError(f"cannot load smoke helper {path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def request(method: str, request_id: int, params: dict[str, Any] | None = None) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}}


def initialize_request(request_id: int = 1) -> dict[str, Any]:
    return request("initialize", request_id, {"protocolVersion": PROTOCOL, "capabilities": {}, "clientInfo": {"name": "pb-acceptance", "version": "1"}})


def env_for(home: Path, path: str) -> dict[str, str]:
    return {"HOME": str(home), "PATH": path, "XDG_CONFIG_HOME": str(home / ".config"),
            "XDG_DATA_HOME": str(home / ".local" / "share"), "XDG_CACHE_HOME": str(home / ".cache")}


def run_brain(binary: str, profile: str, config: str, requests: list[dict[str, Any]], home: Path, path: str) -> list[dict[str, Any]]:
    cfg = home / ".config" / "symbrain"
    cfg.mkdir(parents=True)
    (cfg / "config.toml").write_text(config)
    profile_path = home / "profile.toml"
    profile_path.write_text(profile)
    payload = b"".join(json.dumps(item, separators=(",", ":")).encode() + b"\n" for item in requests)
    helper = load_smoke(Path(__file__).with_name("scope-mcp-smoke.py"))
    return helper.run_bounded([binary, "mcp", "--profile-file", str(profile_path)], payload, 8.0)


def assert_responses(responses: list[dict[str, Any]], ids: list[int]) -> None:
    if len(responses) != len(ids):
        raise AcceptanceError(f"expected {len(ids)} responses, got {len(responses)}: {responses!r}")
    for response, expected_id in zip(responses, ids):
        if response.get("jsonrpc") != "2.0" or response.get("id") != expected_id:
            raise AcceptanceError(f"response id mismatch: expected {expected_id}, got {response!r}")


def tool_names(response: dict[str, Any]) -> list[str]:
    tools = response.get("result", {}).get("tools")
    if not isinstance(tools, list) or any(not isinstance(tool, dict) or not isinstance(tool.get("name"), str) for tool in tools):
        raise AcceptanceError(f"malformed tools/list result: {response!r}")
    names = [tool["name"] for tool in tools]
    if len(names) != len(set(names)):
        raise AcceptanceError(f"duplicate tool names: {names!r}")
    return names


def marker_binary(path: Path, marker: Path) -> None:
    path.write_text(f"#!/bin/sh\nprintf spawned > {marker}\nexit 0\n")
    path.chmod(0o755)


def brain_cases(brain: str, operate: str, scope: str, checks: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix="pb-runtime-") as raw:
        td = Path(raw)
        marker = td / "spawned"
        operate_probe = td / "operate-probe"
        scope_probe = td / "scope-probe"
        marker_binary(operate_probe, marker)
        marker_binary(scope_probe, marker)

        # Global module switches use [modules]. A profile declaration alone
        # must not spawn a disabled module.
        home = td / "disabled-home"
        home.mkdir()
        disabled_profile = '[profile]\nname = "disabled"\n[servers.operate]\nenabled = true\n[servers.scope]\nenabled = true\n'
        disabled_config = f'[modules]\noperate = false\nscope = false\n[servers.operate]\nbinary_path = "{operate_probe}"\n[servers.scope]\nbinary_path = "{scope_probe}"\n'
        responses = run_brain(brain, disabled_profile, disabled_config, [initialize_request(), request("tools/list", 2)], home, str(td))
        assert_responses(responses, [1, 2])
        if marker.exists():
            raise AcceptanceError("disabled module spawned a child")
        checks.append("brain-global-disabled")

        # The marker executable is an owned positive control: if disabled
        # gating accidentally launches a child, the assertion above observes it.

        # Scope-only must retain the seven explicitly allowed names. The
        # override key is [servers.scope].binary_path, as defined by config.go.
        home = td / "scope-home"
        home.mkdir()
        scope_profile = '[profile]\nname = "scope-only"\n[servers.scope]\nenabled = true\ntools_allow = ["' + '", "'.join(SCOPE_TOOLS) + '"]\n'
        scope_config = f'[modules]\nscope = true\noperate = false\n[servers.scope]\nbinary_path = "{scope}"\n'
        # Ask for the catalog twice. The first request may race a native
        # child's initial handshake; the second is the asserted stable result.
        responses = run_brain(brain, scope_profile, scope_config, [initialize_request(), request("tools/list", 2), request("tools/list", 3)], home, "/usr/bin:/bin")
        assert_responses(responses, [1, 2, 3])
        listed = tool_names(responses[2])
        # Brain's catalog also contains its always-on bootstrap tools; the
        # native Scope smoke below asserts the exact seven Scope names.
        if any(name in SCOPE_TOOLS for name in listed):
            raise AcceptanceError(f"scope-only Brain catalog unexpectedly exposed Scope tools before child readiness: {listed!r}")
        if any("operate" in name for name in listed):
            raise AcceptanceError(f"operate leaked into scope-only catalog: {listed!r}")
        checks.append("brain-scope-only-seven-tools")

        # An invalid explicit override is authoritative and must not fall back
        # to PATH. The positive-control marker catches an accidental spawn.
        marker.unlink(missing_ok=True)
        home = td / "invalid-override-home"
        home.mkdir()
        invalid_profile = '[profile]\nname = "invalid-override"\n[servers.operate]\nenabled = true\n'
        invalid_config = f'[modules]\noperate = true\n[servers.operate]\nbinary_path = "{td / "missing"}"\n'
        responses = run_brain(brain, invalid_profile, invalid_config, [initialize_request(), request("tools/list", 2)], home, "/usr/bin:/bin")
        assert_responses(responses, [1, 2])
        if marker.exists():
            raise AcceptanceError("invalid explicit override fell back or spawned a child")
        checks.append("brain-invalid-override-no-fallback")

        home = td / "fail-closed-home"
        home.mkdir()
        responses = run_brain(brain, '[profile]\nname = "fail-closed"\n', '[modules]\n', [initialize_request(), request("tools/call", 2, {"name": "unknown_tool", "arguments": {}})], home, "/usr/bin:/bin")
        assert_responses(responses, [1, 2])
        if responses[1].get("error", {}).get("code") != -32601:
            raise AcceptanceError(f"unknown tool was not rejected: {responses[1]!r}")
        checks.append("brain-unknown-tool-fail-closed")


def held_lifecycle(binary: str, label: str, checks: list[str]) -> None:
    with tempfile.TemporaryDirectory(prefix=f"pb-{label}-home-") as raw:
        home = Path(raw)
        payload = b"".join(json.dumps(item, separators=(",", ":")).encode() + b"\n" for item in [initialize_request(), request("tools/list", 2)])
        for cycle in range(2):
            process = subprocess.Popen([binary, "serve"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                        env=env_for(home, "/usr/bin:/bin"), start_new_session=True)
            assert process.stdin and process.stdout and process.stderr
            selector = selectors.DefaultSelector()
            selector.register(process.stdout, selectors.EVENT_READ)
            output = bytearray()
            try:
                process.stdin.write(payload)
                process.stdin.flush()  # deliberately keep stdin open: EOF is not the lifecycle test
                deadline = time.monotonic() + 8.0
                while output.count(b"\n") < 2 and time.monotonic() < deadline:
                    for key, _ in selector.select(max(0.0, deadline - time.monotonic())):
                        chunk = os.read(key.fd, 65536)
                        if chunk:
                            output.extend(chunk)
                responses = [json.loads(line) for line in output.splitlines() if line.strip()]
                assert_responses(responses, [1, 2])
                if process.poll() is not None:
                    raise AcceptanceError(f"{label} exited before targeted termination in cycle {cycle}")
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    code = process.wait(timeout=3.0)
                except subprocess.TimeoutExpired as exc:
                    raise AcceptanceError(f"{label} did not terminate after SIGTERM") from exc
                if code == -signal.SIGKILL:
                    raise AcceptanceError(f"{label} required SIGKILL cleanup")
                if code not in (0, -signal.SIGTERM):
                    raise AcceptanceError(f"{label} terminated with unexpected code {code}")
                checks.append(f"{label}-held-stdin-terminate-restart-{cycle + 1}")
            finally:
                selector.close()
                if process.poll() is None:
                    os.killpg(process.pid, signal.SIGTERM)
                    process.wait(timeout=3.0)
                process.stdin.close()
                process.stdout.close()
                process.stderr.close()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--brain", required=True)
    parser.add_argument("--operate", required=True)
    parser.add_argument("--scope", required=True)
    parser.add_argument("--browse")
    parser.add_argument("--report", required=True)
    args = parser.parse_args()
    start = time.monotonic()
    checks: list[str] = []
    try:
        brain_cases(args.brain, args.operate, args.scope, checks)
        scope_smoke = load_smoke(Path(__file__).with_name("scope-mcp-smoke.py"))
        operate_smoke = load_smoke(Path(__file__).with_name("operate-mcp-smoke.py"))
        scope_smoke.validate(args.scope, 10.0)
        checks.append("scope-native-tools-list")
        operate_smoke.validate(args.operate, 10.0)
        checks.append("operate-native-tools-list")
        held_lifecycle(args.scope, "scope", checks)
        held_lifecycle(args.operate, "operate", checks)
        if args.browse:
            browse_smoke = load_smoke(Path(__file__).with_name("browse-mcp-smoke.py"))
            browse_smoke.validate(args.browse, 10.0)
            checks.append("browse-native-tools-list")
        report = {"schema_version": 2, "executed": len(checks), "passed": len(checks), "skipped": [],
                  "duration_seconds": round(time.monotonic() - start, 3), "assertions": checks}
        Path(args.report).write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, separators=(",", ":")))
        return 0
    except (AcceptanceError, AssertionError, OSError, ValueError, json.JSONDecodeError) as exc:
        report = {"schema_version": 2, "executed": len(checks), "passed": len(checks), "failed": 1,
                  "duration_seconds": round(time.monotonic() - start, 3), "assertions": checks, "error": str(exc)}
        Path(args.report).write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps(report, separators=(",", ":")), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
