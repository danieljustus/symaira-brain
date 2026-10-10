"""Bounded native startup side-effect controls for approved target behavior."""
import json
import shutil

from fixtures import environment, remove_tree, write

MEMORY_KEY = "config/symbrain/memory/jwt.secret"
MEMORY_KEY_DIR = "config/symbrain/memory"
MEMORY_DIR = "data/symbrain/memory"
MEMORY_DB = f"{MEMORY_DIR}/default.db"
MEMORY_DB_FILES = {
    MEMORY_DB,
    MEMORY_DB + "-wal",
    MEMORY_DB + "-shm",
    MEMORY_DB + "-journal",
}
AUDIT_DIR = "data/symbrain/audit"
AUDIT_FILE = f"{AUDIT_DIR}/startup.jsonl"
HISTORY_DIR = "data/symbrain/recipes"
HISTORY_FILE = f"{HISTORY_DIR}/startup.jsonl"
AUDIT_FILES = {AUDIT_FILE}
HISTORY_FILES = {HISTORY_FILE}
OWNED_DIRS = {MEMORY_KEY_DIR, MEMORY_DIR, AUDIT_DIR, HISTORY_DIR}
OWNED_FILES = {MEMORY_KEY, *MEMORY_DB_FILES, *AUDIT_FILES, *HISTORY_FILES}

# `None` preserves the actual absent-config case; the frozen Go oracle defaults
# recording on. Rust intentionally remains non-recording for every case.
CASES = (
    ("profile-opt-out-history-absent", False, False, True, None),
    ("audit-and-history-explicit-opt-in", False, True, True, True),
    ("global-audit-veto", False, True, False, False),
    ("history-explicitly-disabled", False, False, False, False),
    ("memory-enabled-positive-control", True, False, False, False),
)


def _make_case(root, retained, git, case):
    if root.exists() or root.is_symlink():
        remove_tree(root)
    shutil.copytree(retained, root, symlinks=True)
    _, memory_enabled, profile_audit, global_audit, patterns = case
    profile = (
        '[profile]\nname = "startup"\n\n'
        f"[audit]\nenabled = {str(profile_audit).lower()}\n\n"
        "[servers.memory]\n"
        f"enabled = {str(memory_enabled).lower()}\nmode = \"read_only\"\n\n"
        "[servers.skills]\nenabled = true\n"
    )
    write(root / "profile.toml", profile)
    config = (
        "[audit]\n"
        f"enabled = {str(global_audit).lower()}\nverbose = false\n"
    )
    if patterns is not None:
        config += "\n[patterns]\n" f"enabled = {str(patterns).lower()}\n"
    write(root / "config/symbrain/config.toml", config)
    env = environment(root, git)
    env["SKILLS_ACTUAL_GIT"] = git
    return env


def _row(record, path, kind):
    row = record["filesystem"]["files"].get(path)
    assert row is not None and row["type"] == kind, f"expected {kind} at {path}"
    return row


def _private_file(record, path, *, exact_bytes=None):
    row = _row(record, path, "file")
    assert row["mode"] & 0o077 == 0, f"non-private startup file: {path}"
    raw = record["filesystem"]["raw_file_hashes"].get(path)
    assert raw is not None and len(raw["sha256"]) == 64, f"raw hash missing: {path}"
    assert raw["bytes"] > 0, f"empty startup file: {path}"
    if exact_bytes is not None:
        assert raw["bytes"] == exact_bytes, f"unexpected byte count for {path}"


def _private_dir(record, path):
    row = _row(record, path, "directory")
    assert row["mode"] & 0o077 == 0, f"non-private startup directory: {path}"


def _optional_file(record, path, expected):
    files = record["filesystem"]["files"]
    assert (path in files) == expected, f"unexpected {path}: expected present={expected}"
    if expected:
        _private_file(record, path)


def _classify(case, go, rust):
    _, memory_enabled, profile_audit, global_audit, patterns = case
    go_audit = profile_audit or global_audit  # frozen Go uses profile OR global
    rust_audit = profile_audit and global_audit  # target profile opt-out is final
    go_history = patterns is not False  # frozen Go defaults patterns.enabled=true

    _private_file(go, MEMORY_KEY, exact_bytes=65)
    _private_dir(go, MEMORY_KEY_DIR)
    _private_file(go, MEMORY_DB)
    _row(go, MEMORY_DIR, "directory")
    _optional_file(rust, MEMORY_KEY, False)
    assert MEMORY_KEY_DIR not in rust["filesystem"]["files"], "Rust created a legacy JWT directory"
    _optional_file(rust, MEMORY_DB, memory_enabled)
    if memory_enabled:
        _row(rust, MEMORY_DIR, "directory")
    else:
        assert MEMORY_DIR not in rust["filesystem"]["files"], "disabled Rust memory created its directory"

    _optional_file(go, AUDIT_FILE, go_audit)
    _optional_file(rust, AUDIT_FILE, rust_audit)
    if go_audit:
        _private_dir(go, AUDIT_DIR)
    else:
        assert AUDIT_DIR not in go["filesystem"]["files"]
    if rust_audit:
        _private_dir(rust, AUDIT_DIR)
    else:
        assert AUDIT_DIR not in rust["filesystem"]["files"], "disabled Rust audit created its directory"

    _optional_file(go, HISTORY_FILE, go_history)
    _optional_file(rust, HISTORY_FILE, False)
    if go_history:
        _private_dir(go, HISTORY_DIR)
    else:
        assert HISTORY_DIR not in go["filesystem"]["files"]
    assert HISTORY_DIR not in rust["filesystem"]["files"], "Rust startup recorded history"

    for record, owns_memory in ((go, True), (rust, memory_enabled)):
        memory_rows = {
            path for path in record["filesystem"]["files"]
            if path.startswith(MEMORY_DIR + "/")
        }
        assert memory_rows <= (MEMORY_DB_FILES if owns_memory else set()), memory_rows
        for path in memory_rows:
            _private_file(record, path)
    for record, owns_audit in ((go, go_audit), (rust, rust_audit)):
        audit_rows = {
            path for path in record["filesystem"]["files"]
            if path.startswith(AUDIT_DIR + "/")
        }
        assert audit_rows <= (AUDIT_FILES if owns_audit else set()), audit_rows
    for record, owns_history in ((go, go_history), (rust, False)):
        history_rows = {
            path for path in record["filesystem"]["files"]
            if path.startswith(HISTORY_DIR + "/")
        }
        assert history_rows <= (HISTORY_FILES if owns_history else set()), history_rows

    for path in OWNED_DIRS | OWNED_FILES:
        go_row = go["filesystem"]["files"].get(path)
        rust_row = rust["filesystem"]["files"].get(path)
        if go_row is not None and rust_row is not None:
            assert go_row["type"] == rust_row["type"], f"startup type mismatch: {path}"
            assert go_row["mode"] == rust_row["mode"], f"startup mode mismatch: {path}"

    allowed = OWNED_DIRS | OWNED_FILES
    go_unclassified = {
        path: value for path, value in go["filesystem"]["files"].items()
        if path not in allowed
    }
    rust_unclassified = {
        path: value for path, value in rust["filesystem"]["files"].items()
        if path not in allowed
    }
    assert go_unclassified == rust_unclassified, "unclassified startup filesystem difference"
    return {
        "frozen_go": {
            "memory_initialized_unconditionally": True,
            "audit_enabled_by_profile_or_global": go_audit,
            "history_recorded_by_absent_or_true_setting": go_history,
        },
        "rust_target": {
            "memory_initialized_by_profile": memory_enabled,
            "audit_enabled_by_profile_and_global": rust_audit,
            "history_recorded": False,
        },
        "owner_bound_paths": sorted(allowed),
        "unclassified_filesystem_equal": True,
        "raw_observations_preserved": True,
    }


def _mcp_responses(record):
    output = bytes.fromhex(record["stdout_hex"])
    responses = []
    if output.startswith(b"Content-Length:"):
        remaining = output
        while remaining:
            header, remaining = remaining.split(b"\r\n\r\n", 1)
            assert header.startswith(b"Content-Length: ")
            length = int(header.removeprefix(b"Content-Length: "))
            assert 0 < length <= len(remaining)
            responses.append(json.loads(remaining[:length]))
            remaining = remaining[length:]
    else:
        responses = [json.loads(line) for line in output.splitlines()]
    return responses


def complete_mcp_view(record):
    responses = _mcp_responses(record)
    ids = [response.get("id") for response in responses]
    assert ids == [1, 2, 3], f"startup responses missing or duplicated: {ids}"
    assert all(response.get("jsonrpc") == "2.0" for response in responses)
    tools = responses[1]["result"]["tools"]
    names = [tool["name"] for tool in tools]
    assert len(names) == len(set(names)), "startup tool catalog contains duplicates"
    skills = [tool for tool in tools if tool["name"].startswith("skills_")]
    assert len(skills) == 11
    call = responses[2]
    assert ("result" in call) != ("error" in call)
    result = call["result"]
    assert not result.get("isError", False)
    assert len(result["content"]) == 1 and result["content"][0]["type"] == "text"
    result["content"][0]["text"] = json.loads(result["content"][0]["text"])
    assert record["exit"] == 0
    return record["exit"], responses, bytes.fromhex(record["stderr_hex"])


def run_controls(report, save, binaries, owned, retained, git, invoke, incoming):
    controls = report.setdefault("startup_controls", [])
    for case in CASES:
        name, memory_enabled, profile_audit, global_audit, patterns = case
        row = {
            "id": name,
            "memory_enabled": memory_enabled,
            "profile_audit_enabled": profile_audit,
            "global_audit_enabled": global_audit,
            "patterns_config": "absent" if patterns is None else patterns,
            "matched": False,
        }
        controls.append(row)
        save()
        case_root = owned / ("startup-" + name)
        argv = ["mcp", "--profile-file", str(case_root / "profile.toml")]
        data = incoming("skills_list", {})
        for flavor in ("go", "rust"):
            env = _make_case(case_root, retained, str(git), case)
            record = {}
            row[flavor] = record
            try:
                invoke(binaries[flavor], argv, env, case_root, data, record=record)
            finally:
                record["expected_skills"] = 11
                save()
        assert complete_mcp_view(row["go"]) == complete_mcp_view(row["rust"]), "startup MCP response mismatch"
        row["classification"] = _classify(case, row["go"], row["rust"])
        row["matched"] = True
        save()
