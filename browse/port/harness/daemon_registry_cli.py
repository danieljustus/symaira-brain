"""Literal argv/output observations for registry CLI boundaries; no transport."""
import base64
import copy
import os
import subprocess

UNSUPPORTED_ID_ROW = b"  id          Derive a stable, collision-free session id from the local repository layout\n"
COMMANDS = [("session", "list"), ("session", "info"), ("daemon", "status"), ("state", "list")]
FORMATS = [[], ["--json"], ["--output", "yaml"]]


def execute(binary, root, env, arguments):
    argv = arguments if os.name == "posix" else [value.decode() for value in arguments]
    result = subprocess.run([str(binary), *argv], cwd=root, env=env,
                            capture_output=True, timeout=15)
    return {"arguments_base64": [base64.b64encode(value).decode() for value in arguments],
            "exit": result.returncode, "stdout_base64": base64.b64encode(result.stdout).decode(),
            "stderr_base64": base64.b64encode(result.stderr).decode()}


def observe(binary, root, env):
    before = sorted(p.relative_to(root).as_posix() for p in root.rglob("*"))
    invalid = [b"bad\xffsession", b"bad\xe2\x82session", b"bad\xc0\xafsession"] if os.name == "posix" else []
    replacement = "bad\ufffdsession".encode()
    names = [b"", *invalid, replacement, b"../escape", b"_invalid", b"a" * 65,
             b"bad\nsession", b'bad\t"\\session']
    records = []
    for command in COMMANDS:
        group, verb = [value.encode() for value in command]
        for name in names:
            for output in FORMATS:
                records.append(execute(binary, root, env,
                    [group, verb, b"--session", name, *[value.encode() for value in output]]))
        for name in [*invalid, replacement]:
            for output in FORMATS:
                flags = [value.encode() for value in output]
                for arguments in (
                    [group, verb, b"--session=" + name, *flags],
                    [group, verb, b"--session", b"default", b"--session=" + name, *flags],
                    [group, verb, b"--session=" + name, b"--session", b"bad next", *flags],
                    [*flags, group, verb, b"--session", name],
                    [group, b"--session", name, verb, *flags],
                ):
                    records.append(execute(binary, root, env, arguments))
    # Append the exact12 independently observed JSON/invalid-output cases;
    # keep the existing360/144-case prefix unchanged.
    for command in COMMANDS:
        group, verb = [value.encode() for value in command]
        selected = [group, verb, b"--session", b"bad session"]
        for arguments in (
            [b"--json", b"--output", b"invalid", *selected],
            [*selected, b"--json", b"--output", b"invalid"],
            [*selected, b"--output", b"invalid", b"--json"],
        ):
            records.append(execute(binary, root, env, arguments))
    # Meaningful root-flag controls: inline forms, false overrides and later
    # output overrides must select the final format before its validation.
    for command in COMMANDS:
        group, verb = [value.encode() for value in command]
        selected = [group, verb, b"--session", b"bad session"]
        for arguments in (
            [b"--json=true", b"--output=invalid", *selected],
            [b"--json=false", b"--output=yaml", *selected],
            [b"--output=invalid", b"--output=yaml", *selected],
            [b"--output=invalid", *selected, b"--output=text"],
            [b"--json", *selected, b"--json=false", b"--output=yaml"],
        ):
            records.append(execute(binary, root, env, arguments))
    format_errors = [execute(binary, root, env,
        [*[value.encode() for value in command], b"--session", b"bad session",
         b"--output", b"invalid", b"--json=false"]) for command in COMMANDS]
    help_records = []
    for arguments in ([b"session", b"--help"], [b"session"], [b"session", b"list", b"--help"],
                      [b"session", b"info", b"--help"], [b"help", b"session", b"list"],
                      [b"help", b"session", b"info"]):
        help_records.append(execute(binary, root, env, arguments))
    assert sorted(p.relative_to(root).as_posix() for p in root.rglob("*")) == before, "CLI edges created daemon/profile/state files"
    return {"invalid": records, "format_errors": format_errors, "help": help_records, "no_files_created": True,
            "raw_unix_bytes": os.name == "posix"}


def comparable_help(observed, *, go):
    result = copy.deepcopy(observed)
    assert result["exit"] == 0 and result["stderr_base64"] == "", result
    if go and result["arguments_base64"] in (
        [base64.b64encode(b"session").decode()],
        [base64.b64encode(b"session").decode(), base64.b64encode(b"--help").decode()],
    ):
        text = base64.b64decode(result["stdout_base64"])
        # The native increment intentionally does not implement session id.
        # Verify and remove exactly its known advertisement, preserving every
        # other help byte, including descriptions, global flags and footer.
        assert text.count(UNSUPPORTED_ID_ROW) == 1, text
        result["stdout_base64"] = base64.b64encode(text.replace(UNSUPPORTED_ID_ROW, b"", 1)).decode()
    return result


def compare(left, right):
    assert left["no_files_created"] and right["no_files_created"]
    assert left["raw_unix_bytes"] == right["raw_unix_bytes"]
    assert len(left["invalid"]) == len(right["invalid"]) == (392 if left["raw_unix_bytes"] else 176)
    assert all(record["exit"] == 1 for record in left["invalid"] + right["invalid"])
    assert left["invalid"] == right["invalid"], "raw CLI args/exit/stdout/stderr differ"
    assert len(left["format_errors"]) == len(right["format_errors"]) == 4
    assert all(record["exit"] == 2 for record in left["format_errors"] + right["format_errors"])
    assert left["format_errors"] == right["format_errors"], "selected invalid output must still fail literally"
    assert len(left["help"]) == len(right["help"]) == 6
    assert [comparable_help(record, go=True) for record in left["help"]] == [
        comparable_help(record, go=False) for record in right["help"]], "implemented help bytes differ"
