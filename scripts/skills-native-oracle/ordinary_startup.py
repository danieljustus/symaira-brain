"""#834 startup assertions for the exact ordinary Skills MCP fixture only."""
import hashlib
import json
from contextlib import contextmanager
from contextvars import ContextVar
from pathlib import Path
import sys

sys.path.append(str(Path(__file__).resolve().parents[1] / "skills-argv-oracle"))
from skills_reference import FROZEN as FROZEN_GO, HISTORICAL, startup_identity

from compare import matched as strict_matched
from fixtures import PROFILE
from startup_controls import _classify, _mcp_responses

PROFILES = (PROFILE, PROFILE.replace('[servers.skills]\nenabled = true', '[servers.skills]\nenabled = false'))
BINDINGS = [{"frozen_go": FROZEN_GO, "profile_sha256": hashlib.sha256(profile.encode()).hexdigest(),
             "global_config": "absent", "audit_override": "absent", "patterns_override": "absent"}
            for profile in PROFILES]
_bindings = ContextVar("skills_startup_bindings", default=BINDINGS)


@contextmanager
def using_reference(identity):
    # The actual runner supplies the admitted receipt identity. Do not widen
    # profile/config/owner permissions when selecting the corrected reference.
    expected = startup_identity(identity.get("reference", HISTORICAL), identity.get("oracle_receipt_sha256"))
    if identity != expected:
        raise ValueError("wrong ordinary startup reference identity")
    token = _bindings.set([{**binding, **identity} for binding in BINDINGS])
    try:
        yield
    finally:
        _bindings.reset(token)


def bind(root, env):
    profile = (root / "profile.toml").read_bytes()
    assert profile in [value.encode() for value in PROFILES], "unowned ordinary MCP profile"
    assert not (root / "config/symbrain/config.toml").exists(), "ordinary MCP global config changed"
    assert not any(key.startswith("SYMBRAIN_") and key != "SYMBRAIN_GO_BINARY" for key in env), "unowned startup override"
    return dict(_bindings.get()[[value.encode() for value in PROFILES].index(profile)])


def protocol(record):
    """All responses/catalog fields retained, including non-Skills tools."""
    responses = _mcp_responses(record)
    assert [row.get("id") for row in responses] in ([1, 2, 3], [1, 2, None])
    assert all(row.get("jsonrpc") == "2.0" for row in responses)
    catalog = responses[1]["result"]["tools"]
    names = [tool["name"] for tool in catalog]
    assert len(names) == len(set(names)), "duplicate full MCP catalog"
    expected = record.get("expected_skills", 11)
    assert expected in (0, 11) and len([name for name in names if name.startswith("skills_")]) == expected
    reply = responses[2]
    assert ("result" in reply) != ("error" in reply)
    if "result" in reply:
        result = reply["result"]
        assert len(result["content"]) == 1 and result["content"][0]["type"] == "text"
        if not result.get("isError", False):
            result["content"][0]["text"] = json.loads(result["content"][0]["text"])
    # Canonical JSON preserves boolean/number type distinctions in comparisons.
    return record["exit"], json.dumps(responses, sort_keys=True, ensure_ascii=True), record["stderr_hex"]


def pair(go, rust):
    """Validate raw startup deltas before returning non-mutating compare views."""
    binding = go.get("startup_input")
    assert binding in _bindings.get() and binding == rust.get("startup_input"), "startup input binding missing/changed"
    replies = _mcp_responses(go)
    called = "result" in replies[2]  # the frozen wrapped handler records even tool errors
    classification = _classify(("ordinary", False, False, True, None), go, rust,
                               profile="skills-native-764", called=called, absent_config=True)
    allowed = set(classification["owner_bound_paths"])
    projected = []
    for record in (go, rust):
        view = dict(record)
        view["filesystem"] = {field: {path: row for path, row in rows.items() if path not in allowed}
                              for field, rows in record["filesystem"].items()}
        projected.append(view)
    return (*projected, classification)


def matched(go, rust, root, mcp):
    if not mcp:
        return strict_matched(go, rust, root, False)
    left, right, _ = pair(go, rust)
    return protocol(go) == protocol(rust) and strict_matched(left, right, root, True)


def assert_control_bootstrap(go, mutant):
    # Input controls intentionally change only the third response. Startup,
    # complete catalog, exit and stderr must not become a second difference.
    left, right = protocol(go), protocol(mutant)
    assert left[0] == right[0] and left[2] == right[2], "control process mismatch"
    assert (json.dumps(json.loads(left[1])[:2], sort_keys=True)
            == json.dumps(json.loads(right[1])[:2], sort_keys=True)), "control full bootstrap/catalog mismatch"
