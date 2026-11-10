"""DOC-HERMES / #1021: validate the doctor harness-line deviation.

The frozen Go doctor printed `config not found: ` with an empty path for the
harnesses without an MCP config (agents, hermes, openclaw) and could not see
Hermes. Rust reports `unsupported (no MCP config path)` for agents/openclaw and
the real `~/.hermes/config.yaml` for Hermes. Everything else stays byte-exact:
this projects only those lines (and fails if they are not the expected shape)
back to the frozen Go bytes.

#1024 adds two more read-only report sections that the frozen Go doctor never
printed: Hermes profile configs (`hermes_profiles`, `$HERMES_HOME/profiles/*/
config.yaml`) and a pending first-run backup (`pending_backups`, the
`.<app>-resume.partial` of #1020 plus how to finish it). Both are optional,
validated by shape, and stripped before the byte comparison, so any other
drift still fails.
"""
import json
import re

from contract_json import value_end

_GO_HUMAN = b"config not found: \n"
_UNSUPPORTED = re.compile(rb"(  \xe2\x86\x92  (?:agents|openclaw) +)unsupported \(no MCP config path\)\n")
_HERMES_HUMAN = re.compile(rb"(  \xe2\x86\x92  hermes +config not found: )[^\n]*/config\.yaml\n")
_GO_JSON = b'"name":"hermes","config_path":"","config_found":false,"config_parsed":false,"config_error":"harness does not support MCP installation",'
_HERMES_JSON = re.compile(
    rb'"name":"hermes","config_path":"[^"]*/config\.yaml","config_found":false,"config_parsed":false,'
)


_PROFILES_HUMAN = re.compile(rb"\n  hermes profiles:\n((?:    [^\n]*\n)+)")
_PROFILE_LINE = re.compile(rb"    \S+  hermes:[^\s:]\S* [^\n]*\n")
_BACKUP_HUMAN = re.compile(rb"\n  pending first-run backup:\n((?:    [^\n]*\n|       finish: [^\n]*\n)+)")
_BACKUP_LINE = re.compile(rb"    !  \S+ +\S*-resume\.partial \((?:running|paused)\)\n")
_FINISH_LINE = re.compile(rb"       finish: [^\n]+\n")
_BACKUP_KEYS = {"app", "partial_path", "running", "finish"}


def _strip_json_section(rust: bytes, key: bytes, check) -> bytes:
    head = b',"' + key + b'":['
    if head not in rust:
        return rust
    assert rust.count(head) == 1, f"{key!r} must appear once"
    start = rust.index(head)
    end = value_end(rust, start + len(head) - 1)
    check(json.loads(rust[start + len(head) - 1 : end]))
    return rust[:start] + rust[end:]


def _check_profiles(entries) -> None:
    assert entries, "hermes_profiles must not be empty"
    for entry in entries:
        assert entry["name"].startswith("hermes:"), "profile entries must be named hermes:<profile>"
        assert entry["config_path"].endswith("/config.yaml"), "profile config must be config.yaml"
        assert entry["supports_mcp_install"] is False, "hermes profiles are read-only"


def _check_backups(entries) -> None:
    assert entries, "pending_backups must not be empty"
    for entry in entries:
        assert set(entry) == _BACKUP_KEYS, f"unexpected pending backup keys: {sorted(entry)}"
        assert entry["partial_path"].endswith("-resume.partial"), "pending backup must be a resume.partial"
        assert isinstance(entry["running"], bool) and entry["finish"], "pending backup must say how to finish"


def _strip_human(rust: bytes, pattern, line_ok) -> bytes:
    found = list(pattern.finditer(rust))
    if not found:
        return rust
    assert len(found) == 1, "report section must appear once"
    for line in found[0].group(1).splitlines(keepends=True):
        assert line_ok(line), f"unexpected report line: {line!r}"
    return rust[: found[0].start()] + rust[found[0].end() :]


def _backup_ok(line: bytes) -> bool:
    return bool(_BACKUP_LINE.fullmatch(line) or _FINISH_LINE.fullmatch(line))


def _strip_new_sections(rust: bytes) -> bytes:
    if rust.startswith(b"{"):
        rust = _strip_json_section(rust, b"hermes_profiles", _check_profiles)
        return _strip_json_section(rust, b"pending_backups", _check_backups)
    rust = _strip_human(rust, _PROFILES_HUMAN, lambda line: bool(_PROFILE_LINE.fullmatch(line)))
    return _strip_human(rust, _BACKUP_HUMAN, _backup_ok)


def legacy_doctor_view(go: bytes, rust: bytes) -> bytes:
    """Return `rust` with the accepted doctor harness lines mapped to Go's bytes."""
    if b"hermes_profiles" not in go and b"pending_backups" not in go:
        rust = _strip_new_sections(rust)
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
