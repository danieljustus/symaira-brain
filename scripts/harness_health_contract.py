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
