"""HAR-007 / #598: validate the three owner-approved health deviations.

Everything outside these fixtures remains byte-exact. Never strip arbitrary
fields: check the complete new contract before projecting its legacy view.
"""
import json
import math
import re

HEALTH_DEVIATIONS = frozenset({
    "harness_health_missing_command",
    "harness_health_missing_command_json",
    "harness_health_initialize_json",
})
_MISSING_COMMAND = (
    'discover: broker: "symaira-missing-mcp-fixture" not found on PATH or in '
    'managed directory: exec: "symaira-missing-mcp-fixture": executable file not found in '
)


def legacy_health_view(name: str, go: bytes, rust: bytes) -> bytes:
    assert name in HEALTH_DEVIATIONS
    if name == "harness_health_missing_command":
        # Only the redacted diagnostic changes, not the table's status/identity.
        match = re.fullmatch(
            rb'(  \xe2\x9c\x97  claude       missing        [^\n]+: )'
            + re.escape(_MISSING_COMMAND.encode()) + rb'(?:\$PATH|%PATH%)\n', go,
        )
        assert match is not None, "unexpected frozen Go missing-command diagnostic"
        assert rust == match[1] + b"server command was not found\n"
        return go
    old = json.loads(go)
    new = json.loads(rust)
    assert set(old) == {"servers"}
    assert set(new) == {"health_schema_version", "servers"}
    assert type(new["health_schema_version"]) is int and new["health_schema_version"] == 1
    assert len(old["servers"]) == len(new["servers"]) == 1
    before, after = old["servers"][0], new["servers"][0]
    is_missing = name == "harness_health_missing_command_json"
    assert before["server"] == ("missing" if is_missing else "probe")
    assert before["harness"] == "claude" and before["transport"] == "stdio"
    assert type(after["healthy"]) is bool and after["healthy"] == (not is_missing)
    assert after.pop("outcome") == ("unhealthy" if is_missing else "healthy")
    if is_missing:
        assert before["error"] in {_MISSING_COMMAND + "$PATH", _MISSING_COMMAND + "%PATH%"}
        assert after["error"] == "server command was not found"
        # No probe ran, so latency and method must not appear.
        after["error"] = before["error"]
    else:
        assert after.pop("probe_method") == "initialize+ping"
        latency = after.pop("latency_ms")
        assert type(latency) in (int, float) and math.isfinite(latency) and 0 <= latency < 7000
    assert after == before, "legacy health identity/status or unexpected fields changed"
    # Ensure this narrowly scoped projection preserves the exact legacy bytes.
    projected = json.dumps({"servers": [after]}, separators=(",", ":")).encode() + b"\n"
    assert projected == go
    return projected


# #769 / HAR-008: seeded `harness health` cases (a stdio server with a missing
# command, an HTTP server on a closed loopback port, and a project-local stdio
# server). Server-row identity, order and unhealthy status stay exact; only the
# per-server diagnostic follows the HAR-007 redaction. No-server output is
# compared byte for byte.
SEEDED_HEALTH_PREFIX = "cfgh-seeded-health-"
_HTTP_GO_ERROR = "not probed: http transport is not stdio"
_ROW = re.compile(r"  (\u2713|\u2717)  (\S+)\s+(\S+)\s+(\S.*)")


def _diagnostic_pair(go_error: str, rust_error: str) -> None:
    if go_error == _HTTP_GO_ERROR:
        assert rust_error == "MCP HTTP request failed", f"unexpected HTTP diagnostic {rust_error!r}"
        return
    assert go_error in {_MISSING_COMMAND + "$PATH", _MISSING_COMMAND + "%PATH%"}, (
        f"unexpected frozen Go diagnostic {go_error!r}")
    assert rust_error == "server command was not found", f"unexpected stdio diagnostic {rust_error!r}"


def _text_rows(data: bytes) -> list[tuple[str, str, str, str]]:
    assert data.endswith(b"\n"), "health table must end with a newline"
    rows = []
    for line in data.decode("utf-8").splitlines():
        match = _ROW.fullmatch(line)
        assert match is not None, f"not a server row: {line!r}"
        glyph, harness, server, tail = match.groups()
        config, separator, error = tail.partition(": ")
        assert glyph == "\u2717" and separator, f"seeded servers must all be unhealthy: {line!r}"
        rows.append((harness, server, config, error))
    return rows


def legacy_health_rows_view(name: str, go: bytes, rust: bytes) -> bytes:
    assert name.startswith(SEEDED_HEALTH_PREFIX)
    if go == rust:
        return go
    if go.startswith(b"{"):
        old, new = json.loads(go), json.loads(rust)
        assert set(old) == {"servers"} and set(new) == {"health_schema_version", "servers"}
        assert type(new["health_schema_version"]) is int and new["health_schema_version"] == 1
        assert old["servers"] and new["servers"] and len(old["servers"]) == len(new["servers"]), (
            "server rows differ in count")
        for before, after in zip(old["servers"], new["servers"]):
            assert after.pop("outcome") == "unhealthy"
            method = after.pop("probe_method", None)
            latency = after.pop("latency_ms", None)
            if before["transport"] == "http":
                assert method == "initialize"
                assert type(latency) in (int, float) and math.isfinite(latency) and 0 <= latency < 8000
            else:
                assert method is None and latency is None
            _diagnostic_pair(before["error"], after["error"])
            after["error"] = before["error"]
            assert after == before and before["healthy"] is False, "server row identity/status changed"
        return json.dumps(old, separators=(",", ":")).encode() + b"\n"
    old_rows, new_rows = _text_rows(go), _text_rows(rust)
    assert old_rows and len(old_rows) == len(new_rows), "server rows differ in count"
    for (h1, s1, c1, e1), (h2, s2, c2, e2) in zip(old_rows, new_rows):
        assert (h1, s1, c1) == (h2, s2, c2), f"server row identity changed: {(h1, s1, c1)!r} vs {(h2, s2, c2)!r}"
        _diagnostic_pair(e1, e2)
    return go
