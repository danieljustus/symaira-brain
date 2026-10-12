"""Approved stat-only comparison for the owned demo restore's SHA-1 index.

Not a general Git parser: only v2 entries and an optional TREE extension are
supported. Unknown structures fail closed; original index bytes stay in capture.
"""
import hashlib
import struct

if not __debug__:
    raise RuntimeError("Native Skills evidence requires assertions; remove -O/PYTHONOPTIMIZE")

INDEX = "data/symbrain/skills/library/demo/.git/index"
# Git index-format stat(2) words. dev, mode, uid, gid and size remain exact.
VOLATILE = ("ctime_seconds", "ctime_nanoseconds", "mtime_seconds", "mtime_nanoseconds", "inode")
WORDS = ("ctime_seconds", "ctime_nanoseconds", "mtime_seconds", "mtime_nanoseconds",
         "device", "inode", "mode", "uid", "gid", "size")


def decode(raw):
    """Validate every byte; return a view differing only in approved stat words."""
    assert 32 <= len(raw) <= 1024 * 1024, "unsupported Git index size"
    assert hashlib.sha1(raw[:-20]).digest() == raw[-20:], "Git index checksum mismatch"
    signature, version, count = struct.unpack_from("!4sII", raw)
    assert signature == b"DIRC" and version == 2, "unsupported Git index format"
    assert 0 < count <= (len(raw) - 32) // 64, "invalid Git index entry count"
    position, entries, stat_rows = 12, [], []
    for _ in range(count):
        start = position
        assert position + 62 < len(raw) - 20, "truncated Git index entry"
        fields = dict(zip(WORDS, struct.unpack_from("!10I", raw, position)))
        oid = raw[position + 40:position + 60].hex()
        flags = struct.unpack_from("!H", raw, position + 60)[0]
        assert not flags & 0x4000, "unsupported extended Git index flags"
        end = raw.find(b"\0", position + 62, len(raw) - 20)
        assert end >= 0, "unterminated Git index path"
        name = raw[position + 62:end]
        assert name and len(name) == flags & 0x0fff, "unsupported Git index path length"
        assert not name.startswith(b"/") and all(part not in (b"", b".", b"..") for part in name.split(b"/")), "invalid Git index path"
        assert fields["mode"] in (0o100644, 0o100755, 0o120000, 0o160000), "unsupported Git index mode"
        assert fields["ctime_nanoseconds"] < 1_000_000_000 and fields["mtime_nanoseconds"] < 1_000_000_000, "invalid Git index timestamp"
        position = start + ((end + 1 - start + 7) // 8) * 8
        assert position <= len(raw) - 20 and not any(raw[end:position]), "invalid Git index padding"
        stat_rows.append({"path_hex": name.hex(), **{key: fields.pop(key) for key in VOLATILE}})
        entries.append({"path_hex": name.hex(), "object_id": oid, "flags": flags, **fields})
    order = [(bytes.fromhex(row["path_hex"]), (row["flags"] >> 12) & 3) for row in entries]
    assert order == sorted(set(order)), "unsorted or duplicate Git index entries"
    assert order == [(b"SKILL.md", 0), (b"notes.md", 0)], "unowned Git index entries"
    assert all(row["flags"] == 8 for row in entries), "unowned Git index flags"
    extension = raw[position:-20]
    if extension:
        assert len(extension) >= 8, "truncated Git index extension"
        name, size = struct.unpack_from("!4sI", extension)
        assert name == b"TREE" and size == len(extension) - 8, "unsupported Git index extension"
        assert size == 25 and extension[8:13] == b"\x002 0\n", "unsupported Git index cache tree"
    return {"version": version, "entries": entries, "extension_hex": extension.hex()}, stat_rows


def _raw(record):
    filesystem = record["filesystem"]
    row = filesystem["files"][INDEX]
    assert row["type"] == "file", "Git index is not a regular file"
    encoded = filesystem["raw_git_indexes"][INDEX]
    assert isinstance(encoded, str), "missing raw Git index bytes"
    raw = bytes.fromhex(encoded)
    assert encoded == raw.hex(), "noncanonical raw Git index hex"
    metadata = filesystem["raw_file_hashes"][INDEX]
    assert type(metadata["bytes"]) is int and metadata["bytes"] == len(raw), "Git index byte count mismatch"
    digest = hashlib.sha256(raw).hexdigest()
    assert metadata["sha256"] == digest and row["value"] == digest, "Git index raw digest mismatch"
    return raw


def project(pair, incoming):
    """Return copied views only for exact owned write requests; never erase raw evidence."""
    go, rust = pair["go"], pair["rust"]
    left = go["filesystem"]["files"].get(INDEX)
    right = rust["filesystem"]["files"].get(INDEX)
    names = {"mcp-018-skills_restore": None, "mcp-020-skills_restore": None,
             "mcp-archive-normal-False": "git-normal", "mcp-archive-warning-False": "git-warning"}
    if pair["id"] not in names:
        assert left == right, "unowned Git index difference"
        return go, rust, None
    assert pair["id"] in names and pair["mcp"] is True, "unowned Git index difference"
    assert pair.get("variant") == names[pair["id"]], "unowned Git index fixture variant"
    args = {"name": "demo", "rev": "HEAD~1", "dry_run": False}
    if pair["id"] == "mcp-020-skills_restore":
        args["sync"] = True
    expected = incoming("skills_restore", args)
    assert pair["stdin_hex"] == expected.hex(), "unowned Git index request"
    views, stats = [], []
    for record in (go, rust):
        assert record["stdin_sha256"] == hashlib.sha256(expected).hexdigest(), "Git index process input mismatch"
        value, stat = decode(_raw(record))
        files = dict(record["filesystem"]["files"])
        files[INDEX] = {**files[INDEX], "value": value}
        views.append({**record, "filesystem": {**record["filesystem"], "files": files}})
        stats.append(stat)
    assert views[0]["filesystem"]["files"][INDEX] == views[1]["filesystem"]["files"][INDEX], "nonvolatile Git index difference"
    return *views, {"path": INDEX, "approved_stat_fields": list(VOLATILE),
                    "go": stats[0], "rust": stats[1], "raw_preserved": True}
