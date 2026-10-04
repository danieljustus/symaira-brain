"""Fixed whole-SDK ownership with type/name admission before byte reads.

This module reads inputs only. It never generates or widens the trusted map.
"""
import hashlib
import os
from pathlib import Path
import stat


def sha(path, expected_mode=None):
    path = Path(path)
    before = path.lstat()
    if not stat.S_ISREG(before.st_mode):
        raise ValueError("unbound SDK symlink/special input: " + str(path))
    if expected_mode is not None and stat.S_IMODE(before.st_mode) != expected_mode:
        raise ValueError("unbound SDK bytes/mode changed: " + str(path))
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0)
    flags |= getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
    descriptor = os.open(path, flags)
    try:
        opened = os.fstat(descriptor)
        if not stat.S_ISREG(opened.st_mode) or (
                opened.st_dev, opened.st_ino, opened.st_mode) != (
                before.st_dev, before.st_ino, before.st_mode):
            raise ValueError("unbound SDK file identity/type changed: " + str(path))
        value = hashlib.sha256()
        # Hash only the checked descriptor. POSIX nonblocking/no-follow open
        # also refuses a FIFO or leaf alias substituted after lstat.
        while True:
            chunk = os.read(descriptor, 1024 * 1024)
            if not chunk:
                break
            value.update(chunk)
        return value.hexdigest()
    finally:
        os.close(descriptor)


def census(root):
    root = Path(root)
    if root.is_symlink() or not root.is_dir():
        raise ValueError("SDK root must be a regular directory")
    directories = {".": stat.S_IMODE(root.stat().st_mode)}
    files = {}

    def unreadable(error):
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
                files[relative] = {"mode": stat.S_IMODE(info.st_mode)}
            else:
                raise ValueError("unbound SDK symlink/special input: " + relative)
    return {"files": files, "directories": directories}


def check_shape(observed, role, expected):
    for category in ("files", "directories"):
        if set(observed[category]) != set(expected[category]):
            extra = sorted(set(observed[category]) - set(expected[category]))
            missing = sorted(set(expected[category]) - set(observed[category]))
            raise ValueError("unbound SDK inventory: " + role + ":" + category +
                             ":extra=" + repr(extra) + ":missing=" + repr(missing))
        for name, value in observed[category].items():
            desired = expected[category][name]
            if category == "files":
                value, desired = value["mode"], desired["mode"]
            if value != desired:
                raise ValueError("unbound SDK bytes/mode: " + role + ":" + name)


def hashed(root, shape):
    return {"directories": shape["directories"], "files": {
        name: {"mode": value["mode"], "sha256": sha(Path(root) / name, expected_mode=value["mode"])}
        for name, value in shape["files"].items()}}


def inventory(root):
    # Inventory preparation has no expected map. Even here no byte read occurs
    # until all physical inputs pass the no-follow regular-file census.
    return hashed(root, census(root))


def admit(root, role, expected, shape=None):
    shape = census(root) if shape is None else shape
    check_shape(shape, role, expected)
    observed = hashed(root, shape)
    for name, value in observed["files"].items():
        if value != expected["files"][name]:
            raise ValueError("unbound SDK bytes/mode: " + role + ":" + name)
    return observed
