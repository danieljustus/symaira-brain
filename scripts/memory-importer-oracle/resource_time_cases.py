"""Six PREPARED public-constructor boundaries; no expected report synthesis."""
import os
from pathlib import Path
import fixtures

BODY = b"Owned context records one retained decision without credentials.\n"
DIRECTORY = b"extensions/owned/resources/"
CHILD = b"2026-01-02T00-00-00-owned-10min-child.md"


def recipes():
    return [("invalid-6h", b"60", b"6h", False),
            ("invalid-10min", b"60", b"10min", False),
            ("valid-6h", b"59", b"6h", False),
            ("valid-10min", b"59", b"10min", False),
            ("invalid-parent-with-valid-child", b"60", b"6h", True),
            ("valid-parent-with-valid-child", b"59", b"6h", True)]


def append(destination, packets):
    result = list(packets)
    for number, (name, second, kind, child) in enumerate(recipes(), 1):
        home = destination / ("resource-time-"+name)
        root = home/".codex/memories"
        folder = os.fsencode(root)+b"/"+DIRECTORY
        os.makedirs(folder)
        base = b"2026-01-01T23-59-"+second+b"-owned-"+kind+b"-context.md"
        for filename in [base]+([CHILD] if child else []):
            path = folder+filename
            with open(path, "wb") as handle: handle.write(BODY)
            os.utime(path, (fixtures.STAMP, fixtures.STAMP))
        result.append({"Id":f"I761-L{number:02d}-codex-memory-{name}","Family":"codex-memory",
                       "RootHex":os.fsencode(root).hex(),"Since":"2026-08-28T10:00:00Z"})
    assert len(result) == len(packets)+6
    return result


def original_boundaries():
    return {"invalid-6h":"retain consolidated discovery/direct-import; no timed metadata",
            "invalid-10min":"retain consolidated discovery/direct-import; no timed metadata",
            "valid-6h":"preserve ordinary six-hour metadata",
            "valid-10min":"preserve ordinary ten-minute metadata",
            "invalid-parent-with-valid-child":"retain invalid document and valid child; no invalid-parent coverage",
            "valid-parent-with-valid-child":"retain valid parent; preserve coverage suppression of child"}
