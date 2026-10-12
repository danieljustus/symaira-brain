"""Genuine ENOSPC stdout prerequisites; never fill the host filesystem."""
from contextlib import contextmanager
import errno
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import uuid


@contextmanager
def full_stdout(root, proof):
    proof.update(platform=sys.platform, status="preparing", commands=[])
    if os.name == "nt":
        proof.update(status="not-required", required=False)
        yield None
        return
    stream = None
    image = None

    def command(argv):
        row = {"argv": argv}
        proof["commands"].append(row)
        result = subprocess.run(argv, capture_output=True, timeout=60)
        row.update(exit=result.returncode, stdout_hex=result.stdout.hex(), stderr_hex=result.stderr.hex())
        if result.returncode:
            raise RuntimeError("owned full-stdout fixture command failed")
        return result.stdout

    try:
        if sys.platform == "darwin":
            root.mkdir(mode=0o700)
            image = root / "owned-full.dmg"
            name = "skills-full-" + uuid.uuid4().hex
            proof.update(backend="owned-HFS+-image", image=str(image))
            command(["/usr/bin/hdiutil", "create", "-size", "10m", "-fs", "HFS+", "-type", "UDIF",
                     "-volname", name, "-nospotlight", str(image)])
            data = command(["/usr/bin/hdiutil", "attach", "-nobrowse", "-noautoopen", "-plist", str(image)])
            entities = plistlib.loads(data)["system-entities"]
            mounts = [item["mount-point"] for item in entities if "mount-point" in item]
            if mounts != ["/Volumes/" + name]:
                raise AssertionError("owned image mounted at an unexpected path")
            mount = Path(mounts[0])
            if not os.path.ismount(mount) or mount.stat().st_dev == root.stat().st_dev:
                raise AssertionError("refuse to fill a host filesystem")
            proof["image_bytes"] = image.stat().st_size
            if proof["image_bytes"] > 16 * 1024 * 1024:
                raise AssertionError("owned image exceeded its size bound")
            path = mount / "stdout"
            path.touch()
            total = 0
            with (mount / "filler").open("wb", buffering=0) as filler:
                for size in (65536, 512):
                    for _ in range(65536):
                        try:
                            total += filler.write(b"x" * size)
                        except OSError as error:
                            if error.errno != errno.ENOSPC:
                                raise
                            break
                        if total > 16 * 1024 * 1024:
                            raise AssertionError("owned fill exceeded its write bound")
                    else:
                        raise AssertionError("owned fill did not reach ENOSPC")
            proof["filled_bytes"] = total
            stream = path.open("wb", buffering=0)
        else:
            proof["backend"] = "/dev/full"
            stream = open("/dev/full", "wb", buffering=0)
        try:
            os.write(stream.fileno(), b"x")
        except OSError as error:
            if error.errno != errno.ENOSPC:
                raise
            proof.update(status="passed", required=True, actual_errno=error.errno)
        else:
            raise AssertionError("stdout is writable; no genuine full prerequisite")
        yield stream
    finally:
        if stream is not None:
            stream.close()
        if image is not None:
            # Also recover an attachment whose mount or command timed out.
            # Never detach a device unless its exact image identity is ours.
            info = plistlib.loads(subprocess.check_output(["/usr/bin/hdiutil", "info", "-plist"], timeout=30))
            owned = [item for item in info.get("images", []) if item.get("image-path") == str(image)]
            if len(owned) > 1:
                raise AssertionError("ambiguous owned image attachment")
            if owned:
                devices = owned[0]["system-entities"]
                device = next(item["dev-entry"] for item in devices if item.get("content-hint") == "GUID_partition_scheme")
                command(["/usr/bin/hdiutil", "detach", device])
                after = plistlib.loads(subprocess.check_output(["/usr/bin/hdiutil", "info", "-plist"], timeout=30))
                if any(item.get("image-path") == str(image) for item in after.get("images", [])):
                    raise AssertionError("owned image survived detach")
            proof["cleanup"] = "no-owned-image-attached"
