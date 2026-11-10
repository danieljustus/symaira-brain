"""HERMES-LIST / #1024: validate the `harness list` Hermes addition.

The frozen Go `harness list` never listed Hermes (it has no MCP install path).
Rust appends one read-only Hermes entry (global `config.yaml` plus any
`profiles/*/config.yaml`) as the LAST harness. Everything else stays
byte-exact: this strips only that entry, after checking its shape, and maps the
rest back to the frozen Go bytes. Any other drift still fails the comparison.
"""
import json
import re

from contract_json import value_end

_NOTE = b"  note\tread-only: symbrain install/uninstall do not support Hermes\n"
_BLOCK = re.compile(rb"(?m)^hermes\tHermes\n((?:  [^\n]*\n|    error: [^\n]*\n)+)\n")
_LINE = re.compile(
    rb"  (?:global|profile [^\t\n]+)\t[^\t\n]*config\.yaml\t"
    rb"(?:missing|parsed|unparsed|invalid)\tservers=[^\n]*\n"
)
_JSON_HEAD = b',{"name":"hermes","display_name":"Hermes",'
_ENTRY_KEYS = {"name", "display_name", "global", "profiles", "read_only"}


def _check_human(body: bytes) -> None:
    assert body.endswith(_NOTE), "hermes block must end with the read-only note"
    lines = body[: -len(_NOTE)].splitlines(keepends=True)
    assert lines and lines[0].startswith(b"  global\t"), "hermes block must start with global"
    for line in lines:
        if line.startswith(b"    error: "):
            continue
        assert _LINE.fullmatch(line), f"unexpected hermes line: {line!r}"


def _strip_json(rust: bytes) -> bytes:
    assert rust.count(_JSON_HEAD) == 1, "exactly one hermes entry expected"
    start = rust.index(_JSON_HEAD)
    end = value_end(rust, start + 1)
    assert rust[end : end + 1] == b"]", "hermes must be the last harness entry"
    entry = json.loads(rust[start + 1 : end])
    assert set(entry) <= _ENTRY_KEYS, f"unexpected hermes keys: {sorted(entry)}"
    assert entry["read_only"] is True, "hermes must be marked read_only"
    configs = [entry["global"]] + [profile["config"] for profile in entry.get("profiles", [])]
    assert all(c["path"].endswith("config.yaml") for c in configs), "hermes must list config.yaml files"
    return rust[:start] + rust[end:]


def _already_listed(go: bytes) -> bool:
    return b'"name":"hermes"' in go or go.startswith(b"hermes\tHermes\n") or b"\nhermes\tHermes\n" in go


def legacy_harness_list_view(go: bytes, rust: bytes) -> bytes:
    """Return `rust` without the accepted Hermes entry so it can match `go`."""
    if _already_listed(go):
        return rust
    if rust.startswith(b"{"):
        return _strip_json(rust) if _JSON_HEAD in rust else rust
    matches = list(_BLOCK.finditer(rust))
    if not matches:
        return rust
    assert len(matches) == 1, "exactly one hermes block expected"
    block = matches[0]
    assert block.end() == len(rust), "hermes must be the last harness block"
    _check_human(block.group(1))
    return rust[: block.start()]
