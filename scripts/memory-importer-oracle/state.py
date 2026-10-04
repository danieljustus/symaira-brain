"""Byte/path/mode/mtime snapshots for the actual readonly process gate."""
import hashlib
import json
import os
import stat
import sys


def snapshot(root):
    root = os.fsencode(root)
    rows = []
    for parent, directories, files in os.walk(root,followlinks=False):
        for name in sorted(directories+files):
            path = parent+b"/"+name
            info = os.lstat(path)
            record = {"path_hex":os.path.relpath(path,root).hex(),"mode":stat.S_IMODE(info.st_mode),
                      "type":stat.S_IFMT(info.st_mode),"mtime_ns":info.st_mtime_ns}
            if stat.S_ISREG(info.st_mode):
                with open(path,"rb") as handle: record["sha256"] = hashlib.sha256(handle.read()).hexdigest()
            elif stat.S_ISLNK(info.st_mode): record["link_hex"] = os.readlink(path).hex()
            rows.append(record)
    return sorted(rows,key=lambda row: row["path_hex"])


if __name__ == "__main__":
    print(json.dumps(snapshot(sys.argv[1]),sort_keys=True))
