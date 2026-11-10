"""DOC-HERMES / #1021: validate the doctor harness-line deviation.

The frozen Go doctor printed `config not found: ` with an empty path for the
harnesses without an MCP config (agents, hermes, openclaw) and could not see
Hermes. Rust reports `unsupported (no MCP config path)` for agents/openclaw and
the real `~/.hermes/config.yaml` for Hermes. Everything else stays byte-exact:
this projects only those lines (and fails if they are not the expected shape)
back to the frozen Go bytes.
"""
import re

_GO_HUMAN = b"config not found: \n"
_UNSUPPORTED = re.compile(rb"(  \xe2\x86\x92  (?:agents|openclaw) +)unsupported \(no MCP config path\)\n")
_HERMES_HUMAN = re.compile(rb"(  \xe2\x86\x92  hermes +config not found: )[^\n]*/\.hermes/config\.yaml\n")
_GO_JSON = b'"name":"hermes","config_path":"","config_found":false,"config_parsed":false,"config_error":"harness does not support MCP installation",'
_HERMES_JSON = re.compile(
    rb'"name":"hermes","config_path":"[^"]*/\.hermes/config\.yaml","config_found":false,"config_parsed":false,'
)


def legacy_doctor_view(go: bytes, rust: bytes) -> bytes:
    """Return `rust` with the accepted doctor harness lines mapped to Go's bytes."""
    if go.startswith(b"{"):
        if _GO_JSON not in go:
            return rust
        rust, count = _HERMES_JSON.subn(lambda _m: _GO_JSON, rust)
        assert count == 1, "hermes harness entry did not have the expected shape"
        return rust
    if _GO_HUMAN not in go:
        return rust
    rust, count = _UNSUPPORTED.subn(lambda m: m[1] + _GO_HUMAN, rust)
    assert count == 2, "agents/openclaw must report unsupported"
    rust, count = _HERMES_HUMAN.subn(lambda m: m[1] + b"\n", rust)
    assert count == 1, "hermes must report its config path"
    return rust
