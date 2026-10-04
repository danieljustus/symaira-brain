"""Pinned, read-only Git owner queries without ambient configuration."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

HERE = Path(__file__).resolve().parent


def query(checkout, *arguments, input=None, text=False):
    plan = json.loads((HERE / "dependency-inputs.json").read_bytes())["source_reader_git"]
    executable = Path(plan["path"])
    if hashlib.sha256(executable.read_bytes()).hexdigest() != plan["sha256"]:
        raise ValueError("source-reader Git bytes differ from reviewed owner")
    empty = "NUL" if os.name == "nt" else "/dev/null"
    env = {key: os.environ[key] for key in ("SystemRoot", "SYSTEMROOT", "WINDIR") if key in os.environ}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=empty, GIT_CONFIG_SYSTEM=empty)
    argv = [str(executable), "-c", "core.fsmonitor=false", "-c", "core.hooksPath=" + empty, *arguments]
    return subprocess.check_output(argv, cwd=checkout, env=env, input=input, text=text)


def blob_map(checkout, revision):
    names = query(checkout, "ls-tree", "-r", "--name-only", revision, text=True).splitlines()
    packed = query(checkout, "cat-file", "--batch", input="".join(revision + ":" + name + "\n" for name in names).encode())
    offset, result = 0, {}
    for name in names:
        end = packed.index(b"\n", offset)
        header = packed[offset:end].split()
        if len(header) != 3 or header[1] != b"blob":
            raise ValueError("invalid dependency Git blob")
        size = int(header[2])
        body = packed[end + 1:end + 1 + size]
        offset = end + 1 + size
        if packed[offset:offset + 1] != b"\n" or len(body) != size:
            raise ValueError("invalid dependency Git blob boundary")
        offset += 1
        result[name] = hashlib.sha256(body).hexdigest()
    if offset != len(packed):
        raise ValueError("unexpected dependency Git records")
    return result
