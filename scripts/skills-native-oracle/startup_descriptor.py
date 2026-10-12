"""Strict decoding of native self-relative Windows owner/DACL bytes."""
import re
import struct


def strict_hex(value):
    assert isinstance(value, str) and re.fullmatch(r"(?:[0-9a-f]{2})*", value), "invalid raw hex"
    return bytes.fromhex(value)


def descriptor_fields(value):
    data = strict_hex(value)
    assert len(data) >= 20, "truncated security descriptor"
    revision, reserved, flags, owner, group, sacl, dacl = struct.unpack_from("<BBHIIII", data)
    assert revision == 1 and reserved == 0 and flags & 0x8000, "invalid self-relative descriptor"
    assert all(offset == 0 or 20 <= offset < len(data) for offset in (owner, group, sacl, dacl)), "invalid descriptor offset"

    def sid(offset, limit):
        assert 0 < offset and offset + 8 <= limit, "truncated SID"
        revision, count = data[offset:offset + 2]
        end = offset + 8 + 4 * count
        assert revision == 1 and 1 <= count <= 15 and end <= limit, "invalid SID"
        authority = int.from_bytes(data[offset + 2:offset + 8], "big")
        subauthorities = struct.unpack_from(f"<{count}I", data, offset + 8)
        return "S-1-" + "-".join(str(part) for part in (authority, *subauthorities)), end

    owner_sid, _ = sid(owner, len(data))
    assert flags & 4 and dacl != 0 and dacl + 8 <= len(data), "missing/null DACL"
    revision, reserved, size, count, reserved2 = struct.unpack_from("<BBHHH", data, dacl)
    end = dacl + size
    assert revision in (2, 4) and reserved == reserved2 == 0 and 8 <= size and end <= len(data), "invalid DACL header"
    cursor = dacl + 8
    aces = []
    for _ in range(count):
        assert cursor + 8 <= end, "truncated ACE"
        kind, flags_ace, size_ace, mask = struct.unpack_from("<BBHi", data, cursor)
        assert kind in (0, 1) and size_ace >= 16 and cursor + size_ace <= end, "unreviewed/invalid ACE"
        identity, sid_end = sid(cursor + 8, cursor + size_ace)
        assert sid_end == cursor + size_ace, "unreviewed trailing ACE bytes"
        aces.append({"type": kind, "flags": flags_ace, "mask": mask, "sid": identity})
        cursor += size_ace
    # ACL allocation can contain unused bytes after the last ACE.
    assert cursor <= end, "DACL overflow"
    return {"owner_sid": owner_sid, "control_flags": flags, "dacl_present": True, "aces": aces}
