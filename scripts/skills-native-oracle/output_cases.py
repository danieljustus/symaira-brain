"""Prepared actual owned stdout failure pairs; no runs during source checks."""
import os
import shutil
import subprocess
import time

from compare import filesystem, matched


def selected_sinks():
    return ("readonly", "full", "closed-reader") if os.name != "nt" else ("readonly",)


def required_ids():
    return [f"stdout-{verb}-{form}-{sink}" for verb in ("list", "status", "targets", "log", "sync", "doctor")
            for form in ("table", "json") for sink in selected_sinks()]


def invoke(binary, argv, env, root, sink, terminate, row):
    row.update(argv=[str(binary), *argv], cwd=str(root / "project"), stdout_sink=sink)
    options = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {"start_new_session": True}
    fd = None
    handle = None
    child = None
    start = time.time()
    try:
        if sink == "closed-reader":
            reader, fd = os.pipe()
            os.close(reader)
            stream = fd
        elif sink == "full":
            handle = open("/dev/full", "wb", buffering=0)
            stream = handle
        else:
            path = root / "tmp/owned-readonly-sink"
            path.write_bytes(b"owned read-only stdout sentinel")
            handle = path.open("rb")
            stream = handle
        child = subprocess.Popen(row["argv"], cwd=root / "project", env=env,
                                 stdin=subprocess.DEVNULL, stdout=stream, stderr=subprocess.PIPE, **options)
        row["pid"] = child.pid
        if fd is not None:
            os.close(fd)
            fd = None
        try:
            _, stderr = child.communicate(timeout=20)
        except BaseException:
            terminate(child, row)
            _, stderr = child.communicate(timeout=10)
            row.update(exit=child.returncode, stdout_hex="", stderr_hex=stderr.hex(), timed_out=True)
            raise
        row.update(exit=child.returncode, stdout_hex="", stderr_hex=stderr.hex())
        if sink == "readonly":
            assert path.read_bytes() == b"owned read-only stdout sentinel", "readonly sink was changed"
        if sink == "closed-reader":
            assert row["exit"] == -13 and not stderr, "real Go fd1 SIGPIPE completion required"
        else:
            assert row["exit"] == 1 and stderr, "real stdout write failure required"
    except Exception as error:
        row["invocation_error"] = {"type": type(error).__name__, "message": str(error)}
        raise
    finally:
        if child is not None and child.poll() is None:
            terminate(child, row)
        if fd is not None:
            os.close(fd)
        if handle is not None:
            handle.close()
        row.update(started=start, finished=time.time(), filesystem=filesystem(root, start, time.time()))
    return row


def run_pairs(report, output, binaries, root, retained, env, terminate):
    for verb in ("list", "status", "targets", "log", "sync", "doctor"):
        for form in ("table", "json"):
            argv = ["skills", verb, *(["--json"] if form == "json" else [])]
            for sink in selected_sinks():
                pair = {"id": f"stdout-{verb}-{form}-{sink}", "mcp": False, "matched": False}
                report["results"].append(pair)
                output()
                for flavor in ("go", "rust"):
                    shutil.rmtree(root)
                    shutil.copytree(retained, root, symlinks=True)
                    # Store the live row before assertions/process exceptions.
                    pair[flavor] = {}
                    try:
                        invoke(binaries[flavor], argv, env, root, sink, terminate, pair[flavor])
                    except Exception as error:
                        pair["execution_error"] = {"type": type(error).__name__, "message": str(error)}
                        raise
                    finally:
                        output()
                try:
                    pair["matched"] = matched(pair["go"], pair["rust"], root, False)
                except Exception as error:
                    pair["comparison_error"] = {"type": type(error).__name__, "message": str(error)}
                    raise
                output()
