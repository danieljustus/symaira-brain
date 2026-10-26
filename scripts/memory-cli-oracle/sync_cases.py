"""`memory sync` argument checks that precede database and network work.

Refusal scenarios (flag grammar, positional refusal, missing remote, relay
passphrase, durations, the remote URL guard) run the real Go CLI and the native
CLI in identical fresh disposable roots with the Go fallback path absent and
compare stdout, stderr, exit status and the created filesystem tree.

Gate scenarios name a remote that the shipped implementation accepts (or a URL
spelling outside the ported `net/url` subset) and that the native client does
not run itself (https, `localhost`, other loopback addresses, URL spellings
outside the ported subset, the encrypted relay). They are not compared byte for
byte: Go is run against a closed loopback port and must get *past* validation,
while the native process with the Go fallback absent must stop at the owned
absent-binary diagnostic (exit 1, empty stdout, OS error 2) without creating
anything. Plain `http://127.0.0.1:<port>` remotes run natively and are
compared against Go by the `sync_relay` family (`sync_relay_cases.py`).
"""
import os
from pathlib import Path
import tempfile

POSIX = os.name == "posix"
CLOSED = "127.0.0.1:1"
RELAY_ENV = "SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE"

REFUSALS = [
    # argument grammar
    [], ["-h"], ["--help"], ["-help"], ["-h=x"], ["--pull"], ["--push"], ["--undefined"],
    ["----wrong"], ["-=bad"], ["---"], ["--"], ["--", "stray"], ["stray"], ["stray", "--pull"],
    ["--remote"], ["--remote="], ["--remote", ""], ["--token"], ["--db"], ["--timeout"],
    ["--pull=maybe"], ["--pull=false"], ["--pull", "false"], ["--pull=TRUE", "--push=0"],
    ["--encrypted-relay=2"], ["--allow-insecure-http="], ["--remote", f"https://{CLOSED}", "stray"],
    ["--remote", f"https://{CLOSED}", "--", "--pull"], ["--remote", f"https://{CLOSED}", "--undefined"],
    ["--remote=https://example.test", "--pull", "--push", "--bad"], ["-remote"], ["-remote="],
    ["--encrypted-relay"], ["--allow-insecure-http"], ["--token", "t", "--db", "d"],
    # relay passphrase precedes the URL guard
    ["--remote", f"https://{CLOSED}", "--encrypted-relay"],
    ["--remote", "ftp://example.test", "--encrypted-relay"],
    ["--remote", f"https://{CLOSED}", "--encrypted-relay", "--relay-passphrase="],
    ["--remote", f"https://{CLOSED}", "--encrypted-relay=false", "--allow-insecure-http=x"],
    # remote URL guard
    ["--remote", "ftp://example.test"], ["--remote", "FTP://example.test"],
    ["--remote", "http://example.test"], ["--remote", "http://example.test:8080/path?x=1#frag"],
    ["--remote", "http://"], ["--remote", "http:///x"], ["--remote", "http:foo"], ["--remote", "http:"],
    ["--remote", "http://:80"], ["--remote", "http://h:x"], ["--remote", "https://h:x"],
    ["--remote", "https://h:8a"], ["--remote", "https://h:80:x"], ["--remote", "http://h:"],
    ["--remote", "example.test"], ["--remote", "example.test:8080"], ["--remote", "127.0.0.1:8080"],
    ["--remote", "localhost:8080"], ["--remote", ":foo"], ["--remote", "/path"], ["--remote", "//host/path"],
    ["--remote", "///x"], ["--remote", "1http://x"], ["--remote", "mailto:a"], ["--remote", "ws://x"],
    ["--remote", "http://127.0.0.1.1"], ["--remote", "http://127.0.0.01"], ["--remote", "http://127.0.0.256"],
    ["--remote", "http://0.0.0.0"], ["--remote", "http://localhost."], ["--remote", "http://localhost.example"],
    ["--remote", "http://example.test#x"], ["--remote", "http://example.test?x"], ["--remote", "https://h:x#frag"],
    ["--remote", "a?b:c"], ["--remote", "a/b:c"], ["--remote", "a:b/c"], ["--remote", "+x://h"],
    ["--remote", "x+y://h"], ["--remote", "-x://h"], ["--remote", "*"], ["--remote", "#"], ["--remote", "?"],
    ["--remote", "http://example.test", "--allow-insecure-http=false"],
    ["--remote", "http://", "--allow-insecure-http"], ["--remote", "http://:8080", "--allow-insecure-http"],
    ["--remote=http://example.test", "--pull", "--timeout=1m"],
    ["--remote", "ftp://h", "--allow-insecure-http"],
]
for duration in ["0", "-0", "+0", "1s", "1.5h", "1h30m", ".5s", "5.s", "100ms", "1us", "1\u00b5s", "1\u03bcs",
                 "-1s", "+1s", "9223372036s", "9223372037s", "2562047h47m16.854775807s",
                 "2562047h47m16.854775808s", "-2562047h47m16.854775808s", "99999999999999999999s",
                 "", "-", "s", "1", "1x", "1d", "1 s", "1s1", "1.s.", "..s", "1\u00ff", "0.0.1s",
                 "9223372036.854775807s", "9223372036.854775808s", "1e3s", "0x10s", "1_000s", "1S"]:
    REFUSALS.append([f"--timeout={duration}"])
    REFUSALS.append(["--timeout", duration, "--remote", "ftp://h"])
if POSIX:
    REFUSALS += [[b"stray\xff"], [b"--\xff"], [b"--timeout=\xff"], [b"--pull=\xff"]]

# Go refuses these too, but the spelling is outside the ported net/url subset,
# so the native process must stay on Go instead of guessing its wording.
UNPROVEN = [["--remote", b"\xff"] if POSIX else ["--remote", "\u00e4"],
            ["--remote=ftp://\u00e4"], ["--remote", "ftp://h/%zz"], ["--remote", "ftp://u@h"],
            ["--remote", "ftp://[::1"], ["--remote", "ftp://h/a b"], ["--remote", "ftp:\\h"]]

# (arguments, extra environment); Go runs against a closed loopback port.
GATES = [
    (["--remote", f"https://{CLOSED}"], {}),
    (["--remote", f"HTTPS://{CLOSED}"], {}),
    (["--remote", f"https://{CLOSED}/path?x=1#frag"], {}),
    (["--remote", "https:opaque"], {}),
    (["--remote", "http://localhost:1"], {}),
    (["--remote", "http://LOCALHOST:1"], {}),
    (["--remote", "http://127.9.9.9:1"], {}),
    (["--remote", "http://example.invalid:1", "--allow-insecure-http"], {}),
    (["--remote", f"https://{CLOSED}", "--pull"], {}),
    (["--remote", f"https://{CLOSED}", "--push", "--timeout=1s"], {}),
    (["--remote", f"https://{CLOSED}", "--timeout=1.5h", "--token", "t"], {}),
    (["--remote", f"https://{CLOSED}", "--encrypted-relay", "--relay-passphrase", "p"], {}),
    (["--remote", f"https://{CLOSED}", "--encrypted-relay"], {RELAY_ENV: "from-env"}),
    # outside the ported net/url subset: kept on Go whatever the verdict
    (["--remote", f"https://user@{CLOSED}"], {}),
    (["--remote", f"https://{CLOSED}/%41"], {}),
    (["--remote", "http://[::1]:1"], {}),
    (["--remote", f"https://{CLOSED}/\u00e4"], {}),
]
VALIDATION_MARKERS = (b"parse remote URL", b"requires an https URL", b"is not supported",
                      b"--remote is required", b"requires --relay-passphrase", b"unexpected argument",
                      b"flag provided but not defined", b"invalid value", b"invalid boolean value")


def tree(root):
    return sorted(str(path.relative_to(root)) for path in root.rglob("*"))


def observe(binary, args, root, env, output):
    record = output(binary, ["sync", *args], root, env)
    record["tree"] = tree(root)
    return record


def stopped_at_absent_go(record):
    import base64
    stderr = base64.b64decode(record["stderr"])
    return (record["exit"] == 1 and record["stdout"] == "" and record["tree"] == []
            and b"symbrain: start Go fallback" in stderr and b"(os error 2)" in stderr)


def run(go, rust, output, isolated_env):
    import base64
    records = []
    for index, args in enumerate(REFUSALS):
        with tempfile.TemporaryDirectory(prefix="memory-cli-sync-") as temporary:
            first, second = (observe(binary, args, Path(temporary), isolated_env(Path(temporary)), output)
                             for binary in (go, rust))
        # Neither process may touch the disposable root before refusing.
        refused = first["exit"] != 0 and first["tree"] == []
        records.append(dict(family="sync_preflight", kind="refusal", index=index, name="refusal-%03d" % index,
                            args_base64=[base64.b64encode(v if isinstance(v, bytes) else v.encode()).decode()
                                         for v in args],
                            match=first == second and refused, go=first, rust=second))
    for index, args in enumerate(UNPROVEN):
        with tempfile.TemporaryDirectory(prefix="memory-cli-sync-") as temporary:
            first = output(go, ["sync", *args], Path(temporary), isolated_env(Path(temporary)))
        with tempfile.TemporaryDirectory(prefix="memory-cli-sync-") as temporary:
            second = observe(rust, args, Path(temporary), isolated_env(Path(temporary)), output)
        stopped = stopped_at_absent_go(second)
        records.append(dict(family="sync_preflight", kind="unproven", index=index, name="unproven-%03d" % index,
                            match=first["exit"] != 0 and stopped, rust_stopped_at_absent_go=stopped,
                            go=first, rust=second))
    for index, (args, extra) in enumerate(GATES):
        with tempfile.TemporaryDirectory(prefix="memory-cli-sync-") as temporary:
            root = Path(temporary)
            env = isolated_env(root)
            env.update(extra)
            go_args = [*args, "--db", str(root / "go.db"), "--timeout=2s"]
            go_env = dict(env)
            first = output(go, ["sync", *go_args], root, go_env)
            # Fresh root for the native process: the Go run created files.
        with tempfile.TemporaryDirectory(prefix="memory-cli-sync-") as temporary:
            root = Path(temporary)
            env = isolated_env(root)
            env.update(extra)
            second = observe(rust, args, root, env, output)
        past = not any(marker in base64.b64decode(first["stderr"]) for marker in VALIDATION_MARKERS)
        stopped = stopped_at_absent_go(second)
        records.append(dict(family="sync_preflight", kind="gate", index=index, name="gate-%03d" % index,
                            args=args, match=past and stopped, go_passed_validation=past,
                            rust_stopped_at_absent_go=stopped, go=first, rust=second))
    return records
