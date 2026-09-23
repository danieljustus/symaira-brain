#!/usr/bin/env python3
"""Black-box Go↔Rust parity smoke tests for the migration."""
from __future__ import annotations
import json
import calendar
import gzip
import hashlib
import http.server
import io
import os
import platform
import re
import shutil
import sqlite3
import subprocess
import sys
import tarfile
import tempfile
import threading
import time
import zipfile
from dataclasses import dataclass
from pathlib import Path
from typing import Callable

from external_env import ensure_external_environment
if os.name == "posix":
    import pty
RELEASE_BASE_URL = ""
SAMPLE_CONFIG_TOML = """# Symaira Brain Global Configuration
default_profile = "personal"
[audit]
enabled = true
verbose = false
count = 42
threshold = 3.14
[patterns]
promotion_threshold = 5
[servers.vault]
binary_path = "/opt/bin/symvault"
items = ["a", "b", "c"]
empty_list = []
empty_table = {}
[nested_inline]
sub = { foo = "bar", num = 10 }
"�" = "replacement"
"😀" = "emoji"
"東京" = "cjk"
"""
FLOATS_CONFIG_TOML = """# Floats fixture
pos_zero = 0.0
neg_zero = -0.0
pos_inf = inf
pos_inf_plus = +inf
neg_inf = -inf
nan = nan
pos_nan = +nan
neg_nan = -nan
large = 1e6
large2 = 1e5
small = 1e-4
small2 = 1e-5
large_neg = -1e6
large2_neg = -1e5
small_neg = -1e-4
small2_neg = -1e-5
sci1 = 6.022e23
sci2 = 1.23e-10
pi = 3.14
boundary_below_large = 999999.9
boundary_above_small = 0.000100001
boundary_below_small = 0.000099999
"""
DATETIME_CONFIG_TOML = """# Datetime fixture
odt_z = 1979-05-27T07:32:00Z
odt_z_lower = 1979-05-27t07:32:00z
odt_space_z = 1979-05-27 07:32:00Z
odt_frac_micro_z = 1979-05-27T07:32:00.999999Z
odt_frac_milli_z = 1979-05-27T07:32:00.123Z
odt_frac_nano_z = 1979-05-27T07:32:00.123456789Z
odt_frac_trailing_z = 1979-05-27T07:32:00.120000Z
odt_frac_zero_z = 1979-05-27T07:32:00.000Z
odt_no_sec_z = 1979-05-27T07:32Z
odt_pos_offset = 1979-05-27T07:32:00+02:00
odt_neg_offset = 1979-05-27T00:32:00-07:00
odt_offset_zero = 1979-05-27T00:32:00+00:00
odt_offset_neg_zero = 1979-05-27T00:32:00-00:00
odt_space_offset = 1979-05-27 00:32:00-07:00
odt_frac_micro_offset = 1979-05-27T00:32:00.999999-07:00
odt_frac_half_hour_offset = 1979-05-27T00:32:00.123+05:30
odt_frac_trailing_offset = 1979-05-27T00:32:00.120000-07:00
odt_no_sec_offset = 1979-05-27T07:32-05:00
ldt = 1979-05-27T07:32:00
ldt_space = 1979-05-27 07:32:00
ldt_lower = 1979-05-27t07:32:00
ldt_frac_micro = 1979-05-27T07:32:00.999999
ldt_frac_milli = 1979-05-27T07:32:00.123
ldt_frac_nano = 1979-05-27T07:32:00.123456789
ldt_frac_trailing = 1979-05-27T07:32:00.120000
ldt_frac_zero = 1979-05-27T07:32:00.000
ldt_no_sec = 1979-05-27T07:32
ld = 1979-05-27
ld_today = 2026-09-05
ld_year_one = 0001-01-01
lt = 07:32:00
lt_frac_micro = 07:32:00.999999
lt_frac_milli = 07:32:00.123
lt_frac_nano = 07:32:00.123456789
lt_frac_trailing = 07:32:00.120000
lt_frac_zero = 07:32:00.000
lt_no_sec = 07:32
"""
def _write_text(path: Path, text: str, encoding: str = "utf-8") -> None:
    """Write deterministic UTF-8 fixture bytes without Windows newline conversion."""
    path.write_bytes(text.encode(encoding))


def setup_xdg_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", SAMPLE_CONFIG_TOML)
def setup_codex_stale_auth(_root: Path, env: dict[str, str]) -> None:
    codex_dir = Path(env["HOME"]) / ".codex"
    codex_dir.mkdir(parents=True, exist_ok=True)
    # A logged-out Codex CLI leaves an auth file without an access token; the
    # provider reports "expired" and never reaches the network.
    _write_text(codex_dir / "auth.json", '{"OPENAI_API_KEY":"stale"}\n')
def setup_home_config(_root: Path, env: dict[str, str]) -> None:
    env["XDG_CONFIG_HOME"] = ""
    cfg_dir = Path(env["HOME"]) / ".config" / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", SAMPLE_CONFIG_TOML)
def setup_malformed_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", "invalid = [ unterminated toml\n")
def setup_install_default_profile(_root: Path, env: dict[str, str]) -> None:
    cfg = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "config.toml"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    _write_text(cfg, 'default_profile = "personal"\n')
def setup_install_malformed_global(_root: Path, env: dict[str, str]) -> None:
    cfg = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "config.toml"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    _write_text(cfg, "invalid = [ unterminated toml\n")
def setup_install_malformed(_root: Path, env: dict[str, str]) -> None:
    cfg = Path(env["HOME"]) / ".claude.json"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    cfg.write_bytes(b"{not valid json")
def setup_install_superseded(_root: Path, env: dict[str, str]) -> None:
    cfg = Path(env["HOME"]) / ".cursor" / "mcp.json"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    cfg.write_bytes(
        b'{"mcpServers":{"old-memory":{"command":"/opt/bin/symmemory"},'
        b'"old-skills":{"command":"symskills"},"vault":{"command":"symvault"},'
        b'"other":{"command":"other"}}}'
    )
    cfg.chmod(0o640)
def setup_install_existing(_root: Path, env: dict[str, str]) -> None:
    cfg = Path(env["HOME"]) / ".cursor" / "mcp.json"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    cfg.write_bytes(b'{"mcpServers":{"other":{"command":"other"}}}')
    cfg.chmod(0o640)
def setup_install_foreign(_root: Path, env: dict[str, str]) -> None:
    cfg = Path(env["HOME"]) / ".cursor" / "mcp.json"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    cfg.write_bytes(b'{"mcpServers":{"symbrain":{"command":"not-symbrain","args":["x"]}}}')
    cfg.chmod(0o640)
def setup_install_installed(_root: Path, env: dict[str, str]) -> None:
    cfg = Path(env["HOME"]) / ".cursor" / "mcp.json"
    cfg.parent.mkdir(parents=True, exist_ok=True)
    cfg.write_bytes(
        b'{"mcpServers":{"other":{"command":"other"},'
        b'"symbrain":{"command":"symbrain","args":["mcp","--profile","personal"]}}}'
    )
    cfg.chmod(0o640)
def setup_conflict_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", 'default_profile = "personal"\n')
def setup_existing_parent_mode(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    cfg_dir.chmod(0o755)
    _write_text(cfg_dir / "config.toml", 'existing = true\n')
def setup_symlink_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "target.toml", 'target = true\n')
    (cfg_dir / "config.toml").symlink_to("target.toml")
def setup_readonly_config_dir(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", 'existing = true\n')
    cfg_dir.chmod(0o500)
def setup_unreadable_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    cfg_file = cfg_dir / "config.toml"
    _write_text(cfg_file, SAMPLE_CONFIG_TOML)
    cfg_file.chmod(0o000)
def setup_floats_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", FLOATS_CONFIG_TOML)
def setup_datetime_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", DATETIME_CONFIG_TOML)

INLINE_CONFIG_TOML = """# Inline tables fixture
[server]
inline = { port = 8080, active = true }
[nested]
tree = { mid = { leaf = "initial", count = 10 } }
"""
def setup_inline_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", INLINE_CONFIG_TOML)

COMPLEX_CONFIG_TOML = """# Complex config fixture
items = ["apple", "banana"]
"quoted key" = "initial_val"
[[services]]
name = "auth"
port = 8000
[[services]]
name = "api"
port = 9000
"""
def setup_complex_config(_root: Path, env: dict[str, str]) -> None:
    cfg_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    cfg_dir.mkdir(parents=True, exist_ok=True)
    _write_text(cfg_dir / "config.toml", COMPLEX_CONFIG_TOML)
def setup_profile_fixture(_root: Path, env: dict[str, str]) -> None:
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    _write_text(profiles / "good.toml",
        '[profile]\nname = "good"\ndescription = "Good profile"\n'
        '[servers.vault]\nenabled = true\nmode = "request_only"\n'
    )
    _write_text(profiles / "broken.toml", '[profile]\nname = "wrong-name"\n')
def setup_profile_existing(_root: Path, env: dict[str, str]) -> None:
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    _write_text(profiles / "existing.toml", '[profile]\nname = "existing"\n')
def setup_profile_remove_existing(_root: Path, env: dict[str, str]) -> None:
    setup_profile_existing(_root, env)
def setup_profile_remove_bound_global(_root: Path, env: dict[str, str]) -> None:
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    _write_text(profiles / "bound.toml", '[profile]\nname = "bound"\n')
    config = Path(env["HOME"]) / ".claude.json"
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_bytes(
        b'{"mcpServers":{"symbrain":{"command":"symbrain",'
        b'"args":["mcp","--profile","bound"]}}}'
    )
def setup_profile_remove_bound_project(_root: Path, env: dict[str, str]) -> None:
    setup_profile_remove_existing(_root, env)
    project = Path(env["PROJECT"])
    project.joinpath(".mcp.json").write_bytes(
        b'{"mcpServers":{"symbrain":{"command":"symbrain",'
        b'"args":["mcp","--profile","existing"]}}}'
    )
def setup_profile_remove_bound_project_symlink(root: Path, env: dict[str, str]) -> None:
    setup_profile_remove_existing(_root=root, env=env)
    project = Path(env["PROJECT"])
    real_project = project.parent / "real-project"
    real_project.mkdir(parents=True, exist_ok=True)
    real_project.joinpath(".mcp.json").write_bytes(
        b'{"mcpServers":{"symbrain":{"command":"symbrain",'
        b'"args":["mcp","--profile","existing"]}}}'
    )
    project.joinpath("project-link").symlink_to("../real-project")
def setup_profile_remove_symlink_profiles_root(_root: Path, env: dict[str, str]) -> None:
    profiles_parent = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    profiles_parent.mkdir(parents=True, exist_ok=True)
    outside = Path(env["XDG_CONFIG_HOME"]) / "outside-profiles"
    outside.mkdir(parents=True, exist_ok=True)
    _write_text(outside.joinpath("existing.toml"), "outside\n")
    profiles = profiles_parent / "profiles"
    if profiles.exists():
        profiles.rmdir()
    profiles.symlink_to("../outside-profiles")
def setup_profile_remove_malformed_config(_root: Path, env: dict[str, str]) -> None:
    setup_profile_remove_bound_global(_root, env)
    (Path(env["HOME"]) / ".claude.json").write_bytes(b"{not valid json")
def setup_profile_remove_malformed_profile(_root: Path, env: dict[str, str]) -> None:
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    (profiles / "existing.toml").write_bytes(b"[profile\\n")
def setup_profile_remove_symlink(root: Path, env: dict[str, str]) -> None:
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    target = root / "profile-target.toml"
    _write_text(target, '[profile]\nname = "target"\n')
    (profiles / "existing.toml").symlink_to(target)
def setup_profile_remove_fifo(_root: Path, env: dict[str, str]) -> None:
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    os.mkfifo(profiles / "existing.toml")
def setup_profile_remove_directory(_root: Path, env: dict[str, str]) -> None:
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    (profiles / "existing.toml").mkdir()
def setup_audit_empty(_root: Path, env: dict[str, str]) -> None:
    (Path(env["XDG_DATA_HOME"]) / "symbrain" / "audit").mkdir(parents=True, exist_ok=True)
def setup_doctor_empty(root: Path, env: dict[str, str]) -> None:
    empty_path = root / "empty-path"
    empty_path.mkdir()
    env["PATH"] = str(empty_path)
def setup_doctor_failed_version(root: Path, env: dict[str, str]) -> None:
    binary_dir = root / "doctor-path"
    binary_dir.mkdir()
    _write_fake_vault(
        binary_dir / "symvault",
        "if [ \"$1\" = version ]; then printf '{\"version\":\"9.9.9\"}'; exit 42; fi\nprintf 'not found\\n' >&2",
        exit_code=1,
    )
    env["PATH"] = str(binary_dir)
def setup_audit_fixture(_root: Path, env: dict[str, str]) -> None:
    audit_dir = Path(env["XDG_DATA_HOME"]) / "symbrain" / "audit"
    audit_dir.mkdir(parents=True, exist_ok=True)
    entries = [
        {
            "timestamp": "2026-01-01T00:00:00Z",
            "profile": "ignored",
            "server": "vault",
            "tool": "health",
            "duration_ms": 10,
            "status": "ok",
            "retryable": False,
        },
        {
            "timestamp": "2026-01-01T00:01:00Z",
            "profile": "ignored",
            "server": "memory",
            "tool": "memory_search",
            "duration_ms": 20,
            "status": "ok",
            "retryable": False,
            "arg_keys": "query,limit",
        },
        {
            "timestamp": "2026-01-01T00:02:00Z",
            "profile": "ignored",
            "server": "memory",
            "tool": "memory_set",
            "duration_ms": 30,
            "status": "error",
            "category": "timeout",
            "retryable": True,
        },
    ]
    lines = [json.dumps(entry, separators=(",", ":")) for entry in entries]
    _write_text(audit_dir / "alpha.jsonl", "\n".join(lines) + "\n")
def _write_fake_vault(path: Path, body: str, exit_code: int = 0) -> None:
    _write_text(path, f"#!/bin/sh\n{body}\nexit {exit_code}\n")
    path.chmod(0o755)
def setup_vault_path_fixture(root: Path, env: dict[str, str]) -> None:
    vault_dir = root / "vault-path"
    vault_dir.mkdir()
    _write_fake_vault(
        vault_dir / "symvault",
        "IFS= read -r input\\n"
        "printf 'stdout:%s:%s:%s:%s\\n' \"$1\" \"$2\" \"$3\" \"$input\"\\n"
        "printf 'stderr:vault-child\\n' >&2",
    )
    env["PATH"] = str(vault_dir)
def setup_vault_exit_fixture(root: Path, env: dict[str, str]) -> None:
    vault_dir = root / "vault-path"
    vault_dir.mkdir()
    _write_fake_vault(vault_dir / "symvault", "", exit_code=42)
    env["PATH"] = str(vault_dir)
def setup_vault_managed_precedence(root: Path, env: dict[str, str]) -> None:
    path_dir = root / "vault-path"
    path_dir.mkdir()
    _write_fake_vault(path_dir / "symvault", "printf 'path-child\\n'")
    managed_dir = Path(env["HOME"]) / ".symaira" / "bin"
    managed_dir.mkdir(parents=True)
    _write_fake_vault(managed_dir / "symvault", "printf 'managed-child\\n'")
    env["PATH"] = str(path_dir)
def setup_vault_config_override(root: Path, env: dict[str, str]) -> None:
    path_dir = root / "vault-path"
    path_dir.mkdir()
    _write_fake_vault(path_dir / "symvault", "printf 'path-child\\n'")
    managed_dir = Path(env["HOME"]) / ".symaira" / "bin"
    managed_dir.mkdir(parents=True)
    _write_fake_vault(managed_dir / "symvault", "printf 'managed-child\\n'")
    configured_dir = root / "configured"
    configured_dir.mkdir()
    configured = configured_dir / "symvault"
    _write_fake_vault(configured, "printf 'configured-child\\n'")
    config_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    config_dir.mkdir(parents=True)
    _write_text(config_dir / "config.toml",
        f"[servers.vault]\nbinary_path = {json.dumps(str(configured))}\n"
    )
    env["PATH"] = str(path_dir)
def setup_vault_signal_fixture(root: Path, env: dict[str, str]) -> None:
    vault_dir = root / "vault-path"
    vault_dir.mkdir()
    _write_fake_vault(vault_dir / "symvault", "kill -TERM $$")
    env["PATH"] = str(vault_dir)
def setup_vault_tty_fixture(root: Path, env: dict[str, str]) -> None:
    vault_dir = root / "vault-path"
    vault_dir.mkdir()
    _write_fake_vault(
        vault_dir / "symvault",
        "for fd in 0 1 2; do if [ -t \"$fd\" ]; then printf '1'; else printf '0'; fi; done; printf '\\n'",
    )
    env["PATH"] = str(vault_dir)
def setup_vault_env_override(root: Path, env: dict[str, str]) -> None:
    path_dir = root / "vault-path"
    path_dir.mkdir()
    _write_fake_vault(path_dir / "symvault", "printf 'path-child\\n'")
    managed_dir = Path(env["HOME"]) / ".symaira" / "bin"
    managed_dir.mkdir(parents=True)
    _write_fake_vault(managed_dir / "symvault", "printf 'managed-child\\n'")
    configured_dir = root / "env-configured"
    configured_dir.mkdir()
    configured = configured_dir / "symvault"
    _write_fake_vault(configured, "printf 'env-configured-child\\n'")
    env["SYMBRAIN_SERVERS_VAULT_BINARY_PATH"] = str(configured)
    env["PATH"] = str(path_dir)
def setup_vault_config_missing(root: Path, env: dict[str, str]) -> None:
    empty_path = root / "empty-path"
    empty_path.mkdir()
    config_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    config_dir.mkdir(parents=True)
    missing = root / "missing" / "symvault"
    _write_text(config_dir / "config.toml",
        f"[servers.vault]\nbinary_path = {json.dumps(str(missing))}\n"
    )
    env["PATH"] = str(empty_path)
def setup_vault_empty_config(root: Path, env: dict[str, str]) -> None:
    setup_vault_path_fixture(root, env)
    config_dir = Path(env["XDG_CONFIG_HOME"]) / "symbrain"
    config_dir.mkdir(parents=True)
    _write_text(config_dir / "config.toml", '[servers.vault]\nbinary_path = ""\n')
def setup_vault_raw_env_missing(root: Path, env: dict[str, str]) -> None:
    empty_path = root / "empty-path"
    empty_path.mkdir()
    env["PATH"] = str(empty_path)
    env["SYMBRAIN_SERVERS_VAULT_BINARY_PATH"] = os.fsdecode(b"/tmp/missing_\xff")
def setup_release_fixture(root: Path, env: dict[str, str]) -> None:
    empty_path = root / "empty-path"
    empty_path.mkdir()
    env["PATH"] = str(empty_path)
    env["SYMBRAIN_RELEASE_BASE_URL"] = RELEASE_BASE_URL
def setup_correct_managed_binaries(root: Path, env: dict[str, str]) -> None:
    setup_release_fixture(root, env)
    bin_dir = Path(env["HOME"]) / ".symaira" / "bin"
    bin_dir.mkdir(parents=True)
    versions = {
        "symvault": "0.21.1",
        "symcockpit": "0.5.3",
        "symdesk": "0.11.1",
    }
    for name, version in versions.items():
        script = f"#!/bin/sh\nprintf '{{\"version\":\"{version}\"}}\\n'\n"
        path = bin_dir / name
        _write_text(path, script)
        path.chmod(0o755)
def setup_mismatched_managed_binaries(root: Path, env: dict[str, str]) -> None:
    setup_correct_managed_binaries(root, env)
    path = Path(env["HOME"]) / ".symaira" / "bin" / "symdesk"
    _write_text(path, "#!/bin/sh\nprintf '{\"version\":\"0.0.0\"}\\n'\n")
    path.chmod(0o755)
def setup_failing_cosign(root: Path, env: dict[str, str]) -> None:
    setup_release_fixture(root, env)
    cosign = Path(env["PATH"]) / "cosign"
    _write_text(cosign, "#!/bin/sh\nprintf 'cosign stdout\\n'\nprintf 'cosign stderr\\n' >&2\nexit 9\n")
    cosign.chmod(0o755)
class ReleaseFixtureServer:
    def __init__(self) -> None:
        self.routes = self._build_routes()
        routes = self.routes
        class Handler(http.server.BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                body = routes.get(self.path)
                if body is None:
                    self.send_error(404)
                    return
                self.send_response(200)
                self.send_header("Content-Type", "application/octet-stream")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            def log_message(self, format: str, *args: object) -> None:
                del format, args
                return
        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
    def __enter__(self) -> str:
        self.thread.start()
        host = str(self.server.server_address[0])
        port = int(self.server.server_address[1])
        return f"http://{host}:{port}"
    def __exit__(self, *_args: object) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join()
    @staticmethod
    def _build_routes() -> dict[str, bytes]:
        target_os = "darwin" if sys.platform == "darwin" else "windows" if os.name == "nt" else "linux"
        machine = platform.machine().lower()
        target_arch = "arm64" if machine in {"arm64", "aarch64"} else "amd64"
        cores = (
            ("danieljustus/symaira-vault", "v0.21.1", "symvault", target_arch),
            ("danieljustus/symaira-desktop", "v0.11.1", "symdesk", target_arch),
        )
        if target_os == "darwin":
            cores += (("danieljustus/symaira-cockpit", "v0.5.3", "symcockpit", "universal"),)
        routes: dict[str, bytes] = {}
        extension = "zip" if target_os == "windows" else "tar.gz"
        for repo, tag, binary, arch in cores:
            asset = f"{binary}_{target_os}_{arch}.{extension}"
            version = tag.removeprefix("v")
            executable = (
                f"#!/bin/sh\nprintf '{{\"version\":\"{version}\"}}\\n'\n".encode()
            )
            if extension == "zip":
                output = io.BytesIO()
                with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
                    archive.writestr(binary, executable)
                archive_bytes = output.getvalue()
            else:
                output = io.BytesIO()
                with gzip.GzipFile(fileobj=output, mode="wb", mtime=0) as compressed:
                    with tarfile.open(fileobj=compressed, mode="w") as archive:
                        info = tarfile.TarInfo(binary)
                        info.mode = 0o755
                        info.size = len(executable)
                        info.mtime = 0
                        archive.addfile(info, io.BytesIO(executable))
                archive_bytes = output.getvalue()
            prefix = f"/{repo}/releases/download/{tag}/"
            routes[prefix + asset] = archive_bytes
            digest = hashlib.sha256(archive_bytes).hexdigest()
            routes[prefix + "checksums.txt"] = f"{digest}  {asset}\n".encode()
            if binary == "symvault":
                routes[prefix + asset + ".sig"] = b"fixture-signature"
                routes[prefix + asset + ".pem"] = b"fixture-certificate"
        return routes

# --- skills status: OpenCode user-scope root fixtures ----------------------
#
# The native Rust slice covers `skills status --target opencode` in user scope
# with a purely default configuration. These setups build exactly those root
# states so the differential run compares Go bytes against Rust bytes for each
# of them instead of trusting the Rust unit-test expectations.
OPENCODE_SKILLS_SUBDIR = "home/.config/opencode/skills"
MANAGED_SKILL_MD = """---
name: demo
description: Managed status fixture skill
license: Apache-2.0
---

# Demo

Body text.
"""


def opencode_skills_root(root: Path) -> Path:
    path = root / OPENCODE_SKILLS_SUBDIR
    path.mkdir(parents=True, exist_ok=True)
    return path


def write_skills_marker(directory: Path, **overrides: object) -> None:
    marker: dict[str, object] = {
        "schema_version": 1,
        "managed_by": "symskills",
        "target": "opencode",
        "name": directory.name,
        "mode": "copy",
        "installed": "2026-01-02T03:04:05Z",
        "source_hash": "abc123",
        "allow_executable": True,
    }
    marker.update(overrides)
    _write_text(directory / ".symskills.json", json.dumps(marker), encoding="utf-8")


def write_opencode_skill(root: Path, name: str) -> Path:
    directory = opencode_skills_root(root) / name
    directory.mkdir(parents=True, exist_ok=True)
    _write_text(directory / "SKILL.md", f"{name}\n", encoding="utf-8")
    return directory


def setup_skills_opencode_root_empty(root: Path, env: dict[str, str]) -> None:
    opencode_skills_root(root)


def setup_skills_opencode_unmanaged_skill(root: Path, env: dict[str, str]) -> None:
    write_opencode_skill(root, "handwritten")


def setup_skills_opencode_dir_without_skill(root: Path, env: dict[str, str]) -> None:
    directory = opencode_skills_root(root) / "notaskill"
    directory.mkdir(parents=True)
    _write_text(directory / "notes.txt", "notes\n", encoding="utf-8")


def setup_skills_opencode_regular_file(root: Path, env: dict[str, str]) -> None:
    _write_text(opencode_skills_root(root) / "README.md", "readme\n", encoding="utf-8")


def setup_skills_opencode_dangling_symlink(root: Path, env: dict[str, str]) -> None:
    skills = opencode_skills_root(root)
    os.symlink(skills / "gone", skills / "dangling")


def setup_skills_opencode_symlink_to_dir(root: Path, env: dict[str, str]) -> None:
    # Go reads the marker through the link; a link onto a real directory with no
    # marker stays an unmanaged row.
    skills = opencode_skills_root(root)
    target = root / "home/.config/opencode/outside-skill"
    target.mkdir(parents=True, exist_ok=True)
    _write_text(target / "SKILL.md", "outside\n", encoding="utf-8")
    os.symlink(target, skills / "linked")


def setup_skills_opencode_symlink_to_file(root: Path, env: dict[str, str]) -> None:
    skills = opencode_skills_root(root)
    target = root / "home/.config/opencode/outside-file.md"
    _write_text(target, "outside\n", encoding="utf-8")
    os.symlink(target, skills / "linkedfile")


def setup_skills_opencode_symlink_to_marker_file(root: Path, env: dict[str, str]) -> None:
    # A link whose target is a marker-shaped regular file: Go reads
    # `<link>/.symskills.json`, fails, and reports the entry as unmanaged.
    skills = opencode_skills_root(root)
    target = root / "home/.config/opencode/marker-file.json"
    _write_text(target,
        json.dumps(
            {
                "schema_version": 1,
                "managed_by": "symskills",
                "target": "opencode",
                "name": "markerfile",
                "mode": "copy",
                "installed": "2026-01-02T03:04:05Z",
                "source_hash": "abc123",
            }
        ),
        encoding="utf-8",
    )
    os.symlink(target, skills / "markerfile")


def setup_skills_opencode_symlink_chain(root: Path, env: dict[str, str]) -> None:
    # A second link level is the one shape the native scan refuses to follow.
    real = root / "home/.config/opencode/real-dir"
    real.mkdir(parents=True, exist_ok=True)
    _write_text(real / "SKILL.md", "real\n", encoding="utf-8")
    middle = root / "home/.config/opencode/middle-link"
    os.symlink(real, middle)
    os.symlink(middle, opencode_skills_root(root) / "chain")


def setup_skills_opencode_orphaned_marker(root: Path, env: dict[str, str]) -> None:
    write_skills_marker(write_opencode_skill(root, "managed"))


def setup_skills_opencode_foreign_marker(root: Path, env: dict[str, str]) -> None:
    directory = write_opencode_skill(root, "foreign")
    write_skills_marker(directory, managed_by="someone-else")


def setup_skills_opencode_legacy_hash_only_marker(root: Path, env: dict[str, str]) -> None:
    directory = write_opencode_skill(root, "legacy")
    _write_text(directory / ".symskills.json",
        json.dumps({"source_hash": "abc123"}), encoding="utf-8"
    )


def setup_skills_opencode_malformed_marker(root: Path, env: dict[str, str]) -> None:
    directory = write_opencode_skill(root, "broken")
    _write_text(directory / ".symskills.json", "{not json", encoding="utf-8")


def setup_skills_opencode_unsupported_schema_marker(root: Path, env: dict[str, str]) -> None:
    # Go ignores an unknown schema_version and keeps every marker field; the
    # native scan hands this entry back to Go instead of reporting it stale.
    write_skills_marker(write_opencode_skill(root, "schema"), schema_version=99)


def setup_skills_opencode_empty_marker(root: Path, env: dict[str, str]) -> None:
    directory = write_opencode_skill(root, "emptymarker")
    _write_text(directory / ".symskills.json", "", encoding="utf-8")


def setup_skills_opencode_wrong_type_marker(root: Path, env: dict[str, str]) -> None:
    write_skills_marker(write_opencode_skill(root, "wrongtype"), installed=5)


def setup_skills_opencode_mixed(root: Path, env: dict[str, str]) -> None:
    for name in ("zebra", "alpha", "middle"):
        write_opencode_skill(root, name)
    _write_text(opencode_skills_root(root) / "README.md", "ignored\n", encoding="utf-8")


def project_scope_fixture(setup: Callable[[Path, dict[str, str]], None]) -> Callable[[Path, dict[str, str]], None]:
    """Reuse a user-scope fixture builder against the project-scope root.

    Project scope reads `<project>/.opencode/skills`, while every user-scope
    builder (and `symbrain sync`) writes under `$HOME`, so the freshly built
    tree is moved into place. Builders whose symlinks point into the user root
    are not wrapped: moving the root would dangle those links and silently turn
    them into a different fixture.
    """

    def wrapped(root: Path, env: dict[str, str]) -> None:
        setup(root, env)
        user = root / OPENCODE_SKILLS_SUBDIR
        if user.exists():
            project = root / "project/.opencode/skills"
            project.parent.mkdir(parents=True, exist_ok=True)
            shutil.move(str(user), str(project))

    return wrapped


def setup_skills_opencode_project_symlink_to_file(root: Path, env: dict[str, str]) -> None:
    skills = root / "project/.opencode/skills"
    skills.mkdir(parents=True)
    target = skills.parent / "outside-file.md"
    _write_text(target, "outside\n", encoding="utf-8")
    os.symlink(target, skills / "linkedfile")


def setup_skills_opencode_project_symlink_chain(root: Path, env: dict[str, str]) -> None:
    skills = root / "project/.opencode/skills"
    skills.mkdir(parents=True)
    real = skills.parent / "real-dir"
    real.mkdir(parents=True)
    _write_text(real / "SKILL.md", "real\n", encoding="utf-8")
    middle = skills.parent / "middle-link"
    os.symlink(real, middle)
    os.symlink(middle, skills / "chain")


def setup_skills_opencode_project_isolated_from_user_root(root: Path, env: dict[str, str]) -> None:
    # The project scan must ignore whatever the user root contains.
    write_opencode_skill(root, "user-only")
    skills = root / "project/.opencode/skills"
    skills.mkdir(parents=True)
    projects_skill = skills / "only-here"
    projects_skill.mkdir()
    _write_text(projects_skill / "SKILL.md", "only-here\n", encoding="utf-8")


SKILLS_LIBRARY_STAMP = 1767323045  # 2026-01-02T03:04:05Z


def write_library_skill(root: Path, name: str) -> Path:
    """Write one library skill with a frozen timestamp.

    `skills list --json` reports created/modified times straight from the
    filesystem, so both comparison roots must carry identical mtimes.
    """
    directory = root / "data/symbrain/skills/library" / name
    directory.mkdir(parents=True, exist_ok=True)
    _write_text(directory / "SKILL.md",
        f"---\nname: {name}\ndescription: library fixture\nlicense: Apache-2.0\n---\n\n# {name}\n\nBody.\n",
        encoding="utf-8",
    )
    os.utime(directory / "SKILL.md", (SKILLS_LIBRARY_STAMP, SKILLS_LIBRARY_STAMP))
    os.utime(directory, (SKILLS_LIBRARY_STAMP, SKILLS_LIBRARY_STAMP))
    return directory


def write_symskills_config(root: Path, body: str) -> None:
    config = root / "config/symskills/config.toml"
    config.parent.mkdir(parents=True, exist_ok=True)
    _write_text(config, body, encoding="utf-8")


def setup_skills_targets_mixed(root: Path, env: dict[str, str]) -> None:
    write_opencode_skill(root, "handwritten")
    directory = write_opencode_skill(root, "managed")
    write_skills_marker(directory)
    _write_text(opencode_skills_root(root) / "README.md", "ignored\n", encoding="utf-8")


def setup_skills_targets_empty_root(root: Path, env: dict[str, str]) -> None:
    opencode_skills_root(root)


def setup_skills_targets_managed_only(root: Path, env: dict[str, str]) -> None:
    write_skills_marker(write_opencode_skill(root, "managed"))


def setup_skills_targets_config_dir_evidence(root: Path, env: dict[str, str]) -> None:
    # A present harness config directory is evidence even without a binary.
    (root / "home/.claude").mkdir(parents=True, exist_ok=True)


def setup_skills_targets_project_scope(root: Path, env: dict[str, str]) -> None:
    skills = root / "project/.opencode/skills"
    directory = skills / "managed"
    directory.mkdir(parents=True)
    write_skills_marker(directory)


def setup_skills_targets_symlink_entry(root: Path, env: dict[str, str]) -> None:
    skills = opencode_skills_root(root)
    target = root / "home/.config/opencode/outside"
    target.mkdir(parents=True, exist_ok=True)
    _write_text(target / "SKILL.md", "outside\n", encoding="utf-8")
    os.symlink(target, skills / "linked")


def setup_skills_dynamic_config(root: Path, env: dict[str, str]) -> None:
    write_library_skill(root, "demo")
    write_symskills_config(root, 'library_dir = "/nonexistent/library"\n')


def setup_skills_event_log(root: Path, env: dict[str, str]) -> None:
    log = root / "home/.local/share/symskills/events.jsonl"
    log.parent.mkdir(parents=True, exist_ok=True)
    _write_text(log,
        json.dumps(
            {
                "ts": "2026-01-02T03:04:05Z",
                "event": "install",
                "skill": "demo",
                "target": "opencode",
                "path": "/nonexistent/installed/demo",
                "outcome": "ok",
            }
        )
        + "\n",
        encoding="utf-8",
    )


def setup_activity_profile(root: Path, env: dict[str, str]) -> None:
    """A profile that explicitly grants the activity read tools."""
    profiles = Path(env["XDG_CONFIG_HOME"]) / "symbrain" / "profiles"
    profiles.mkdir(parents=True, exist_ok=True)
    _write_text(profiles / "reader.toml",
        '[profile]\nname = "reader"\n'
        "[servers.memory]\nenabled = true\n"
        'tools_allow = ["activity_status", "activity_get", "activity_search"]\n'
    )


def setup_skills_library_fixture(root: Path, env: dict[str, str]) -> None:
    for name in ("demo", "second"):
        write_library_skill(root, name)


def setup_skills_opencode_escapable_name(root: Path, env: dict[str, str]) -> None:
    # Go encodes skills reports with `json.Encoder`, which escapes `&`, `<` and
    # `>`; a name carrying those bytes proves the native encoder matches.
    write_opencode_skill(root, "a&b<c>d")


def setup_memory_rules(root: Path, env: dict[str, str]) -> None:
    """One rule row, inserted with the shipped column set and frozen values."""
    setup_memory_seeded(root, env)
    database = root / "data/symbrain/memory/default.db"
    connection = sqlite3.connect(database)
    try:
        connection.execute(
            "INSERT INTO rules(id, content, scope, metadata, created_at, updated_at, created_by, updated_by) "
            "VALUES(?,?,?,?,?,?,?,?)",
            (
                "00000000-0000-4000-8000-000000000003",
                "always verify before publishing",
                "global",
                json.dumps({"source": "fixture"}),
                "2026-01-02 03:04:05 +0000 UTC",
                "2026-01-02 03:04:05 +0000 UTC",
                "cli:symbrain",
                "cli:symbrain",
            ),
        )
        connection.commit()
    finally:
        connection.close()


def setup_memory_seeded(root: Path, env: dict[str, str]) -> None:
    """One memory and one query-log row, written by the pinned Go binary.

    The clock is pinned afterwards so the two comparison roots produce the same
    bytes; `memory list` reports created/updated timestamps straight from the
    database.
    """
    subprocess.run(
        [
            env["SYMBRAIN_GO_BINARY"],
            "memory",
            "set",
            "alpha memory content",
            "--kind",
            "user",
            "--scope",
            "global",
        ],
        env=env,
        cwd=env["PROJECT"],
        input=b"",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        timeout=60,
    )
    subprocess.run(
        [env["SYMBRAIN_GO_BINARY"], "memory", "search", "alpha"],
        env=env,
        cwd=env["PROJECT"],
        input=b"",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        timeout=60,
    )
    # A second row shares the first row's embedding below, so ranking has two
    # candidates to order; its own text never reaches the query.
    subprocess.run(
        [
            env["SYMBRAIN_GO_BINARY"],
            "memory",
            "set",
            "beta memory content",
            "--kind",
            "user",
            "--scope",
            "global",
        ],
        env=env,
        cwd=env["PROJECT"],
        input=b"",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        timeout=60,
    )
    database = root / "data/symbrain/memory/default.db"
    metadata = {
        "authority": "direct",
        "confidence": "high",
        "observed_at": "2026-01-02T03:04:05Z",
        "sensitivity": "internal",
        "sharing_level": "private",
        "source_tool": "symbrain-cli",
        "source_type": "direct",
        "source_uri": "",
        "verification_status": "unverified",
    }
    connection = sqlite3.connect(database)
    try:
        # Identifiers and metadata carry generation-time values; both are
        # pinned so the two roots produce identical bytes.
        connection.execute(
            """
            UPDATE memories
               SET id = ?, content = ?, metadata = ?, created_at = ?, updated_at = ?
             WHERE content = 'alpha memory content'
            """,
            (
                "00000000-0000-4000-8000-000000000001",
                "alpha memory content",
                json.dumps(metadata),
                "2026-01-02 03:04:05",
                "2026-01-02 03:04:05",
            ),
        )
        # The second memory keeps the first memory's vector, so both rows are
        # candidates for the same query and only importance separates them.
        connection.execute(
            """
            UPDATE memories SET
                embedding = (SELECT embedding FROM memories WHERE content = 'alpha memory content'),
                embedding_dim = (SELECT embedding_dim FROM memories WHERE content = 'alpha memory content'),
                embedding_source = (SELECT embedding_source FROM memories WHERE content = 'alpha memory content'),
                embedding_model = (SELECT embedding_model FROM memories WHERE content = 'alpha memory content'),
                embedding_quantization = (
                    SELECT embedding_quantization FROM memories WHERE content = 'alpha memory content'
                ),
                lsh_hash = (SELECT lsh_hash FROM memories WHERE content = 'alpha memory content'),
                content = ?,
                importance = ?,
                metadata = ?,
                created_at = ?,
                updated_at = ?
            WHERE content = 'beta memory content'
            """,
            (
                "beta memory content",
                0.9,
                json.dumps(metadata),
                "2026-01-02 03:04:05",
                "2026-01-02 03:04:05",
            ),
        )
        connection.execute(
            "UPDATE memories SET importance = 0.5 WHERE content = 'alpha memory content'"
        )
        connection.execute(
            "UPDATE memories SET id = ? WHERE content = 'beta memory content'",
            ("00000000-0000-4000-8000-000000000003",),
        )
        connection.execute(
            "UPDATE query_log SET id = ?, created_at = ?",
            ("00000000-0000-4000-8000-000000000002", "2026-01-02 03:04:05"),
        )
        # Every non-NULL datetime column carries a generation-time value; set
        # them to the frozen stamp while leaving NULLs NULL, so the row means
        # the same thing in both roots.
        for column in (
            "valid_from",
            "valid_to",
            "expires_at",
            "last_access",
            "retired_at",
        ):
            try:
                connection.execute(
                    f"UPDATE memories SET {column} = '2026-01-02 03:04:05'"
                    f" WHERE {column} IS NOT NULL"
                )
            except sqlite3.OperationalError:
                pass
        for table in ("memory_entities",):
            try:
                connection.execute(
                    f"UPDATE {table} SET memory_id = '00000000-0000-4000-8000-000000000001'"
                )
            except sqlite3.OperationalError:
                pass
        # Both rows must look identical apart from what is pinned above.
        connection.execute("UPDATE memories SET scope = 'global', kind = 'user'")
        connection.commit()
    finally:
        connection.close()
    os.utime(database, (SKILLS_LIBRARY_STAMP, SKILLS_LIBRARY_STAMP))


def set_access_times(root: Path, name: str, access: float) -> None:
    """Give an installed skill's SKILL.md an explicit access time.

    `last_used` needs a read that happened after the file's own mtime, so the
    fixture sets atime and mtime explicitly instead of relying on the mount's
    relatime behaviour. Symlink-mode installs point at the render cache, so
    both locations are pinned.
    """
    candidates = [
        root / "home/.config/opencode/skills" / name / "SKILL.md",
        root / "data/symbrain/skills/rendered/opencode" / name / "SKILL.md",
    ]
    for path in candidates:
        if path.is_file():
            os.utime(path, (access, SKILLS_LIBRARY_STAMP))


def setup_skills_library_categories(root: Path, env: dict[str, str]) -> None:
    """Case variants and stray whitespace must collapse to one spelling."""
    for name, category in (("alpha", "Guides"), ("beta", "  guides  "), ("gamma", "GUIDES")):
        directory = write_library_skill(root, name)
        _write_text(directory / "SKILL.md",
            f"---\nname: {name}\ndescription: {name} skill\nlicense: Apache-2.0\ncategory: \"{category}\"\n---\n\n# {name}\n",
            encoding="utf-8",
        )
        os.utime(directory / "SKILL.md", (SKILLS_LIBRARY_STAMP, SKILLS_LIBRARY_STAMP))
        os.utime(directory, (SKILLS_LIBRARY_STAMP, SKILLS_LIBRARY_STAMP))


def setup_skills_library_managed(root: Path, env: dict[str, str]) -> None:
    """A library skill with a real install: installs and last_rendered_at."""
    write_library_skill(root, "demo")
    directory = root / "data/symbrain/skills/library/demo"
    _write_text(directory / "SKILL.md", MANAGED_SKILL_MD, encoding="utf-8")
    subprocess.run(
        [env["SYMBRAIN_GO_BINARY"], "sync", "opencode"],
        env=env,
        cwd=env["PROJECT"],
        input=b"",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        timeout=60,
    )
    freeze_managed_clocks(root)


def setup_skills_library_last_used(root: Path, env: dict[str, str]) -> None:
    setup_skills_library_managed(root, env)
    set_access_times(root, "demo", SKILLS_LIBRARY_STAMP + 3600.5)


def setup_skills_library_last_used_below_gap(root: Path, env: dict[str, str]) -> None:
    """atime newer than mtime but inside the install gap: still no evidence."""
    setup_skills_library_managed(root, env)
    set_access_times(root, "demo", SKILLS_LIBRARY_STAMP + 30)


def setup_skills_library_broken_skill(root: Path, env: dict[str, str]) -> None:
    write_library_skill(root, "good")
    directory = root / "data/symbrain/skills/library/broken"
    directory.mkdir(parents=True)
    _write_text(directory / "SKILL.md", "no frontmatter here\n", encoding="utf-8")


def setup_skills_library_missing_skill_md(root: Path, env: dict[str, str]) -> None:
    write_library_skill(root, "good")
    (root / "data/symbrain/skills/library/empty-dir").mkdir(parents=True)


def capture_access_time_ns(root: Path, name: str) -> int | None:
    """Return an installed skill's SKILL.md atime in nanoseconds.

    `last_used` is the access time the runtime itself observes, so the parity
    harness has to read the very same value immediately before the runtime
    runs. Nanoseconds come from `st_atime_ns`: the float `st_atime` cannot
    represent a nanosecond epoch timestamp exactly.
    """
    candidates = [
        root / "home/.config/opencode/skills" / name / "SKILL.md",
        root / "data/symbrain/skills/rendered/opencode" / name / "SKILL.md",
    ]
    for path in candidates:
        try:
            return path.stat().st_atime_ns
        except OSError:
            continue
    return None


def freeze_managed_clocks(root: Path) -> None:
    """Pin the clocks the pinned Go binary wrote, so both comparison roots
    produce the same bytes instead of differing by a wall-clock second.

    The install marker, the operation log and every library file are frozen:
    `skills list --json` reports marker timestamps, `last_rendered_at` comes
    from the log, and `created_at`/`modified_at` come from the filesystem.
    """
    for marker_file in sorted(root.rglob(".symskills.json")):
        data = json.loads(marker_file.read_text(encoding="utf-8"))
        if "installed" in data:
            data["installed"] = "2026-01-02T03:04:05Z"
            _write_text(marker_file, json.dumps(data), encoding="utf-8")
    log = root / "home/.local/share/symskills/events.jsonl"
    if log.is_file():
        lines = []
        for line in log.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            record = json.loads(line)
            record["ts"] = "2026-01-02T03:04:05Z"
            lines.append(json.dumps(record))
        _write_text(log, "\n".join(lines) + "\n", encoding="utf-8")
    for path in sorted(root.rglob("SKILL.md")):
        os.utime(path, (SKILLS_LIBRARY_STAMP, SKILLS_LIBRARY_STAMP))
        os.utime(path.parent, (SKILLS_LIBRARY_STAMP, SKILLS_LIBRARY_STAMP))


def generate_managed_install(root: Path, env: dict[str, str]) -> Path:
    """Install one library skill with the pinned Go binary.

    The library lives where the packaged defaults point (XDG data home plus
    `symbrain/skills/library`), so the resulting marker, render cache and base
    snapshot are genuine artifacts of the reference implementation.
    """
    skill_dir = root / "data/symbrain/skills/library/demo"
    skill_dir.mkdir(parents=True, exist_ok=True)
    _write_text(skill_dir / "SKILL.md", MANAGED_SKILL_MD, encoding="utf-8")
    subprocess.run(
        [env["SYMBRAIN_GO_BINARY"], "sync", "opencode"],
        env=env,
        cwd=env["PROJECT"],
        input=b"",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=True,
        timeout=60,
    )
    freeze_managed_clocks(root)
    return opencode_skills_root(root) / "demo"


def convert_managed_install_to_copy(root: Path, env: dict[str, str]) -> Path:
    """Deliver the generated install as a copy instead of a link.

    `symbrain sync` installs in symlink mode only, but copy mode is a real
    marker state (`mode = "copy"`) that status must classify. The tree is the
    Go-generated render, copied verbatim, and only the marker's delivery mode
    is rewritten — no content is invented.
    """
    installed = generate_managed_install(root, env)
    if installed.is_symlink():
        rendered = root / "data/symbrain/skills/rendered/opencode/demo"
        installed.unlink()
        shutil.copytree(rendered, installed)
    # Without symlink privileges (Windows) the pinned Go binary already
    # delivers a copy; only the marker's mode is normalized either way.
    write_skills_marker(installed, mode="copy")
    return installed


def setup_skills_opencode_managed_symlink(root: Path, env: dict[str, str]) -> None:
    generate_managed_install(root, env)


def setup_skills_opencode_managed_copy(root: Path, env: dict[str, str]) -> None:
    convert_managed_install_to_copy(root, env)


def setup_skills_opencode_managed_harness_changed(root: Path, env: dict[str, str]) -> None:
    installed = convert_managed_install_to_copy(root, env)
    with (installed / "SKILL.md").open("a", encoding="utf-8") as handle:
        handle.write("\n# harness edit\n")


def setup_skills_opencode_managed_conflict(root: Path, env: dict[str, str]) -> None:
    installed = convert_managed_install_to_copy(root, env)
    with (installed / "SKILL.md").open("a", encoding="utf-8") as handle:
        handle.write("\n# harness edit\n")
    library = root / "data/symbrain/skills/library/demo/SKILL.md"
    with library.open("a", encoding="utf-8") as handle:
        handle.write("\n# library edit\n")


def setup_skills_opencode_managed_orphaned(root: Path, env: dict[str, str]) -> None:
    generate_managed_install(root, env)
    shutil.rmtree(root / "data/symbrain/skills/library/demo")


@dataclass(frozen=True)
class Case:
    name: str
    argv: tuple[str | bytes, ...]
    env_overrides: dict[str, str] | None = None
    normalize_runtime: bool = False
    setup: Callable[[Path, dict[str, str]], None] | None = None
    normalize_parse_error: bool = False
    normalize_os_error: bool = False
    normalize_uuid: bool = False
    normalize_atime: bool = False
    require_last_used: bool = False
    mutating: bool = False
    posix_only: bool = False
    windows_skip_reason: str | None = None
    stdin: bytes | None = None
    pty: bool = False
CASES = (
    Case("no_args", ()),
    Case("help", ("help",)),
    Case("help_long", ("--help",)),
    Case("help_short", ("-h",)),
    Case("version_human", ("version",), normalize_runtime=True),
    Case("version_terminator", ("version", "--"), normalize_runtime=True),
    Case("version_json", ("version", "--json")),
    Case("version_terminator_json", ("version", "--", "--json")),
    Case("version_json_before", ("--json", "version")),
    Case("version_output_equals", ("version", "--output=json")),
    Case("version_missing_output_value", ("version", "--output")),
    Case("version_output_single_missing", ("version", "-output")),
    Case("version_conflicting_output", ("version", "--json", "--output", "table")),
    Case("version_unknown_flag", ("version", "--bogus")),
    Case("version_flag_value_double", ("version", "--bogus=123")),
    Case("version_flag_value_single", ("version", "-bogus=123")),
    Case("version_bare_hyphen", ("version", "-")),
    Case("version_help_short", ("version", "-h")),
    Case("version_help_single_dash", ("version", "-help")),
    Case("version_help_double_dash", ("version", "--help")),
    Case("version_unexpected_argument", ("version", "extra")),
    Case("version_terminator_extra", ("version", "--", "extra")),
    Case("version_terminator_flag", ("version", "--", "--extra")),
    Case("version_version", ("version", "version")),
    Case("version_json_version", ("--json", "version", "version")),
    Case("json_help", ("--json", "help")),
    Case("config_path", ("config", "path")),
    Case("config_path_extra", ("config", "path", "extra")),
    Case("config_path_unexpected_double", ("config", "path", "--unexpected")),
    Case("config_path_unexpected_single", ("config", "path", "-unexpected")),
    Case("config_path_unexpected_terminator", ("config", "path", "--")),
    Case(
        "config_path_no_env",
        ("config", "path"),
        env_overrides={"HOME": "", "XDG_CONFIG_HOME": ""},
    ),
    Case(
        "config_path_relative_xdg",
        ("config", "path"),
        env_overrides={"HOME": "", "XDG_CONFIG_HOME": "relative/config"},
    ),
    Case("unknown_command_quoted", ("bogus\"cmd",)),
    Case("unknown_fallback", ("definitely-not-a-command",)),
    # Phase 1 Task 1.2: native config get parity cases
    Case("config_get_no_file", ("config", "get")),
    Case("config_get_no_file_key", ("config", "get", "default_profile")),
    # Unix argv bytes must be rendered as Go %q escapes, not U+FFFD.
    Case("config_get_raw_ff_key", ("config", "get", b"\xff"), setup=setup_xdg_config),
    Case("config_get_raw_latin1_key", ("config", "get", b"cafe\xe9"), setup=setup_xdg_config),
    Case("config_get_raw_nel_key", ("config", "get", b"\xc2\x85"), setup=setup_xdg_config),
    Case("config_get_raw_nbsp_key", ("config", "get", b"\xc2\xa0"), setup=setup_xdg_config),
    Case("config_get_raw_zwsp_key", ("config", "get", b"\xe2\x80\x8b"), setup=setup_xdg_config),
    Case("config_get_raw_line_separator_key", ("config", "get", b"\xe2\x80\xa8"), setup=setup_xdg_config),
    Case("config_get_raw_bom_key", ("config", "get", b"\xef\xbb\xbf"), setup=setup_xdg_config),
    Case(
        "config_get_raw_quote_backslash_control_key",
        ("config", "get", b"quote\"slash\\\x01"),
        setup=setup_xdg_config,
    ),
    Case("config_get_unicode_key", ("config", "get", "café"), setup=setup_xdg_config),
    Case("config_get_unicode_replacement_key", ("config", "get", "�"), setup=setup_xdg_config),
    Case("config_get_unicode_emoji_key", ("config", "get", "😀"), setup=setup_xdg_config),
    Case("config_get_unicode_cjk_key", ("config", "get", "東京"), setup=setup_xdg_config),
    Case("config_get_unicode_nel_key", ("config", "get", "\u0085"), setup=setup_xdg_config),
    Case("config_get_unicode_nbsp_key", ("config", "get", "\u00a0"), setup=setup_xdg_config),
    Case("config_get_unicode_zwsp_key", ("config", "get", "\u200b"), setup=setup_xdg_config),
    Case("config_get_unicode_line_separator_key", ("config", "get", "\u2028"), setup=setup_xdg_config),
    Case("config_get_unicode_bom_key", ("config", "get", "\ufeff"), setup=setup_xdg_config),
    Case("config_get_whole_file", ("config", "get"), setup=setup_xdg_config),
    Case("config_get_whole_file_malformed", ("config", "get"), setup=setup_malformed_config),
    Case("config_get_terminator", ("config", "--", "get"), setup=setup_xdg_config),
    Case("config_get_string", ("config", "get", "default_profile"), setup=setup_xdg_config),
    Case("config_get_bool_true", ("config", "get", "audit.enabled"), setup=setup_xdg_config),
    Case("config_get_bool_false", ("config", "get", "audit.verbose"), setup=setup_xdg_config),
    Case("config_get_int", ("config", "get", "patterns.promotion_threshold"), setup=setup_xdg_config),
    Case("config_get_dotted_three", ("config", "get", "servers.vault.binary_path"), setup=setup_xdg_config),
    Case("config_get_table", ("config", "get", "servers.vault"), setup=setup_xdg_config),
    Case("config_get_table_nested", ("config", "get", "audit"), setup=setup_xdg_config),
    Case("config_get_inline_table", ("config", "get", "nested_inline.sub"), setup=setup_xdg_config),
    Case("config_get_inline_table_leaf", ("config", "get", "nested_inline.sub.foo"), setup=setup_xdg_config),
    Case("config_get_list", ("config", "get", "items"), setup=setup_xdg_config),
    Case("config_get_empty_list", ("config", "get", "empty_list"), setup=setup_xdg_config),
    Case("config_get_empty_table", ("config", "get", "empty_table"), setup=setup_xdg_config),
    Case("config_get_missing_key", ("config", "get", "nonexistent.key"), setup=setup_xdg_config),
    Case(
        "config_get_malformed_key",
        ("config", "get", "some.key"),
        setup=setup_malformed_config,
        normalize_parse_error=True,
    ),
    Case("config_get_extra_arg", ("config", "get", "default_profile", "extra"), setup=setup_xdg_config),
    Case(
        "config_get_raw_extra_arg",
        ("config", "get", "default_profile", b"extra\xff"),
        setup=setup_xdg_config,
    ),
    Case("config_get_extra_arg_flag", ("config", "get", "default_profile", "--extra"), setup=setup_xdg_config),
    Case("config_get_key_flag", ("config", "get", "--extra"), setup=setup_xdg_config),
    Case("config_get_home_variant", ("config", "get", "default_profile"), setup=setup_home_config),
    Case(
        "config_get_unreadable_file",
        ("config", "get"),
        setup=setup_unreadable_config,
        normalize_os_error=True,
    ),
    Case(
        "config_get_unreadable_file_key",
        ("config", "get", "default_profile"),
        setup=setup_unreadable_config,
        normalize_os_error=True,
    ),
    # Float parity cases
    Case("config_get_float_pos_zero", ("config", "get", "pos_zero"), setup=setup_floats_config),
    Case("config_get_float_neg_zero", ("config", "get", "neg_zero"), setup=setup_floats_config),
    Case("config_get_float_pos_inf", ("config", "get", "pos_inf"), setup=setup_floats_config),
    Case("config_get_float_pos_inf_plus", ("config", "get", "pos_inf_plus"), setup=setup_floats_config),
    Case("config_get_float_neg_inf", ("config", "get", "neg_inf"), setup=setup_floats_config),
    Case("config_get_float_nan", ("config", "get", "nan"), setup=setup_floats_config),
    Case("config_get_float_pos_nan", ("config", "get", "pos_nan"), setup=setup_floats_config),
    Case("config_get_float_neg_nan", ("config", "get", "neg_nan"), setup=setup_floats_config),
    Case("config_get_float_large", ("config", "get", "large"), setup=setup_floats_config),
    Case("config_get_float_large2", ("config", "get", "large2"), setup=setup_floats_config),
    Case("config_get_float_small", ("config", "get", "small"), setup=setup_floats_config),
    Case("config_get_float_small2", ("config", "get", "small2"), setup=setup_floats_config),
    Case("config_get_float_large_neg", ("config", "get", "large_neg"), setup=setup_floats_config),
    Case("config_get_float_large2_neg", ("config", "get", "large2_neg"), setup=setup_floats_config),
    Case("config_get_float_small_neg", ("config", "get", "small_neg"), setup=setup_floats_config),
    Case("config_get_float_small2_neg", ("config", "get", "small2_neg"), setup=setup_floats_config),
    Case("config_get_float_sci1", ("config", "get", "sci1"), setup=setup_floats_config),
    Case("config_get_float_sci2", ("config", "get", "sci2"), setup=setup_floats_config),
    Case("config_get_float_pi", ("config", "get", "pi"), setup=setup_floats_config),
    Case("config_get_float_boundary_below_large", ("config", "get", "boundary_below_large"), setup=setup_floats_config),
    Case("config_get_float_boundary_above_small", ("config", "get", "boundary_above_small"), setup=setup_floats_config),
    Case("config_get_float_boundary_below_small", ("config", "get", "boundary_below_small"), setup=setup_floats_config),
    # Datetime parity cases
    Case("config_get_odt_z", ("config", "get", "odt_z"), setup=setup_datetime_config),
    Case("config_get_odt_z_lower", ("config", "get", "odt_z_lower"), setup=setup_datetime_config),
    Case("config_get_odt_space_z", ("config", "get", "odt_space_z"), setup=setup_datetime_config),
    Case("config_get_odt_frac_micro_z", ("config", "get", "odt_frac_micro_z"), setup=setup_datetime_config),
    Case("config_get_odt_frac_milli_z", ("config", "get", "odt_frac_milli_z"), setup=setup_datetime_config),
    Case("config_get_odt_frac_nano_z", ("config", "get", "odt_frac_nano_z"), setup=setup_datetime_config),
    Case("config_get_odt_frac_trailing_z", ("config", "get", "odt_frac_trailing_z"), setup=setup_datetime_config),
    Case("config_get_odt_frac_zero_z", ("config", "get", "odt_frac_zero_z"), setup=setup_datetime_config),
    Case("config_get_odt_no_sec_z", ("config", "get", "odt_no_sec_z"), setup=setup_datetime_config),
    Case("config_get_odt_pos_offset", ("config", "get", "odt_pos_offset"), setup=setup_datetime_config),
    Case("config_get_odt_neg_offset", ("config", "get", "odt_neg_offset"), setup=setup_datetime_config),
    Case("config_get_odt_offset_zero", ("config", "get", "odt_offset_zero"), setup=setup_datetime_config),
    Case("config_get_odt_offset_neg_zero", ("config", "get", "odt_offset_neg_zero"), setup=setup_datetime_config),
    Case("config_get_odt_space_offset", ("config", "get", "odt_space_offset"), setup=setup_datetime_config),
    Case("config_get_odt_frac_micro_offset", ("config", "get", "odt_frac_micro_offset"), setup=setup_datetime_config),
    Case("config_get_odt_frac_half_hour_offset", ("config", "get", "odt_frac_half_hour_offset"), setup=setup_datetime_config),
    Case("config_get_odt_frac_trailing_offset", ("config", "get", "odt_frac_trailing_offset"), setup=setup_datetime_config),
    Case("config_get_odt_no_sec_offset", ("config", "get", "odt_no_sec_offset"), setup=setup_datetime_config),
    Case("config_get_ldt", ("config", "get", "ldt"), setup=setup_datetime_config),
    Case("config_get_ldt_space", ("config", "get", "ldt_space"), setup=setup_datetime_config),
    Case("config_get_ldt_lower", ("config", "get", "ldt_lower"), setup=setup_datetime_config),
    Case("config_get_ldt_frac_micro", ("config", "get", "ldt_frac_micro"), setup=setup_datetime_config),
    Case("config_get_ldt_frac_milli", ("config", "get", "ldt_frac_milli"), setup=setup_datetime_config),
    Case("config_get_ldt_frac_nano", ("config", "get", "ldt_frac_nano"), setup=setup_datetime_config),
    Case("config_get_ldt_frac_trailing", ("config", "get", "ldt_frac_trailing"), setup=setup_datetime_config),
    Case("config_get_ldt_frac_zero", ("config", "get", "ldt_frac_zero"), setup=setup_datetime_config),
    Case("config_get_ldt_no_sec", ("config", "get", "ldt_no_sec"), setup=setup_datetime_config),
    Case("config_get_ld", ("config", "get", "ld"), setup=setup_datetime_config),
    Case("config_get_ld_today", ("config", "get", "ld_today"), setup=setup_datetime_config),
    Case("config_get_ld_year_one", ("config", "get", "ld_year_one"), setup=setup_datetime_config),
    Case("config_get_lt", ("config", "get", "lt"), setup=setup_datetime_config),
    Case("config_get_lt_frac_micro", ("config", "get", "lt_frac_micro"), setup=setup_datetime_config),
    Case("config_get_lt_frac_milli", ("config", "get", "lt_frac_milli"), setup=setup_datetime_config),
    Case("config_get_lt_frac_nano", ("config", "get", "lt_frac_nano"), setup=setup_datetime_config),
    Case("config_get_lt_frac_trailing", ("config", "get", "lt_frac_trailing"), setup=setup_datetime_config),
    Case("config_get_lt_frac_zero", ("config", "get", "lt_frac_zero"), setup=setup_datetime_config),
    Case("config_get_lt_no_sec", ("config", "get", "lt_no_sec"), setup=setup_datetime_config),
    # Phase 1 Task 1.5: native profile list/show/add parity cases
    Case("profile_help", ("profile", "help")),
    Case("profile_help_long", ("profile", "--help")),
    Case("profile_list_empty", ("profile", "list")),
    Case("profile_list_empty_json", ("profile", "list", "--json")),
    Case("profile_list_fixture_json", ("profile", "list", "--output=json"), setup=setup_profile_fixture),
    Case("profile_list_partial_error", ("profile", "list", "--json"), setup=setup_profile_fixture),
    Case("profile_list_extra", ("profile", "list", "extra")),
    Case("profile_list_help_flag", ("profile", "list", "--help")),
    Case("profile_list_help_short", ("profile", "list", "-h")),
    Case("profile_show_missing", ("profile", "show", "missing")),
    Case("profile_show_missing_json", ("profile", "show", "--json", "missing")),
    Case("profile_show_help_flag", ("profile", "show", "--help")),
    Case("profile_show_help_short", ("profile", "show", "-h")),
    Case("profile_show_fixture_json", ("profile", "show", "good", "--json"), setup=setup_profile_fixture),
    Case("profile_show_fixture_json_flag_before_name", ("profile", "show", "--json", "good"), setup=setup_profile_fixture),
    Case("profile_show_fixture_human", ("profile", "show", "good"), setup=setup_profile_fixture),
    Case("profile_add_default", ("profile", "add", "new-profile"), mutating=True),
    Case("profile_add_personal_after_name", ("profile", "add", "personal-copy", "--from", "personal"), mutating=True),
    Case("profile_add_personal_before_name", ("profile", "add", "--from", "personal", "personal-copy"), mutating=True),
    Case("profile_add_invalid_name", ("profile", "add", "../evil")),
    Case("profile_add_invalid_from", ("profile", "add", "new-profile", "--from", "nope")),
    Case("profile_add_exists", ("profile", "add", "existing"), setup=setup_profile_existing),
    Case("profile_add_help_flag", ("profile", "add", "--help")),
    Case("profile_add_help_short", ("profile", "add", "-h")),
    Case("profile_add_unknown_flag", ("profile", "add", "--bogus")),
    Case("profile_add_unknown_after_name", ("profile", "add", "new-profile", "--bogus")),
    Case("profile_add_missing_from_value", ("profile", "add", "--from")),
    Case("profile_add_from_equals", ("profile", "add", "new-equals", "--from=personal"), mutating=True),
    Case("profile_add_trailing_terminator", ("profile", "add", "new-terminator", "--"), mutating=True),
    Case("profile_add_terminator_then_flag", ("profile", "add", "new-terminator", "--", "--from", "personal"), mutating=True),
    Case("profile_remove_missing", ("profile", "remove", "missing", "--force")),
    Case("profile_remove_force", ("profile", "remove", "existing", "--force"), setup=setup_profile_remove_existing, mutating=True),
    Case("profile_remove_prompt_yes", ("profile", "remove", "existing"), setup=setup_profile_remove_existing, mutating=True, stdin=b"yes\n"),
    Case("profile_remove_prompt_no", ("profile", "remove", "existing"), setup=setup_profile_remove_existing, stdin=b"n\n"),
    Case("profile_remove_prompt_eof", ("profile", "remove", "existing"), setup=setup_profile_remove_existing, stdin=b""),
    Case("profile_remove_prompt_oversized", ("profile", "remove", "existing"), setup=setup_profile_remove_existing, stdin=b"x" * 1025),
    Case("profile_remove_bound_global_refuses", ("profile", "remove", "bound"), setup=setup_profile_remove_bound_global, mutating=True),
    Case("profile_remove_bound_project_refuses", ("profile", "remove", "existing", "--project", "PROJECT"), setup=setup_profile_remove_bound_project, mutating=True),
    Case("profile_remove_bound_project_relative_refuses", ("profile", "remove", "existing", "--project", "."), setup=setup_profile_remove_bound_project, mutating=True),
    Case("profile_remove_bound_project_symlink_refuses", ("profile", "remove", "existing", "--project", "project-link"), setup=setup_profile_remove_bound_project_symlink, mutating=True, posix_only=True),
    Case("profile_remove_bound_force", ("profile", "remove", "bound", "--force"), setup=setup_profile_remove_bound_global, mutating=True),
    Case("profile_remove_malformed_profile", ("profile", "remove", "existing", "--force"), setup=setup_profile_remove_malformed_profile, mutating=True),
    Case("profile_remove_malformed_harness_config", ("profile", "remove", "bound", "--force"), setup=setup_profile_remove_malformed_config, mutating=True),
    Case("profile_remove_malformed_harness_refuses", ("profile", "remove", "bound"), setup=setup_profile_remove_malformed_config, mutating=True),
    Case("profile_remove_symlink", ("profile", "remove", "existing", "--force"), setup=setup_profile_remove_symlink, mutating=True, posix_only=True),
    Case("profile_remove_symlink_profiles_root", ("profile", "remove", "existing", "--force"), setup=setup_profile_remove_symlink_profiles_root, mutating=True, posix_only=True),
    Case("profile_remove_special_file", ("profile", "remove", "existing", "--force"), setup=setup_profile_remove_fifo, mutating=True, posix_only=True),
    Case("profile_remove_empty_directory", ("profile", "remove", "existing", "--force"), setup=setup_profile_remove_directory, mutating=True),
    Case("profile_remove_force_before_name", ("profile", "remove", "--force", "existing"), setup=setup_profile_remove_existing, mutating=True),
    Case("profile_remove_terminator", ("profile", "remove", "existing", "--", "--force"), setup=setup_profile_remove_existing, mutating=True),
    # Phase 2 Task 2.3: native audit tail parity cases
    Case("audit_missing_subcommand", ("audit",), setup=setup_audit_empty),
    Case("audit_unknown_subcommand", ("audit", "unknown"), setup=setup_audit_empty),
    Case("audit_help", ("audit", "--help"), setup=setup_audit_empty),
    Case("audit_tail_empty_json", ("audit", "tail", "--json"), setup=setup_audit_empty),
    Case("audit_tail_missing_profile_json", ("audit", "tail", "--json", "--profile", "missing"), setup=setup_audit_empty),
    Case("audit_tail_fixture_json", ("audit", "tail", "--json", "--profile", "alpha", "-n", "2"), setup=setup_audit_fixture),
    Case("audit_tail_global_json", ("--json", "audit", "tail", "--profile=alpha", "-n=1"), setup=setup_audit_fixture),
    Case("audit_tail_fixture_human", ("audit", "tail", "--profile", "alpha", "-n", "2"), setup=setup_audit_fixture),
    Case("audit_tail_negative_limit", ("audit", "tail", "--json", "--profile", "alpha", "-n", "-1"), setup=setup_audit_fixture),
    Case("audit_tail_help", ("audit", "tail", "--help"), setup=setup_audit_empty),
    Case("audit_tail_unknown_flag", ("audit", "tail", "--bogus"), setup=setup_audit_empty),
    Case("audit_tail_missing_n", ("audit", "tail", "-n"), setup=setup_audit_empty),
    Case("audit_tail_invalid_n", ("audit", "tail", "-n", "nope"), setup=setup_audit_empty),
    # Phase 5 Task 5.3: native doctor baseline and flag-parser parity.
    Case("doctor_empty_human", ("doctor",), setup=setup_doctor_empty),
    Case("doctor_empty_json", ("doctor", "--json"), setup=setup_doctor_empty),
    Case("doctor_global_json", ("--json", "doctor"), setup=setup_doctor_empty),
    Case("doctor_help", ("doctor", "--help")),
    Case("doctor_unknown_flag", ("doctor", "--bogus")),
    Case("doctor_ignores_positionals", ("doctor", "ignored", "--bogus"), setup=setup_doctor_empty),
    # The Go flag package consumes the next argument as the `-vault-agent`
    # value and stops at `-h`/`-help`, so these never reach the handshake that
    # keeps a surviving `-vault-agent` on the Go path.
    Case(
        "doctor_vault_agent_value_consumes_force_release",
        ("doctor", "--vault-agent", "--force-release", "--help"),
    ),
    Case("doctor_vault_agent_then_help", ("doctor", "--vault-agent", "agent", "--help")),
    Case("doctor_vault_agent_equals_then_help", ("doctor", "--vault-agent=agent", "--help")),
    Case("doctor_vault_agent_missing_value", ("doctor", "--vault-agent")),
    Case(
        "doctor_failed_version_json",
        ("doctor", "--json"),
        setup=setup_doctor_failed_version,
        windows_skip_reason="fixture uses an extensionless POSIX shell executable",
    ),
    # Phase 4 Task 4.3: native symvault passthrough with opaque argv and lookup order
    Case(
        "vault_passthrough_argv_stdio",
        ("vault", "--flag", b"raw\xff", "value with spaces"),
        setup=setup_vault_path_fixture,
        posix_only=True,
        stdin=b"stdin payload\n",
    ),
    Case("vault_exit_status", ("vault",), setup=setup_vault_exit_fixture, posix_only=True),
    Case("vault_signal_status", ("vault",), setup=setup_vault_signal_fixture, posix_only=True),
    Case("vault_tty_inheritance", ("vault",), setup=setup_vault_tty_fixture, posix_only=True, pty=True),
    Case("vault_managed_precedes_path", ("vault",), setup=setup_vault_managed_precedence, posix_only=True),
    Case("vault_configured_precedes_managed", ("vault",), setup=setup_vault_config_override, posix_only=True),
    Case("vault_env_override_precedes_config", ("vault",), setup=setup_vault_env_override, posix_only=True),
    Case(
        "vault_missing_binary",
        ("vault",),
        setup=setup_release_fixture,
        windows_skip_reason="release archive fixture contains a POSIX shell stub, not a PE executable",
    ),
    Case("vault_configured_binary_missing", ("vault",), setup=setup_vault_config_missing, posix_only=True),
    Case("vault_empty_config_uses_path", ("vault",), setup=setup_vault_empty_config, posix_only=True),
    Case("vault_raw_env_missing", ("vault",), setup=setup_vault_raw_env_missing, posix_only=True),
    # Phase 4 Task 4.2: native managed setup against immutable local archives
    Case("setup_unknown_flag", ("setup", "--unknown")),
    Case(
        "setup_lone_hyphen",
        ("setup", "-"),
        setup=setup_release_fixture,
        mutating=True,
        windows_skip_reason="release archive fixture contains a POSIX shell stub, not a PE executable",
    ),
    Case(
        "setup_install_json",
        ("setup", "--allow-unsigned", "--json"),
        setup=setup_release_fixture,
        mutating=True,
        windows_skip_reason="release archive fixture contains a POSIX shell stub, not a PE executable",
    ),
    Case(
        "setup_install_human",
        ("setup", "--allow-unsigned"),
        setup=setup_release_fixture,
        mutating=True,
        windows_skip_reason="release archive fixture contains a POSIX shell stub, not a PE executable",
    ),
    Case(
        "setup_install_cosign_fail_json",
        ("setup", "--json"),
        setup=setup_release_fixture,
        mutating=True,
        windows_skip_reason="release archive fixture contains a POSIX shell stub, not a PE executable",
    ),
    Case(
        "setup_cosign_command_fail_json",
        ("setup", "--json"),
        setup=setup_failing_cosign,
        mutating=True,
        posix_only=True,
    ),
    Case(
        "setup_fix_correct_json",
        ("setup", "--fix", "--json"),
        setup=setup_correct_managed_binaries,
        mutating=True,
        posix_only=True,
    ),
    Case(
        "setup_fix_correct_human",
        ("setup", "--fix"),
        setup=setup_correct_managed_binaries,
        mutating=True,
        posix_only=True,
    ),
    Case(
        "setup_fix_mismatched_json",
        ("setup", "--fix", "--allow-unsigned", "--json"),
        setup=setup_mismatched_managed_binaries,
        mutating=True,
        posix_only=True,
    ),
    # Phase 6.3A: native install/uninstall parity, including filesystem side effects.
    Case(
        "install_cursor_fresh",
        ("install", "--harness", "cursor", "--profile", "personal"),
        mutating=True,
    ),
    Case("install_claude_fresh", ("install", "--harness", "claude", "--profile", "personal"), mutating=True),
    Case("install_claude_desktop_fresh", ("install", "--harness", "claude-desktop", "--profile", "personal"), mutating=True),
    Case("install_opencode_fresh", ("install", "--harness", "opencode", "--profile", "personal"), mutating=True),
    Case("install_codex_fresh", ("install", "--harness", "codex", "--profile", "personal"), mutating=True),
    Case("install_antigravity_fresh", ("install", "--harness", "antigravity", "--profile", "personal"), mutating=True),
    Case(
        "install_default_profile_env_override",
        ("install", "--harness", "claude"),
        setup=setup_install_default_profile,
        env_overrides={"SYMBRAIN_DEFAULT_PROFILE": "restricted"},
        mutating=True,
    ),
    Case(
        "install_malformed_global_config",
        ("install", "--harness", "claude"),
        setup=setup_install_malformed_global,
        normalize_parse_error=True,
        mutating=True,
    ),
    Case(
        "install_cursor_equals_flags",
        ("install", "--harness=cursor", "--profile=personal"),
        mutating=True,
    ),
    Case(
        "install_missing_profile",
        ("install", "--harness", "cursor"),
        mutating=True,
    ),
    Case(
        "install_default_profile",
        ("install", "--harness", "claude"),
        setup=setup_install_default_profile,
        mutating=True,
    ),
    Case(
        "install_malformed_refuses_before_backup",
        ("install", "--harness", "claude", "--profile", "personal"),
        setup=setup_install_malformed,
        mutating=True,
    ),
    Case(
        "install_superseded_basenames_keep_vault",
        ("install", "--harness", "cursor", "--profile", "personal"),
        setup=setup_install_superseded,
        mutating=True,
    ),
    Case(
        "install_keep_superseded",
        (
            "install",
            "--harness",
            "cursor",
            "--profile",
            "personal",
            "--keep-superseded",
        ),
        setup=setup_install_superseded,
        mutating=True,
    ),
    Case(
        "install_claude_project",
        (
            "install",
            "--harness",
            "claude",
            "--profile",
            "personal",
            "--project",
            "PROJECT",
        ),
        mutating=True,
    ),
    Case(
        "install_unsupported_project",
        (
            "install",
            "--harness",
            "cursor",
            "--profile",
            "personal",
            "--project",
            "PROJECT",
        ),
        mutating=True,
    ),
    Case(
        "install_dry_run",
        (
            "install",
            "--harness",
            "cursor",
            "--profile",
            "personal",
            "--dry-run",
        ),
        setup=setup_install_existing,
        mutating=True,
    ),
    Case(
        "uninstall_foreign_symbrain_noop",
        ("uninstall", "--harness", "cursor"),
        setup=setup_install_foreign,
        mutating=True,
    ),
    Case("uninstall_missing_config_noop", ("uninstall", "--harness", "cursor"), mutating=True),
    Case(
        "uninstall_absent_entry_noop",
        ("uninstall", "--harness", "cursor"),
        setup=setup_install_existing,
        mutating=True,
    ),
    Case(
        "uninstall_removes_symbrain",
        ("uninstall", "--harness", "cursor"),
        setup=setup_install_installed,
        mutating=True,
    ),
    Case(
        "uninstall_dry_run",
        ("uninstall", "--harness", "cursor", "--dry-run"),
        setup=setup_install_existing,
        mutating=True,
    ),
    # Mutating commands
    Case(
        "config_set_fallback",
        ("config", "set", "default_profile", "custom"),
        setup=setup_xdg_config,
        mutating=True,
    ),
    Case("config_set_no_args", ("config", "set")),
    Case("config_set_one_arg", ("config", "set", "key")),
    Case("config_set_three_args", ("config", "set", "key", "val", "extra")),
    Case("config_set_empty_key", ("config", "set", "", "val")),
    Case("config_set_new_string", ("config", "set", "default_profile", "custom"), mutating=True),
    Case("config_set_new_bool_true", ("config", "set", "audit.enabled", "true"), mutating=True),
    Case("config_set_new_bool_false", ("config", "set", "audit.enabled", "false"), mutating=True),
    Case("config_set_new_bool_1", ("config", "set", "audit.enabled", "1"), mutating=True),
    Case("config_set_new_bool_0", ("config", "set", "audit.enabled", "0"), mutating=True),
    Case("config_set_new_bool_t", ("config", "set", "audit.enabled", "t"), mutating=True),
    Case("config_set_new_bool_f", ("config", "set", "audit.enabled", "f"), mutating=True),
    Case("config_set_new_bool_T", ("config", "set", "audit.enabled", "T"), mutating=True),
    Case("config_set_new_bool_F", ("config", "set", "audit.enabled", "F"), mutating=True),
    Case("config_set_new_bool_TRUE", ("config", "set", "audit.enabled", "TRUE"), mutating=True),
    Case("config_set_new_bool_FALSE", ("config", "set", "audit.enabled", "FALSE"), mutating=True),
    Case("config_set_new_bool_True", ("config", "set", "audit.enabled", "True"), mutating=True),
    Case("config_set_new_bool_False", ("config", "set", "audit.enabled", "False"), mutating=True),
    Case("config_set_new_int", ("config", "set", "patterns.promotion_threshold", "42"), mutating=True),
    Case("config_set_new_int_neg", ("config", "set", "threshold", "-7"), mutating=True),
    Case("config_set_new_string_float", ("config", "set", "ratio", "4.5"), mutating=True),
    Case("config_set_new_nested", ("config", "set", "servers.vault.binary_path", "/opt/bin/symvault"), mutating=True),
    Case("config_set_new_deep_nested", ("config", "set", "a.b.c.d", "deep_value"), mutating=True),
    Case("config_set_existing_update", ("config", "set", "default_profile", "restricted"), setup=setup_conflict_config, mutating=True),
    Case("config_set_existing_parent_mode", ("config", "set", "new_key", "hello"), setup=setup_existing_parent_mode, mutating=True),
    Case("config_set_existing_sample", ("config", "set", "audit.verbose", "true"), setup=setup_xdg_config, mutating=True),
    Case("config_set_terminator", ("config", "--", "set", "default_profile", "custom"), setup=setup_xdg_config, mutating=True),
    Case("config_set_type_conflict", ("config", "set", "default_profile.sub", "val"), setup=setup_conflict_config),
    Case("config_set_malformed_existing", ("config", "set", "key", "value"), setup=setup_malformed_config, normalize_parse_error=True),
    Case("config_set_unreadable_file", ("config", "set", "key", "value"), setup=setup_unreadable_config, normalize_os_error=True),
    Case("config_unrelated_subcommand_fallback", ("config", "frobnicate"), setup=setup_xdg_config),
    Case("config_no_subcommand_fallback", ("config",), setup=setup_xdg_config),
    # Phase 1 Task 1.3: config set parity cases
    # Raw bytes (POSIX)
    Case("config_set_raw_ff_key", ("config", "set", b"raw_\xff", b"val_\x80"), setup=setup_xdg_config, mutating=True),
    Case("config_set_raw_latin1_key", ("config", "set", b"cafe\xe9", "ok"), setup=setup_xdg_config, mutating=True),
    Case("config_set_raw_dotted_key", ("config", "set", b"raw_sec.\xff_leaf", "42"), setup=setup_xdg_config, mutating=True),
    Case("config_set_raw_type_conflict", ("config", "set", b"default_profile.\xff", "bad"), setup=setup_xdg_config),
    # Inline & nested inline
    Case("config_set_inline_leaf", ("config", "set", "server.inline.port", "9090"), setup=setup_inline_config, mutating=True),
    Case("config_set_inline_new_key", ("config", "set", "server.inline.tls", "true"), setup=setup_inline_config, mutating=True),
    Case("config_set_nested_inline_leaf", ("config", "set", "nested.tree.mid.leaf", "updated"), setup=setup_inline_config, mutating=True),
    Case("config_set_nested_inline_new_key", ("config", "set", "nested.tree.mid.new_field", "bonus"), setup=setup_inline_config, mutating=True),
    Case("config_set_sample_nested_inline", ("config", "set", "nested_inline.sub.foo", "updated_foo"), setup=setup_xdg_config, mutating=True),
    # Floats
    Case("config_set_floats_preserve_existing", ("config", "set", "new_float_key", "1"), setup=setup_floats_config, mutating=True),
    Case("config_set_floats_update_neg_zero", ("config", "set", "neg_zero", "false"), setup=setup_floats_config, mutating=True),
    Case("config_set_floats_update_large", ("config", "set", "large", "99"), setup=setup_floats_config, mutating=True),
    # Datetime
    Case("config_set_datetime_preserve_existing", ("config", "set", "new_dt_flag", "true"), setup=setup_datetime_config, mutating=True),
    # Complex
    Case("config_set_complex_preserve", ("config", "set", "new_scalar", "added"), setup=setup_complex_config, mutating=True),
    Case("config_set_quoted_key_update", ("config", "set", "quoted key", "new_val"), setup=setup_complex_config, mutating=True),
    Case("config_set_array_of_tables_add_field", ("config", "set", "services.name", "override"), setup=setup_complex_config),
    Case(
        "config_set_symlink",
        ("config", "set", "new_key", "val"),
        setup=setup_symlink_config,
        mutating=True,
        posix_only=True,
    ),
    Case(
        "config_set_readonly_dir",
        ("config", "set", "new_key", "val"),
        setup=setup_readonly_config_dir,
        normalize_os_error=True,
        posix_only=True,
    ),
    # Phase 1 Task 1.4: native `skills status` OpenCode user-scope parity.
    Case("skills_status_opencode_root_missing", ("skills", "status", "--target", "opencode")),
    Case(
        "skills_status_opencode_root_empty",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_root_empty,
    ),
    Case(
        "skills_status_opencode_unmanaged_skill",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_unmanaged_skill,
    ),
    Case(
        "skills_status_opencode_dir_without_skill",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_dir_without_skill,
    ),
    Case(
        "skills_status_opencode_regular_file",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_regular_file,
    ),
    Case(
        "skills_status_opencode_dangling_symlink",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_dangling_symlink,
        posix_only=True,
    ),
    Case(
        "skills_status_opencode_symlink_to_dir",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_symlink_to_dir,
        posix_only=True,
    ),
    Case(
        "skills_status_opencode_symlink_to_file",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_symlink_to_file,
        posix_only=True,
    ),
    Case(
        "skills_status_opencode_symlink_to_marker_file",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_symlink_to_marker_file,
        posix_only=True,
    ),
    Case(
        "skills_status_opencode_symlink_chain",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_symlink_chain,
        posix_only=True,
    ),
    Case(
        "skills_status_opencode_orphaned_marker",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_orphaned_marker,
    ),
    Case(
        "skills_status_opencode_foreign_marker",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_foreign_marker,
    ),
    Case(
        "skills_status_opencode_legacy_hash_only_marker",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_legacy_hash_only_marker,
    ),
    Case(
        "skills_status_opencode_malformed_marker",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_malformed_marker,
    ),
    Case(
        "skills_status_opencode_unsupported_schema_marker",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_unsupported_schema_marker,
    ),
    Case(
        "skills_status_opencode_empty_marker",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_empty_marker,
    ),
    Case(
        "skills_status_opencode_wrong_type_marker",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_wrong_type_marker,
    ),
    Case(
        "skills_status_opencode_mixed",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_mixed,
    ),
    Case(
        "skills_status_opencode_mixed_json",
        ("skills", "status", "--target", "opencode", "--json"),
        setup=setup_skills_opencode_mixed,
    ),
    Case(
        "skills_status_opencode_managed_symlink",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_managed_symlink,
    ),
    Case(
        "skills_status_opencode_managed_symlink_json",
        ("skills", "status", "--target", "opencode", "--json"),
        setup=setup_skills_opencode_managed_symlink,
    ),
    Case(
        "skills_status_opencode_managed_copy_json",
        ("skills", "status", "--target", "opencode", "--json"),
        setup=setup_skills_opencode_managed_copy,
    ),
    Case(
        "skills_status_opencode_managed_harness_changed_json",
        ("skills", "status", "--target", "opencode", "--json"),
        setup=setup_skills_opencode_managed_harness_changed,
    ),
    Case(
        "skills_status_opencode_managed_conflict_json",
        ("skills", "status", "--target", "opencode", "--json"),
        setup=setup_skills_opencode_managed_conflict,
    ),
    Case(
        "skills_status_opencode_managed_conflict",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_managed_conflict,
    ),
    Case(
        "skills_status_opencode_managed_orphaned",
        ("skills", "status", "--target", "opencode"),
        setup=setup_skills_opencode_managed_orphaned,
    ),
    Case(
        "skills_status_default_dynamic_target_fallback",
        ("skills", "status"),
        setup=setup_skills_opencode_unmanaged_skill,
    ),
    Case(
        "skills_status_opencode_project_root_missing",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
    ),
    Case(
        "skills_status_opencode_project_unmanaged_skill",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=project_scope_fixture(setup_skills_opencode_unmanaged_skill),
    ),
    Case(
        "skills_status_opencode_project_dir_without_skill",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=project_scope_fixture(setup_skills_opencode_dir_without_skill),
    ),
    Case(
        "skills_status_opencode_project_regular_file",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=project_scope_fixture(setup_skills_opencode_regular_file),
    ),
    Case(
        "skills_status_opencode_project_mixed_json",
        ("skills", "status", "--target", "opencode", "--scope", "project", "--json"),
        setup=project_scope_fixture(setup_skills_opencode_mixed),
    ),
    Case(
        "skills_status_opencode_project_orphaned_marker",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=project_scope_fixture(setup_skills_opencode_orphaned_marker),
    ),
    Case(
        "skills_status_opencode_project_unsupported_schema_marker",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=project_scope_fixture(setup_skills_opencode_unsupported_schema_marker),
    ),
    Case(
        "skills_status_opencode_project_malformed_marker_json",
        ("skills", "status", "--target", "opencode", "--scope", "project", "--json"),
        setup=project_scope_fixture(setup_skills_opencode_malformed_marker),
    ),
    Case(
        "skills_status_opencode_project_symlink_to_file",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=setup_skills_opencode_project_symlink_to_file,
        posix_only=True,
    ),
    Case(
        "skills_status_opencode_project_symlink_chain",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=setup_skills_opencode_project_symlink_chain,
        posix_only=True,
    ),
    Case(
        "skills_status_opencode_project_isolated_from_user_root",
        ("skills", "status", "--target", "opencode", "--scope", "project"),
        setup=setup_skills_opencode_project_isolated_from_user_root,
    ),
    Case(
        "skills_status_unknown_scope_fallback",
        ("skills", "status", "--target", "opencode", "--scope", "team"),
        setup=setup_skills_opencode_unmanaged_skill,
    ),
    Case("skills_status_other_target_fallback", ("skills", "status", "--target", "claude")),
    Case(
        "skills_status_opencode_escapable_name_json",
        ("skills", "status", "--target", "opencode", "--json"),
        setup=setup_skills_opencode_escapable_name,
    ),
    # `memory` is native for the pinned shapes: the fixture pins the seeded
    # row's clock, the identifier and the query-log row, so both roots produce
    # the same bytes.
    Case("memory_list_fallback", ("memory", "list", "--json")),
    Case("memory_list_seeded", ("memory", "list", "--json"), setup=setup_memory_seeded),
    # `memory search` ranks embedding candidates. The query is the seeded row's
    # own content: the LSH neighbourhood of a different phrase does not contain
    # it, so a paraphrased query would compare two empty reports. Ranking is
    # reproducible because the fixture pins the row's clock to a fixed date -
    # the recency term of a freshly written row would drift between the two
    # roots.
    Case(
        "memory_search_seeded",
        ("memory", "search", "alpha memory content", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_seeded_table",
        ("memory", "search", "alpha memory content"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_seeded_scope",
        ("memory", "search", "alpha memory content", "-s", "global", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_seeded_other_scope",
        ("memory", "search", "alpha memory content", "-s", "project", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_seeded_limit",
        ("memory", "search", "alpha memory content", "-l", "1", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_seeded_limit_zero",
        ("memory", "search", "alpha memory content", "--limit", "0", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_two_candidates_limit",
        ("memory", "search", "alpha memory content", "--limit", "2", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_two_candidates_limit_one",
        ("memory", "search", "alpha memory content", "--limit", "1", "--json"),
        setup=setup_memory_seeded,
    ),
    # `--db` and `--limit` together are the shape whose values must be consumed
    # as flag values rather than counted as query arguments.
    Case(
        "memory_search_db_override",
        ("memory", "search", "alpha memory content", "--db", "MEMORY_DB", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_limit_and_db",
        (
            "memory",
            "search",
            "alpha memory content",
            "--limit",
            "1",
            "--db",
            "MEMORY_DB",
            "--json",
        ),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_no_candidate",
        ("memory", "search", "zzz", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_no_candidate_table",
        ("memory", "search", "zzz"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_search_empty_query",
        ("memory", "search", ""),
        setup=setup_memory_seeded,
    ),
    Case("memory_search_missing_query_fallback", ("memory", "search")),
    Case(
        "memory_search_unknown_flag_fallback",
        ("memory", "search", "alpha", "--bogus"),
    ),
    Case("memory_search_help_fallback", ("memory", "search", "--help")),
    Case(
        "memory_query_log_seeded",
        ("memory", "query-log", "--json"),
        setup=setup_memory_seeded,
    ),
    # `activity`: the dispatch text and the policy message are native, the
    # subcommands themselves stay on Go until their budget and page shapes are
    # ported.
    Case("activity_usage", ("activity",)),
    Case("activity_help", ("activity", "--help")),
    Case(
        "activity_missing_profile",
        ("activity", "status", "--profile", "absent", "--max-tokens", "100"),
    ),
    Case(
        "activity_get_missing_profile",
        ("activity", "get", "--profile", "absent", "entry-1", "--max-tokens", "100"),
    ),
    Case(
        "activity_search_missing_profile",
        (
            "activity",
            "search",
            "q",
            "--profile",
            "absent",
            "--from",
            "2026-01-01T00:00:00Z",
            "--to",
            "2026-01-02T00:00:00Z",
            "--limit",
            "5",
            "--max-tokens",
            "100",
        ),
    ),
    Case(
        "activity_status_reader_profile",
        ("activity", "status", "--profile", "reader", "--max-tokens", "100", "--json"),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_status_reader_profile_table",
        ("activity", "status", "--profile", "reader", "--max-tokens", "100"),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_get_missing_row",
        ("activity", "get", "--profile", "reader", "--max-tokens", "100", "--json", "missing-id"),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_get_missing_row_table",
        ("activity", "get", "--profile", "reader", "--max-tokens", "100", "missing-id"),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_search_reader_profile_table",
        (
            "activity",
            "search",
            "--profile",
            "reader",
            "--from",
            "2026-01-01T00:00:00Z",
            "--to",
            "2026-01-02T00:00:00Z",
            "--limit",
            "5",
            "--max-tokens",
            "100",
            "q",
        ),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_search_invalid_window_keeps_go",
        (
            "activity",
            "search",
            "q",
            "--profile",
            "reader",
            "--from",
            "not-a-time",
            "--to",
            "2026-01-02T00:00:00Z",
            "--limit",
            "5",
            "--max-tokens",
            "100",
        ),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_search_out_of_bounds_keeps_go",
        (
            "activity",
            "search",
            "q",
            "--profile",
            "reader",
            "--from",
            "2026-01-01T00:00:00Z",
            "--to",
            "2026-01-02T00:00:00Z",
            "--limit",
            "500",
            "--max-tokens",
            "100",
        ),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_get_identifier_before_flags_keeps_go",
        ("activity", "get", "missing-id", "--profile", "reader", "--max-tokens", "100"),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_search_reader_profile",
        (
            "activity",
            "search",
            "--profile",
            "reader",
            "--from",
            "2026-01-01T00:00:00Z",
            "--to",
            "2026-01-02T00:00:00Z",
            "--limit",
            "5",
            "--max-tokens",
            "100",
            "q",
        ),
        setup=setup_activity_profile,
    ),
    Case(
        "activity_unknown_subcommand",
        ("activity", "frobnicate", "--profile", "reader", "--max-tokens", "100"),
        setup=setup_activity_profile,
    ),
    # `usage` reports the credential state machine natively whenever no
    # credential is stored: the fixture home is empty, so every provider is in
    # its missing (or, for the Codex case, its stale-auth-file) state and no
    # endpoint is reached. Reports that do find a credential fetch from a live
    # endpoint and stay on the shipped implementation (see the CLI predicate).
    Case("usage_missing", ("usage",)),
    Case("usage_missing_json", ("usage", "--json")),
    Case("usage_codex_stale_auth_file", ("usage",), setup=setup_codex_stale_auth),
    Case(
        "usage_codex_stale_auth_file_json",
        ("usage", "--json"),
        setup=setup_codex_stale_auth,
    ),
    Case("activity_usage_fallback", ("activity",)),
    Case("activity_help_fallback", ("activity", "--help")),
    Case("activity_missing_profile_fallback", ("activity", "status", "--profile", "absent")),
    Case(
        "activity_get_fallback",
        ("activity", "get", "--profile", "absent", "entry-1"),
    ),
    # `audit tail` mirrors the shipped `os.PathError` text for a missing dir.
    Case("audit_tail_missing_directory", ("audit", "tail")),
    Case("audit_tail_missing_directory_json", ("audit", "tail", "--json")),
    Case(
        "memory_list_seeded_table",
        ("memory", "list"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_list_seeded_scope_alias",
        ("memory", "list", "-s", "global", "-l", "5", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_list_seeded_limit_zero",
        ("memory", "list", "--limit", "0", "--json"),
        setup=setup_memory_seeded,
    ),
    Case("memory_list_unknown_flag_fallback", ("memory", "list", "--bogus")),
    Case(
        "memory_set_native",
        ("memory", "set", "x", "--kind", "user", "--json"),
        normalize_uuid=True,
    ),
    Case(
        "memory_set_native_table",
        ("memory", "set", "y", "--kind", "project", "-s", "project"),
        normalize_uuid=True,
    ),
    Case(
        "memory_set_native_staged",
        ("memory", "set", "z", "--kind", "feedback", "--staged", "--json"),
        normalize_uuid=True,
    ),
    Case(
        "memory_set_missing_kind_fallback",
        ("memory", "set", "x", "--json"),
    ),
    Case(
        "memory_set_invalid_kind_fallback",
        ("memory", "set", "x", "--kind", "bogus", "--json"),
    ),
    Case(
        "memory_set_author_fallback",
        ("memory", "set", "x", "--kind", "user", "--author", "someone", "--json"),
        normalize_uuid=True,
    ),
    Case(
        "memory_delete_missing_row",
        ("memory", "delete", "missing-id"),
    ),
    Case(
        "memory_delete_seeded_json",
        ("memory", "delete", "00000000-0000-4000-8000-000000000001", "--json"),
        setup=setup_memory_seeded,
    ),
    Case(
        "memory_delete_seeded_table",
        ("memory", "delete", "00000000-0000-4000-8000-000000000001"),
        setup=setup_memory_seeded,
    ),
    Case("memory_delete_no_id_fallback", ("memory", "delete")),
    Case("memory_help_fallback", ("memory",)),
    Case("memory_rules_empty", ("memory", "rules", "--json")),
    Case("memory_rules_empty_table", ("memory", "rules")),
    Case(
        "memory_rules_seeded",
        ("memory", "rules", "--json"),
        setup=setup_memory_rules,
    ),
    Case(
        "memory_rules_seeded_table",
        ("memory", "rules"),
        setup=setup_memory_rules,
    ),
    Case(
        "memory_rules_seeded_scope",
        ("memory", "rules", "-s", "global", "--json"),
        setup=setup_memory_rules,
    ),
    Case(
        "memory_query_log_seeded_table",
        ("memory", "query-log"),
        setup=setup_memory_seeded,
    ),
    Case("memory_query_log_fallback", ("memory", "query-log", "--json")),
    # `sync` pushes instructions and skills to installed harnesses; it was
    # native without oracle coverage.
    Case("sync_dry_run_empty", ("sync", "--dry-run")),
    Case("sync_dry_run_all", ("sync", "--dry-run", "opencode")),
    Case(
        "sync_dry_run_with_library",
        ("sync", "--dry-run", "opencode"),
        setup=setup_skills_library_fixture,
    ),
    Case(
        "sync_installs_library_skill",
        ("sync", "opencode"),
        setup=setup_skills_library_managed,
    ),
    Case("sync_unknown_harness", ("sync", "nope")),
    Case("sync_help", ("sync", "--help")),
    Case("sync_unknown_flag", ("sync", "--bogus")),
    # `harness list`/`harness health` were native from the start but never
    # oracle-pinned; these cases hold them to the shipped bytes.
    Case("harness_list_empty", ("harness", "list")),
    Case("harness_list_empty_json", ("harness", "list", "--json")),
    Case(
        "harness_list_installed",
        ("harness", "list"),
        setup=setup_install_installed,
    ),
    Case(
        "harness_list_installed_json",
        ("harness", "list", "--json"),
        setup=setup_install_installed,
    ),
    Case(
        "harness_list_foreign_json",
        ("harness", "list", "--json"),
        setup=setup_install_foreign,
    ),
    Case(
        "harness_list_malformed_json",
        ("harness", "list", "--json"),
        setup=setup_install_malformed,
    ),
    Case("harness_list_unknown_flag", ("harness", "list", "--bogus")),
    Case("harness_unknown_subcommand", ("harness", "frobnicate")),
    Case("harness_health_empty_json", ("harness", "health", "--json")),
    Case(
        "harness_health_filter_json",
        ("harness", "health", "--harness", "cursor", "--json"),
    ),
    Case("harness_health_unknown_harness", ("harness", "health", "--harness", "nope")),
    # Phase 1 Task 1.5: `skills list` keeps only the empty-library slice native.
    Case("skills_list_empty_library", ("skills", "list")),
    Case("skills_list_empty_library_json", ("skills", "list", "--json")),
    Case(
        "skills_list_populated_library",
        ("skills", "list"),
        setup=setup_skills_library_fixture,
    ),
    Case(
        "skills_list_populated_library_json",
        ("skills", "list", "--json"),
        setup=setup_skills_library_fixture,
    ),
    Case(
        "skills_list_categories_json",
        ("skills", "list", "--json"),
        setup=setup_skills_library_categories,
    ),
    Case(
        "skills_list_managed_installs_json",
        ("skills", "list", "--json"),
        setup=setup_skills_library_managed,
        normalize_atime=True,
    ),
    Case(
        "skills_list_last_used_json",
        ("skills", "list", "--json"),
        setup=setup_skills_library_last_used,
        normalize_atime=True,
        require_last_used=True,
    ),
    Case(
        "skills_list_target_flag_is_ignored",
        ("skills", "list", "--target", "opencode", "--json"),
        setup=setup_skills_library_managed,
        normalize_atime=True,
    ),
    Case(
        "skills_list_broken_skill_fallback",
        ("skills", "list", "--json"),
        setup=setup_skills_library_broken_skill,
    ),
    Case(
        "skills_list_missing_skill_md_fallback",
        ("skills", "list", "--json"),
        setup=setup_skills_library_missing_skill_md,
    ),
    Case(
        "skills_list_unknown_flag_fallback",
        ("skills", "list", "--bogus"),
        setup=setup_skills_library_fixture,
    ),
    Case(
        "skills_list_dynamic_config_fallback",
        ("skills", "list", "--json"),
        setup=setup_skills_dynamic_config,
    ),
    Case(
        "skills_targets_dynamic_root_native",
        ("skills", "targets", "--json"),
        setup=setup_skills_opencode_unmanaged_skill,
    ),
    Case(
        "skills_targets_mixed_json",
        ("skills", "targets", "--json"),
        setup=setup_skills_targets_mixed,
    ),
    Case(
        "skills_targets_mixed_table",
        ("skills", "targets"),
        setup=setup_skills_targets_mixed,
    ),
    Case(
        "skills_targets_empty_root_json",
        ("skills", "targets", "--json"),
        setup=setup_skills_targets_empty_root,
    ),
    Case(
        "skills_targets_managed_only_json",
        ("skills", "targets", "--json"),
        setup=setup_skills_targets_managed_only,
    ),
    Case(
        "skills_targets_config_dir_evidence_json",
        ("skills", "targets", "--json"),
        setup=setup_skills_targets_config_dir_evidence,
    ),
    Case(
        "skills_targets_project_scope_json",
        ("skills", "targets", "--scope", "project", "--json"),
        setup=setup_skills_targets_project_scope,
    ),
    Case(
        "skills_targets_symlink_entry_json",
        ("skills", "targets", "--json"),
        setup=setup_skills_targets_symlink_entry,
        posix_only=True,
    ),
    Case(
        "skills_targets_unknown_scope_fallback",
        ("skills", "targets", "--scope", "team"),
        setup=setup_skills_targets_mixed,
    ),
    Case(
        "skills_targets_unknown_flag_fallback",
        ("skills", "targets", "--bogus"),
        setup=setup_skills_targets_mixed,
    ),
    Case(
        "skills_log_populated_fallback",
        ("skills", "log", "--json"),
        setup=setup_skills_event_log,
    ),
    Case(
        "skills_sync_dry_run_dynamic_config",
        ("skills", "sync", "--dry-run", "--json"),
        setup=setup_skills_dynamic_config,
    ),
    Case("skills_sync_dry_run_json", ("skills", "sync", "--dry-run", "--json")),
    Case(
        "skills_log_empty_json",
        ("skills", "log", "--json"),
    ),
    Case(
        "skills_targets_default_json",
        ("skills", "targets", "--json"),
    ),
    Case("skills_status_unknown_target", ("skills", "status", "--target", "bogus")),
    Case("skills_status_unknown_flag", ("skills", "status", "--bogus")),
)
def materialize_argv(argv: tuple[str | bytes, ...], root: Path) -> tuple[str | bytes, ...]:
    replacements = {
        "PROJECT": root / "project",
        "MEMORY_DB": root / "data/symbrain/memory/default.db",
    }
    return tuple(
        str(replacements[arg]) if isinstance(arg, str) and arg in replacements else arg
        for arg in argv
    )
def run(
    binary: Path,
    argv: tuple[str | bytes, ...],
    env: dict[str, str],
    stdin: bytes | None = None,
    use_pty: bool = False,
) -> subprocess.CompletedProcess[bytes]:
    if use_pty:
        master, slave = pty.openpty()
        try:
            process = subprocess.Popen(
                [str(binary), *argv],
                env=env,
                cwd=env.get("PROJECT"),
                stdin=slave,
                stdout=slave,
                stderr=slave,
            )
        finally:
            os.close(slave)
        chunks: list[bytes] = []
        while True:
            try:
                chunk = os.read(master, 4096)
            except OSError:
                break
            if not chunk:
                break
            chunks.append(chunk)
        os.close(master)
        return subprocess.CompletedProcess(
            [str(binary), *argv], process.wait(timeout=10), b"".join(chunks), b""
        )
    return subprocess.run(
        [str(binary), *argv],
        env=env,
        cwd=env.get("PROJECT"),
        input=stdin if stdin is not None else b"",
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=10,
        check=False,
    )
def format_access_time_ns(atime_ns: int) -> str:
    """Format an atime the way Go's `time.RFC3339Nano` does."""
    seconds, nanos = divmod(atime_ns, 1_000_000_000)
    base = time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(seconds))
    if nanos == 0:
        return f"{base}Z"
    return f"{base}.{nanos:09d}".rstrip("0") + "Z"


def reported_last_used(stdout_bytes: bytes) -> str | None:
    """Return the reported `last_used` string, or None when it is absent."""
    match = re.search(rb'"last_used":"([^"]*)"', stdout_bytes)
    return match.group(1).decode() if match else None


_RFC3339_UTC = re.compile(
    r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.\d+)?Z$"
)


def reported_last_used_seconds(value: str) -> int | None:
    """Parse a reported `last_used` into epoch seconds, or None if foreign."""
    match = _RFC3339_UTC.match(value)
    if not match:
        return None
    return calendar.timegm(time.strptime(match.group(1), "%Y-%m-%dT%H:%M:%S"))


def normalize_last_used(stdout_bytes: bytes, placeholder: str) -> bytes:
    """Replace an atime-derived `last_used` value with a fixed placeholder."""
    return re.sub(
        rb'("last_used":")([^"]*)(")',
        rb"\g<1>" + placeholder.encode() + rb"\g<3>",
        stdout_bytes,
    )


def prepare_root(root_path: Path, go_binary: Path) -> dict[str, str]:
    (root_path / "home").mkdir(parents=True, exist_ok=True)
    (root_path / "config").mkdir(parents=True, exist_ok=True)
    (root_path / "cache").mkdir(parents=True, exist_ok=True)
    (root_path / "data").mkdir(parents=True, exist_ok=True)
    (root_path / "state").mkdir(parents=True, exist_ok=True)
    (root_path / "project").mkdir(parents=True, exist_ok=True)
    env = {
        "HOME": str(root_path / "home"),
        "PATH": os.environ.get("PATH", ""),
        "LANG": "C.UTF-8",
        "LC_ALL": "C.UTF-8",
        "TZ": "UTC",
        "XDG_CACHE_HOME": str(root_path / "cache"),
        "XDG_CONFIG_HOME": str(root_path / "config"),
        "XDG_DATA_HOME": str(root_path / "data"),
        "XDG_STATE_HOME": str(root_path / "state"),
        "PROJECT": str(root_path / "project"),
        "SYMBRAIN_GO_BINARY": str(go_binary),
    }
    if os.name == "nt":
        # subprocess replaces the parent environment, so keep the Windows
        # runtime variables needed by CreateProcess and align the Go/Rust
        # home-directory contracts with the hermetic fixture root.
        env["USERPROFILE"] = env["HOME"]
        for key in ("SystemRoot", "windir", "ComSpec", "PATHEXT"):
            if value := os.environ.get(key):
                env[key] = value
    for key in ("TMPDIR", "TMP", "TEMP"):
        if value := os.environ.get(key):
            env[key] = value
    return env
def get_fs_manifest(root: Path) -> dict[str, tuple[str, int, bytes]]:
    manifest: dict[str, tuple[str, int, bytes]] = {}
    for p in sorted(root.rglob("*")):
        rel = p.relative_to(root).as_posix()
        rel = re.sub(r"\.bak\.\d{8}T\d{6}Z(?:\.\d+)?", ".bak.<timestamp>", rel)
        try:
            st = p.lstat()
        except OSError:
            continue
        mode = st.st_mode & 0o777
        if p.is_symlink():
            manifest[rel] = ("symlink", mode, os.readlink(p).encode())
        elif p.is_dir():
            manifest[rel] = ("dir", mode, b"")
        elif p.is_file():
            try:
                content = p.read_bytes()
            except OSError as err:
                content = f"<read error: {err}>".encode()
            manifest[rel] = ("file", mode, content)
    return manifest
def main() -> int:
    ensure_external_environment(__file__)
    global RELEASE_BASE_URL
    if len(sys.argv) != 3:
        print("usage: differential.py GO_BINARY RUST_BINARY", file=sys.stderr)
        return 2
    go_binary = Path(sys.argv[1]).resolve()
    rust_binary = Path(sys.argv[2]).resolve()
    fixture_server = ReleaseFixtureServer()
    RELEASE_BASE_URL = fixture_server.__enter__()
    failures: list[str] = []
    def windows_skip_reason(case: Case) -> str | None:
        if os.name != "nt":
            return None
        if case.windows_skip_reason:
            return case.windows_skip_reason
        if case.posix_only:
            return "POSIX-only filesystem/process behavior"
        if any(isinstance(arg, bytes) for arg in case.argv):
            return "raw-byte argv is not representable by Windows process APIs"
        return None

    skipped_cases = tuple((case.name, windows_skip_reason(case)) for case in CASES if windows_skip_reason(case))
    cases = tuple(case for case in CASES if not windows_skip_reason(case))
    if skipped_cases:
        print(f"SKIP {len(skipped_cases)} platform-specific parity cases:")
        for name, reason in skipped_cases:
            print(f"  {name}: {reason}")
    for case in cases:
        with tempfile.TemporaryDirectory(prefix="symbrain-parity-") as temp_dir:
            # macOS exposes /var as a symlink to /private/var. The native
            # capability walk intentionally rejects symlink ancestors, so
            # parity roots must use their canonical spelling on every OS.
            base = Path(temp_dir).resolve()
            go_root = base / "go"
            rust_root = base / "rust"
            go_root.mkdir()
            rust_root.mkdir()
            go_env = prepare_root(go_root, go_binary)
            rust_env = prepare_root(rust_root, go_binary)
            if case.env_overrides:
                go_env.update(case.env_overrides)
                rust_env.update(case.env_overrides)
            try:
                if case.setup:
                    case.setup(go_root, go_env)
                    case.setup(rust_root, rust_env)
                go_argv = materialize_argv(case.argv, go_root)
                rust_argv = materialize_argv(case.argv, rust_root)
                # For atime-sensitive cases: capture atime before each run.
                go_atime_captured = None
                rust_atime_captured = None
                if case.normalize_atime:
                    go_atime_captured = capture_access_time_ns(go_root, "demo")
                    rust_atime_captured = capture_access_time_ns(rust_root, "demo")
                go_result = run(go_binary, go_argv, go_env, case.stdin, case.pty)
                rust_result = run(rust_binary, rust_argv, rust_env, case.stdin, case.pty)
                go_stdout = go_result.stdout
                rust_stdout = rust_result.stdout
                go_stderr = go_result.stderr
                rust_stderr = rust_result.stderr
                # Normalize the isolated fixture roots in output paths
                go_stdout = go_stdout.replace(str(go_root).encode(), b"<root>")
                rust_stdout = rust_stdout.replace(str(rust_root).encode(), b"<root>")
                go_stderr = go_stderr.replace(str(go_root).encode(), b"<root>")
                rust_stderr = rust_stderr.replace(str(rust_root).encode(), b"<root>")
                backup_timestamp = rb"\.bak\.[0-9]{8}T[0-9]{6}Z(?:\.[0-9]+)?"
                go_stdout = re.sub(backup_timestamp, b".bak.<timestamp>", go_stdout)
                rust_stdout = re.sub(backup_timestamp, b".bak.<timestamp>", rust_stdout)
                go_stderr = re.sub(backup_timestamp, b".bak.<timestamp>", go_stderr)
                rust_stderr = re.sub(backup_timestamp, b".bak.<timestamp>", rust_stderr)
                # Download scratch roots are intentionally runtime-specific.
                download_temp = rb"[/\\][^\"\n]*?[/\\]symbrain-managed-[^/\\\"\n]+[/\\]"
                go_stdout = re.sub(download_temp, b"<download>/", go_stdout)
                rust_stdout = re.sub(download_temp, b"<download>/", rust_stdout)
                go_stderr = re.sub(download_temp, b"<download>/", go_stderr)
                rust_stderr = re.sub(download_temp, b"<download>/", rust_stderr)
                if case.normalize_runtime:
                    go_stdout = re.sub(rb"\n  go\s+[^\n]+", b"\n  <runtime>", go_stdout)
                    rust_stdout = re.sub(rb"\n  rust\s+[^\n]+", b"\n  <runtime>", rust_stdout)
                if case.normalize_parse_error:
                    install_parse = rb"(symbrain install: config: failed to load .*?global config error: failed to parse .*?\.toml: ).*"
                    go_stderr = re.sub(install_parse, rb"\1<parse error>\n", go_stderr)
                    rust_stderr = re.sub(install_parse, rb"\1<parse error>\n", rust_stderr)
                    go_stderr = re.sub(
                        rb"(symbrain config (?:get|set): parse [^:]+: )[\s\S]+",
                        rb"\1<parse error>\n",
                        go_stderr,
                    )
                    rust_stderr = re.sub(
                        rb"(symbrain config (?:get|set): parse [^:]+: )[\s\S]+",
                        rb"\1<parse error>\n",
                        rust_stderr,
                    )
                if case.normalize_uuid:
                    # Generated identifiers differ by construction; the case
                    # pins the response shape around them.
                    uuid_pattern = rb"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}"
                    go_stdout = re.sub(uuid_pattern, b"<uuid>", go_stdout)
                    rust_stdout = re.sub(uuid_pattern, b"<uuid>", rust_stdout)
                    go_stderr = re.sub(uuid_pattern, b"<uuid>", go_stderr)
                    rust_stderr = re.sub(uuid_pattern, b"<uuid>", rust_stderr)
                if case.normalize_os_error:
                    go_stderr = re.sub(
                        rb"(symbrain config (?:get|set): (?:read|parse|backup|write|create) [^:]+: ).*(?:[Pp]ermission denied).*",
                        rb"\1<permission denied>\n",
                        go_stderr,
                    )
                    rust_stderr = re.sub(
                        rb"(symbrain config (?:get|set): (?:read|parse|backup|write|create) [^:]+: ).*(?:[Pp]ermission denied).*",
                        rb"\1<permission denied>\n",
                        rust_stderr,
                    )
                if case.normalize_atime:
                    # `last_used` is the access time the runtime observes, so
                    # the harness reads it immediately before each runtime and
                    # requires that runtime to report exactly that value. The
                    # byte comparison then normalizes it, because an ambient
                    # reader can still bump the atime between the two runs.
                    for runtime, captured in (
                        ("Go", go_atime_captured),
                        ("Rust", rust_atime_captured),
                    ):
                        reported = (
                            reported_last_used(go_stdout if runtime == "Go" else rust_stdout)
                        )
                        # An ambient reader can only move the atime forward, so
                        # the runtime must report a value derived from the file's
                        # atime and never an older one. Absence is only allowed
                        # for the fixtures that carry no last-used evidence.
                        if reported is None:
                            assert not case.require_last_used, (
                                f"{case.name}: {runtime} reported no last_used for a "
                                f"fixture that pins atime={format_access_time_ns(captured)!r}"
                            )
                            continue
                        parsed = reported_last_used_seconds(reported)
                        assert parsed is not None, (
                            f"{case.name}: {runtime} reported a non-RFC3339 last_used "
                            f"{reported!r}"
                        )
                        assert captured is not None and parsed >= captured // 1_000_000_000, (
                            f"{case.name}: {runtime} reported last_used={reported!r}, "
                            f"older than the fixture atime "
                            f"{format_access_time_ns(captured) if captured is not None else 'n/a'!r}"
                        )
                    pinned = format_access_time_ns(SKILLS_LIBRARY_STAMP * 1_000_000_000)
                    go_stdout = normalize_last_used(go_stdout, pinned)
                    rust_stdout = normalize_last_used(rust_stdout, pinned)
                observed = (rust_result.returncode, rust_stdout, rust_stderr)
                expected = (go_result.returncode, go_stdout, go_stderr)
                if observed != expected:
                    failures.append(
                        f"{case.name}: Go={expected!r}, Rust={observed!r}"
                    )
                elif case.mutating:
                    go_manifest = get_fs_manifest(go_root)
                    rust_manifest = get_fs_manifest(rust_root)
                    if go_manifest != rust_manifest:
                        failures.append(
                            f"{case.name} filesystem mismatch: Go={go_manifest!r}, Rust={rust_manifest!r}"
                        )
                    else:
                        print(f"PASS {case.name} (stdout, stderr, exit code, fs manifest/modes)")
                else:
                    print(f"PASS {case.name}")
            finally:
                for base_root in (go_root, rust_root):
                    if base_root.exists():
                        for p in base_root.rglob("*"):
                            try:
                                p.chmod(0o700 if p.is_dir() else 0o600)
                            except OSError:
                                pass
    if failures:
        fixture_server.__exit__()
        print("\n".join(failures), file=sys.stderr)
        return 1
    fixture_server.__exit__()
    print(f"{len(cases)} parity cases passed")
    return 0
if __name__ == "__main__":
    raise SystemExit(main())
