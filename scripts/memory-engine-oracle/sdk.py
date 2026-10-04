"""Fixed whole-SDK ownership, including configuration, assembly and assets.

This module reads inputs only. It does not generate the trusted map, execute
an SDK, or allow a caller to widen its independently reviewed inventory.
"""
import hashlib
import os
from pathlib import Path
import stat


def sha(path):
    value = hashlib.sha256()
    with Path(path).open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def inventory(root):
    root = Path(root)
    if root.is_symlink() or not root.is_dir():
        raise ValueError("SDK root must be a regular directory")
    directories = {".": stat.S_IMODE(root.stat().st_mode)}
    files = {}

    def unreadable(error):
        # A partial enumeration must never become an admitted whole inventory.
        raise error

    for parent, dirs, names in os.walk(root, followlinks=False, onerror=unreadable):
        dirs.sort()
        names.sort()
        for name in dirs + names:
            path = Path(parent) / name
            relative = path.relative_to(root).as_posix()
            info = path.lstat()
            if stat.S_ISDIR(info.st_mode):
                directories[relative] = stat.S_IMODE(info.st_mode)
            elif stat.S_ISREG(info.st_mode):
                files[relative] = {"sha256": sha(path), "mode": stat.S_IMODE(info.st_mode)}
            else:
                raise ValueError("unbound SDK symlink/special input: " + relative)
    return {"files": files, "directories": directories}


def admit(root, role, expected):
    observed = inventory(root)
    for category in ("files", "directories"):
        if set(observed[category]) != set(expected[category]):
            extra = sorted(set(observed[category]) - set(expected[category]))
            missing = sorted(set(expected[category]) - set(observed[category]))
            raise ValueError("unbound SDK inventory: " + role + ":" + category +
                             ":extra=" + repr(extra) + ":missing=" + repr(missing))
        for name, value in observed[category].items():
            if value != expected[category][name]:
                raise ValueError("unbound SDK bytes/mode: " + role + ":" + name)
    return observed
