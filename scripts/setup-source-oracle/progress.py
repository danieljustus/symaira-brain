"""Lossless checkpoints outside fixtures; never supplies comparison values."""
import base64
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
import time


class Journal:
    latest = None

    def __init__(self, report, selected, control, identities):
        self.path = Path(str(report) + ".progress.json")
        self.data = dict(format_version=1, status="running", control=control,
                         selected_cases=[case["name"] for case in selected],
                         identities=identities, events=[], complete_pairs=[],
                         limitations="Failure snapshots are diagnostic and may observe live files. "
                         "They never replace the unchanged exact comparison or assert cleanup.")
        self.write()
        Journal.latest = self

    def write(self):
        raw = (json.dumps(self.data, indent=2) + "\n").encode("utf-8")
        if len(raw) > 128 * 1024 * 1024:
            raise RuntimeError("owned source progress journal exceeds128MiB")
        self.path.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(dir=self.path.parent, delete=False) as stream:
            temporary = Path(stream.name)
            try:
                stream.write(raw)
                stream.flush()
                os.fsync(stream.fileno())
                stream.close()
                os.replace(temporary, self.path)
            finally:
                temporary.unlink(missing_ok=True)

    def event(self, phase, **values):
        self.data["events"].append(dict(phase=phase, recorded_unix=time.time(), **values))
        if len(self.data["events"]) > 1000:
            raise RuntimeError("owned source progress journal exceeds1000 events")
        self.write()

    def pair(self, observation):
        self.data["complete_pairs"].append(observation)
        self.write()

    def failed(self, error):
        self.data.update(status="failed", exception_type=type(error).__name__,
                         exception=str(error))
        self.write()

    def finish(self, report):
        self.data.update(status="complete", comparison_exit=report["exit"],
                         complete_observations=report["complete_observations"])
        self.write()


def raw(value):
    return None if value is None else base64.b64encode(value).decode("ascii")


def diagnostic_tree(root):
    """Bounded owned live-state inventory, distinct from contract filesystem()."""
    root = Path(root)
    rows = []
    errors = []
    captured = 0
    hashed = 0
    pending = [root]
    while pending and len(rows) < 4096:
        path = pending.pop()
        name = str(path.relative_to(root))
        try:
            metadata = path.lstat()
            row = dict(path=name, path_os_bytes_base64=raw(os.fsencode(name)),
                       mode=stat.S_IMODE(metadata.st_mode), length=metadata.st_size,
                       modified_ns=metadata.st_mtime_ns)
            if stat.S_ISLNK(metadata.st_mode):
                row.update(type="link", target=str(path.readlink()))
            elif stat.S_ISDIR(metadata.st_mode):
                row["type"] = "directory"
                pending.extend(sorted(path.iterdir(), reverse=True))
            elif stat.S_ISREG(metadata.st_mode):
                row["type"] = "file"
                digest = hashlib.sha256()
                remaining_hash = 128 * 1024 * 1024 - hashed
                file_hashed = 0
                with path.open("rb") as stream:
                    while remaining_hash and (chunk := stream.read(min(65536, remaining_hash))):
                        digest.update(chunk)
                        hashed += len(chunk)
                        file_hashed += len(chunk)
                        remaining_hash -= len(chunk)
                row["hashed_bytes"] = file_hashed
                row["sha256"] = digest.hexdigest()
                row["sha256_complete"] = file_hashed == metadata.st_size
                capture = path.name.startswith(".symbrain-source-capture-")
                if metadata.st_size < 8192 or capture:
                    remaining = min(16 * 1024 * 1024, 64 * 1024 * 1024 - captured)
                    with path.open("rb") as stream:
                        contents = stream.read(remaining + 1)
                    row["raw_bytes_complete"] = len(contents) <= remaining
                    contents = contents[:remaining]
                    row["raw_bytes_base64"] = raw(contents)
                    captured += len(contents)
                row["metadata_stable_during_read"] = metadata == path.lstat()
            else:
                row["type"] = "other"
            rows.append(row)
        except OSError as error:
            errors.append(dict(path=name, error=str(error), errno=error.errno))
    return dict(root=str(root), rows=rows, errors=errors,
                inventory_complete=not pending, retained_raw_bytes=captured,
                scope="Diagnostic owned live-state only; no cleanup/parity assertion.")
