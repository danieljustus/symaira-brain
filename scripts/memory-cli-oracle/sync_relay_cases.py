"""`memory sync` against a loopback fake relay (#762 slice 2).

The shipped Go CLI (immutable oracle) and the native CLI run the same scenario
against their own fresh relay and disposable root. Compared: stdout, stderr and
exit status of every step, the exact requests the relay received (method,
target, every header) and the complete database state afterwards. Ports are
normalised; nothing here connects to a non-loopback host.

* ``native`` scenarios must match byte for byte with the Go fallback absent,
  so a hand-off to Go cannot pass.
* ``handoff`` scenarios are everything the native client must not claim. Go
  runs normally; the native process (Go absent) must stop at the owned
  absent-binary diagnostic, create no cursor, and have sent either no request
  or exactly the first request Go sent (a read-only GET).
"""
import base64
from contextlib import closing
import gzip
import json
import os
from pathlib import Path
import sqlite3
import re
import tempfile

from sync_relay_server import Relay, http, json_reply, reply

POSIX = os.name == "posix"
OK_TIME = "2026-10-26T01:02:03.456Z"
FUTURE = "2999-01-01 00:00:00 +0000 UTC"
OLD = "2000-01-01 00:00:00 +0000 UTC"
NEW = "2026-10-26 00:00:00 +0000 UTC"


def ok_body(time=OK_TIME, extra=""):
    return '{"memories":[],"deleted":[],"server_time":"%s"%s}' % (time, extra)


def static(data):
    return lambda request: reply(data)


def ok(time=OK_TIME):
    return static(json_reply(200, ok_body(time)))


def step(*args, env=None, responder=None):
    return dict(args=["--remote={remote}", *args], env=env or {}, responder=responder)


def scenario(name, steps, responder=None, kind="native", post_get=False, seed=(), tree=False, go_exit=None):
    if isinstance(steps, dict):
        steps = [steps]
    return dict(name=name, steps=steps, responder=responder or ok(), kind=kind, post_get=post_get,
                seed=list(seed), tree=tree, go_exit=go_exit)


def sleeper(seconds, data):
    return lambda request: [("sleep", seconds), ("send", data)]


def build():
    cases = []
    add = cases.append
    # --- empty exchanges: modes, formats, repeated runs ---------------------
    for index, args in enumerate([[], ["--pull"], ["--push"], ["--json"], ["--pull", "--json"],
                                  ["--push", "--json"], ["--output=json"], ["--output", "table"],
                                  ["--pull", "--push"], ["--timeout=60s"], ["--timeout=1m"],
                                  ["--timeout", "30s"], ["--timeout=1s"], ["-pull"], ["-push=true"],
                                  ["--pull=false"]]):
        add(scenario("empty-%02d" % index, step(*args)))
    add(scenario("second-run-uses-cursor", [step(), step(), step("--json")]))
    add(scenario("push-after-pull-resets-cursor", [step("--pull"), step("--push"), step("--pull")]))
    add(scenario("both-after-both", [step(), step("--pull"), step("--push", "--json"), step()]))
    for index, remote in enumerate(["{remote}/", "{remote}//", "{remote}/prefix", "{remote}/prefix/",
                                    "{remote}/a/b.c/d_e-f", "{remote}/a//"]):
        add(scenario("prefix-%02d" % index, dict(args=["--remote=" + remote], env={}, responder=None)))
    # --- bearer token --------------------------------------------------------
    add(scenario("token-flag", step("--token", "abc.DEF-123")))
    add(scenario("token-env", step(env={"SYMBRAIN_MEMORY_SYNC_TOKEN": "from-env"})))
    add(scenario("token-flag-beats-env", step("--token=flag", env={"SYMBRAIN_MEMORY_SYNC_TOKEN": "env"})))
    add(scenario("token-empty-flag-uses-env", step("--token=", env={"SYMBRAIN_MEMORY_SYNC_TOKEN": "env"})))
    add(scenario("token-with-punctuation", step("--token", "a=b&c<d>e'f\"g!~")))
    add(scenario("insecure-flag-loopback-warns", step("--allow-insecure-http", "--token", "t")))
    # --- server_time spellings ------------------------------------------------
    for index, time in enumerate(["2026-10-26T01:02:03Z", "2026-10-26T01:02:03.1Z", "2026-10-26T01:02:03.123Z",
                                  "2026-10-26T01:02:03.123456Z", "2026-10-26T01:02:03.123456789Z",
                                  "2026-10-26T01:02:03.100000000Z", "2026-10-26T01:02:03.000Z",
                                  "0001-01-01T00:00:00Z", "0001-01-01T00:00:00.5Z", "9999-12-31T23:59:59Z",
                                  "2024-02-29T12:00:00Z"]):
        add(scenario("time-%02d" % index, [step("--json", responder=ok(time)), step("--pull", responder=ok(time))]))
    # --- success bodies --------------------------------------------------------
    for index, body in enumerate(['{}', 'null', ' \n{} \n', '{"memories":null,"deleted":null}',
                                  '{"memories":[],"deleted":[],"next_cursor":""}', '{"next_cursor":null}',
                                  '{"unknown":{"a":[1,2,{"b":null}]},"x":1e999}', '{"server_time":null}',
                                  '{"server_time":"2026-10-26T01:02:03Z","server_time":null}',
                                  '{"memories":[]} trailing text', '{"a":1}{"b":2}',
                                  '{"memories":[],"deleted":[],"server_time":"2026-10-26T01:02:03Z"}\n',
                                  '\ufeff{}'.encode(), '{"k\\u00e4y":"v\\u00e4lue"}']):
        data = body if isinstance(body, bytes) else body.encode()
        add(scenario("body-%02d" % index, [step(), step("--json")], static(json_reply(200, data))))
    add(scenario("status-300-body-is-decoded", step(), static(json_reply(300, ok_body()))))
    add(scenario("body-without-length-until-close", step(),
                 lambda request: [("send", http(200, b"", {"Connection": "close"}, length=False)),
                                  ("send", ok_body().encode()), ("close",)]))
    chunked = (b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"
               + b"%x\r\n" % 9 + b'{"server_' + b"\r\n" + b"%x\r\n" % 29 + b'time":"2026-10-26T01:02:03Z"}' + b"\r\n"
               + b"0\r\n\r\n")
    add(scenario("body-chunked", step(), static(chunked)))
    # --- decode failures ---------------------------------------------------------
    for index, body in enumerate([b"", b" ", b"\n\t\r ", b"x", b"}", b"]", b",", b":", b"'", b"\\", b"<html>",
                                  b"+1", b".5", b"@", b"\xff", b"\xef\xbb\xbf{}", b"\x01", b"\x0c", b"\x7f", b"\x85", b"\xa0", b"\xad",
                                  b"\x00", b"\x07", b"\x08", b"\x0b", b"\xc3\xa4", b'{"server_time"', b'{"memories":',
                                  b'{"a":1', b'{']):
        add(scenario("decode-failure-%02d" % index, step(), static(json_reply(200, body))))
    add(scenario("status-204-empty", step(), static(http(204))))
    add(scenario("truncated-by-close", step(),
                 lambda request: [("send", http(200, b'{"server_time"', length=50)), ("close",)]))
    add(scenario("timeout-stalled-body", step("--timeout=1s"),
                 lambda request: [("send", http(200, b'{"server_time"', length=50)), ("sleep", 4)]))
    add(scenario("timeout-stalled-headers", step("--timeout=1s"),
                 lambda request: [("send", b"HTTP/1.1 200 OK\r\nContent-Le"), ("sleep", 4)]))
    add(scenario("timeout-slow-reply", step("--timeout=1s"), sleeper(4, json_reply(200, ok_body()))))
    add(scenario("within-timeout-reply", step("--timeout=2s"), sleeper(0.3, json_reply(200, ok_body()))))
    add(scenario("informational-reply-then-silence", step("--timeout=1s"),
                 lambda request: [("send", http(199)), ("sleep", 4)]))
    # --- error statuses -----------------------------------------------------------
    add(scenario("status-304-empty", step(), static(http(304))))
    for status in (400, 401, 403, 404, 409, 413, 418, 429, 500, 502, 503, 504, 599, 305, 306):
        add(scenario("status-%d-json" % status, step(),
                     static(json_reply(status, '{"error":"nope %d","code":"E%d"}' % (status, status)))))
    for index, body in enumerate([b'{"error":"boom"}', b'{"code":"only-code"}', b'{"error":"a","code":""}',
                                  b'{"ERROR":"up","Code":"x"}', b'{"Error":"Mixed","cOdE":"y"}',
                                  b'{"error":5,"code":"c"}', b'{"error":"e","code":7}', b'{"error":null}',
                                  b'{"error":"first","error":"second"}', b'{"error":"a","ERROR":"b"}',
                                  b'{"error":"\\u00e4\\n\\"quoted\\" <&>"}', "{\"error\":\"\u00e4\u20ac\"}".encode(),
                                  b'[1]', b'"text"', b'12', b'null', b'', b'<html>oops</html>', b'{"error":"x"} {"error":"y"}',
                                  b'{"error":"cut', b'{"nested":{"error":"deep"},"error":"top"}',
                                  b'{"error":"' + b"a" * 70000 + b'"}', b'{"error":"a"}' + b" " * 70000,
                                  b'{"x":[' + b"1," * 40000 + b'2],"error":"late"}',
                                  b'{"\\u0065rror":"escaped key"}', b'{"x":1e999,"error":"z"}']):
        add(scenario("error-body-%02d" % index, step(), static(json_reply(500, body))))
        add(scenario("error-body-%02d-4xx" % index, step(), static(json_reply(404, body))))
    add(scenario("error-status-leaves-old-cursor", [step(), step(responder=static(json_reply(500, '{"error":"x"}'))),
                                                    step()]))
    for depth in (50, 200, 900):
        add(scenario("error-body-nesting-%d" % depth, step(),
                     static(json_reply(500, b'{"x":' + b"[" * depth + b"]" * depth + b',"error":"deep"}'))))
    add(scenario("error-body-stalled", step("--timeout=1s"),
                 lambda request: [("send", http(500, b'{"error":"part', length=50)), ("sleep", 4)]))
    add(scenario("error-body-cut", step(),
                 lambda request: [("send", http(500, b'{"error":"part', length=50)), ("close",)]))
    add(scenario("error-body-complete-then-cut", step(),
                 lambda request: [("send", http(500, b'{"error":"whole"}tail', length=60)), ("close",)]))
    # --- transport failures ----------------------------------------------------------
    add(scenario("connection-refused", dict(args=["--remote=http://127.0.0.1:1"], env={}, responder=None)))
    add(scenario("connection-refused-with-query-cursor", [step(),
                 dict(args=["--remote={remote}"], env={}, responder=None, close_relay=True)]))
    add(scenario("connection-refused-pull", dict(args=["--remote=http://127.0.0.1:1", "--pull", "--token=t"],
                                                  env={}, responder=None)))
    add(scenario("connection-refused-prefix", dict(args=["--remote=http://127.0.0.1:1/p/q/"], env={},
                                                    responder=None)))
    add(scenario("connection-refused-insecure-flag", dict(args=["--remote=http://127.0.0.1:1", "--allow-insecure-http"],
                                                           env={}, responder=None)))
    # --- stored cursors and local changes -----------------------------------------------
    cursor = lambda text: ("cursor", text)
    memory = lambda updated, metadata="{}": ("memory", updated, metadata)
    for index, text in enumerate(["2026-10-26 01:02:03 +0000 UTC", "2026-10-26 01:02:03.123456789 +0000 UTC",
                                  "2026-10-26 01:02:03.5 +0000 UTC", "0001-01-01 00:00:00 +0000 UTC", FUTURE,
                                  "2026-12-31 23:59:59.999999999 +0000 UTC"]):
        add(scenario("stored-cursor-%02d" % index, [step(), step("--json"), step("--pull")], seed=[cursor(text)]))
    add(scenario("old-memory-before-cursor-does-not-push", [step(), step("--push"), step("--pull")],
                 seed=[cursor("2026-01-01 00:00:00 +0000 UTC"), memory(OLD)]))
    add(scenario("excluded-memory-does-not-push", step(),
                 seed=[cursor(OLD), memory(NEW, '{"sync_exclude":"true"}')]))
    add(scenario("push-only-with-future-cursor-resets-it", step("--push"), seed=[cursor(FUTURE)]))
    add(scenario("another-remote-cursor-untouched", step(), seed=[("other-cursor", "http://127.0.0.1:2",
                                                                    "2026-10-26 01:02:03 +0000 UTC")]))
    # --- default database location ----------------------------------------------------------
    add(scenario("default-database-created", dict(args=["--remote={remote}", "--no-db"], env={}, responder=None),
                 tree=True))
    add(scenario("default-database-refused-reply", dict(args=["--remote={remote}", "--no-db"], env={},
                                                       responder=static(json_reply(500, '{"error":"x"}'))), tree=True))
    # =============================== hand-offs =========================================
    pre = lambda name, steps, **kw: add(scenario(name, steps, kind="handoff", **kw))
    post = lambda name, steps, responder, **kw: add(scenario(name, steps, responder, kind="handoff", post_get=True, **kw))
    # before any network traffic
    pre("handoff-https", dict(args=["--remote=https://127.0.0.1:1"], env={}, responder=None))
    pre("handoff-uppercase-scheme", dict(args=["--remote=HTTP://127.0.0.1:{port}"], env={}, responder=None))
    pre("handoff-localhost", dict(args=["--remote=http://localhost:{port}"], env={}, responder=None))
    pre("handoff-other-loopback-quad", dict(args=["--remote=http://127.9.9.9:{port}", "--timeout=1s"], env={}, responder=None))
    pre("handoff-no-port", dict(args=["--remote=http://127.0.0.1"], env={}, responder=None))
    pre("handoff-port-80", dict(args=["--remote=http://127.0.0.1:80"], env={}, responder=None))
    pre("handoff-leading-zero-port", dict(args=["--remote=http://127.0.0.1:0{port}"], env={}, responder=None))
    pre("handoff-query-in-remote", dict(args=["--remote={remote}?x=1"], env={}, responder=None))
    pre("handoff-fragment-in-remote", dict(args=["--remote={remote}#f"], env={}, responder=None))
    pre("handoff-dotdot-in-remote", dict(args=["--remote={remote}/a/../b"], env={}, responder=None))
    pre("handoff-empty-segment-in-remote", dict(args=["--remote={remote}/a//b"], env={}, responder=None))
    pre("handoff-userinfo", dict(args=["--remote=http://u@127.0.0.1:{port}"], env={}, responder=None))
    pre("handoff-encrypted-relay", step("--encrypted-relay", "--relay-passphrase", "pw"))
    pre("handoff-encrypted-relay-env", step("--encrypted-relay", env={"SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE": "pw"}))
    for index, token in enumerate(["a b", " lead", "trail ", "t\u00e4", "tab\there"]):
        pre("handoff-token-%d" % index, step("--token", token))
    for index, timeout in enumerate(["2m", "61s", "1h", "500ms", "999ms", "0", "-1s", "0s", "1.5s", "1m1s", "01s",
                                     "1000s", "90s", "1ns", "1us"]):
        pre("handoff-timeout-%02d" % index, step("--timeout=" + timeout))
    pre("handoff-unsupported-cursor", step(), seed=[cursor("2026-10-26 03:02:03.456 +0200 +0200")])
    pre("handoff-cursor-rfc3339", step(), seed=[cursor("2026-10-26T01:02:03Z")])
    pre("handoff-cursor-no-zone", step(), seed=[cursor("2026-10-26 01:02:03")])
    pre("handoff-pending-memory", step(), seed=[cursor(OLD), memory(NEW)])
    pre("handoff-pending-memory-push-only", step("--push"), seed=[cursor(OLD), memory(NEW)])
    pre("handoff-pending-memory-zero-cursor", step(), seed=[memory(OLD)])
    pre("handoff-pending-tombstone", step(), seed=[cursor(OLD), ("tombstone", NEW)])
    pre("handoff-pending-tombstone-push-only", step("--push"), seed=[cursor(OLD), ("tombstone", NEW)])
    # after the first read-only GET
    memories = '{"memories":[{"id":"remote-1","content":"c","scope":"global","created_at":"2026-10-26T01:00:00Z","updated_at":"2026-10-26T01:00:00Z"}],"deleted":[],"server_time":"%s"}' % OK_TIME
    for name, data in [("memories", memories),
                       ("offset-server-time", '{"server_time":"2026-10-26T03:02:03+02:00"}'),
                       ("bad-server-time", '{"server_time":"bad"}'),
                       ("numeric-server-time", '{"server_time":5}'),
                       ("array-body", '[]'), ("string-body", '"x"'), ("number-body", '1'), ("true-body", 'true'),
                       ("memories-wrong-type", '{"memories":{}}'), ("syntax-error", '{"a":}'),
                       ("folded-key", '{"Server_Time":"2026-10-26T01:02:03Z"}'),
                       ("lowercase-fraction-t", '{"server_time":"2026-10-26t01:02:03Z"}'),
                       ("time-seconds-60", '{"server_time":"2026-10-26T01:02:60Z"}'),
                       ("time-year-zero", '{"server_time":"0000-01-01T00:00:00Z"}'),
                       ("time-ten-digit-fraction", '{"server_time":"2026-10-26T01:02:03.1234567890Z"}')]:
        post("handoff-reply-" + name, step(), static(json_reply(200, data)))
    post("handoff-reply-gzip", step(), static(http(200, gzip.compress(ok_body().encode()),
                                                   {"Content-Encoding": "gzip"})))
    for status in (301, 302, 303, 307, 308):
        post("handoff-redirect-%d" % status, step(), static(http(status, b"", {"Location": "/moved"})))
    post("handoff-redirect-without-location", step(), static(http(301, b'{"error":"moved"}')))
    post("handoff-garbage-status-line", step(), static(b"garbage\r\n\r\n"))
    post("handoff-closed-before-reply", step(), lambda request: [("close",)])
    post("handoff-malformed-chunks", step(),
         static(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n8\r\n{\"server\r\n19\r\n_time\":\"2026-10-26T01:02:0\r\n0\r\n\r\n"))
    post("handoff-oversized-success-body", step(), static(json_reply(200, '{"pad":"' + "a" * (1 << 20) + '"}')))
    post("handoff-error-with-bad-utf8", step(), static(json_reply(500, b'{"error":"\xff\xfe"}')))
    for depth in (1001, 9000, 10000, 10001, 12000):
        post("handoff-error-nesting-%d" % depth, step(),
             static(json_reply(500, b'{"x":' + b"[" * depth + b"]" * depth + b',"error":"deep"}')))
    post("handoff-reply-nesting-1001", step(),
         static(json_reply(200, b'{"x":' + b"[" * 1001 + b"]" * 1001 + b'}')))
    post("handoff-error-leading-bad-byte", step(), static(json_reply(500, b'\xff{"error":"x"}')))
    import sync_pull_cases  # imported here: that module imports this one

    cases.extend(sync_pull_cases.build())
    if not POSIX:
        # The refusal wording is the POSIX errno text (Windows words it differently): skip visibly.
        skipped = [case["name"] for case in cases if case["name"].startswith("connection-refused")]
        print("sync_relay: skipping POSIX-only connection-refused wording scenarios on Windows:", skipped, flush=True)
        cases = [case for case in cases if case["name"] not in skipped]
    return cases


# --------------------------------------------------------------------------------------
ABSENT_MARKER = b"symbrain: start Go fallback"


def seed_database(go, root, env, db, port, items, output):
    result = output(go, ["list", "--db", str(db)], root, env)
    assert result["exit"] == 0, result
    remote = f"http://127.0.0.1:{port}"
    with closing(sqlite3.connect(db)) as database, database:
        for number, item in enumerate(items):
            if item[0] == "cursor":
                database.execute("INSERT INTO sync_state(remote,last_sync) VALUES(?,?)", (remote, item[1]))
            elif item[0] == "other-cursor":
                database.execute("INSERT INTO sync_state(remote,last_sync) VALUES(?,?)", (item[1], item[2]))
            elif item[0] == "memory":
                database.execute(
                    "INSERT INTO memories(id,content,scope,kind,created_at,updated_at,metadata,embedding) "
                    "VALUES(?,?,'global','reference',?,?,?,X'')",
                    (f"seed-{number}", f"content {number}", OLD, item[1], item[2]))
            elif item[0] == "memory-id":  # (kind, id, updated_at text, content, metadata)
                database.execute(
                    "INSERT INTO memories(id,content,scope,kind,created_at,updated_at,metadata,embedding) "
                    "VALUES(?,?,'global','reference',?,?,?,X'')", (item[1], item[3], OLD, item[2], item[4]))
            elif item[0] == "tombstone-id":  # (kind, id, ts text)
                database.execute("INSERT INTO sync_oplog(op,memory_id,ts) VALUES('delete',?,?)", (item[1], item[2]))
            elif item[0] == "tombstone":
                database.execute("INSERT INTO sync_oplog(op,memory_id,ts) VALUES('delete',?,?)",
                                 (f"gone-{number}", item[1]))
            else:
                raise AssertionError(item)


def tree_listing(root):
    listing = []
    for path in sorted(root.rglob("*")):
        mode = path.stat().st_mode & 0o777
        listing.append([str(path.relative_to(root)), "dir" if path.is_dir() else "file", mode if POSIX else None])
    return listing


def fresh(responder):
    """Stateful responders (see ``sync_pull_cases.Nth``) restart for every execution."""
    return responder.fresh() if hasattr(responder, "fresh") else responder


def execute(binary, case, go, output, snapshot, isolated_env):
    """Runs every step of `case` with `binary`; returns normalised observations."""
    with tempfile.TemporaryDirectory(prefix="memory-cli-relay-") as temporary:
        root = Path(temporary)
        env = isolated_env(root)
        database = root / "x.db"
        case_responder = fresh(case["responder"])
        current = {"responder": case_responder}
        relay = Relay(lambda request: current["responder"](request))
        port = relay.port
        try:
            if case["seed"]:
                seed_database(go, root, env, database, port, case["seed"], output)
            seeded_state = snapshot(root)
            steps = []
            for item in case["steps"]:
                current["responder"] = fresh(item["responder"]) if item.get("responder") else case_responder
                if item.get("close_relay"):
                    relay.close()
                    relay.assert_refusing()  # the scenario is only meaningful if the port really refuses
                args = []
                for argument in item["args"]:
                    if argument == "--no-db":
                        continue
                    args.append(argument.replace("{remote}", f"http://127.0.0.1:{port}").replace("{port}", str(port)))
                if "--no-db" not in item["args"]:
                    args += ["--db", str(database)]
                step_env = dict(env)
                step_env.update(item["env"])
                relay.requests.clear()
                record = output(binary, ["sync", *args], root, step_env)
                record["requests"] = [dict(request) for request in relay.requests]
                steps.append(record)
            state = snapshot(root)
            tree = tree_listing(root) if case["tree"] else None
        finally:
            relay.close()
    return normalise(dict(steps=steps, state=state, seeded_state=seeded_state, tree=tree), port)


def normalise(observation, port):
    stamp = re.compile(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z$")
    shipped = re.compile(r"^\d{4}-\d\d-\d\d \d\d:\d\d:\d\d(\.\d{1,9})? \+0000 UTC$")
    uuid = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$")
    for key in ("state", "seeded_state"):
        for tables in observation[key].values():
            for row in tables.get("schema_migrations", []):
                row[1] = "applied-at"  # second-resolution wall clock of each run
            for row in tables.get("sync_oplog", []):
                # oplog triggers stamp rows with the wall clock; seeded tombstones keep fixed times
                row[:] = ["wall-clock" if isinstance(cell, str) and stamp.match(cell) else cell for cell in row]
            # audit rows carry a random id and the wall clock (columns id, ..., created_at);
            # only values in the shipped spelling are replaced, so a different shape still differs
            audit = tables.get("audit_log", [])
            for row in audit:
                if isinstance(row[0], str) and uuid.match(row[0]):
                    row[0] = "uuid"
                if isinstance(row[7], str) and shipped.match(row[7]):
                    row[7] = "wall-clock"
            audit.sort(key=lambda row: json.dumps(row, sort_keys=True))
    text = json.dumps(observation, sort_keys=True)
    # stdout/stderr are base64; decode them so the port inside can be replaced. Only ``:<port>`` is
    # replaced: a bare digit run can occur inside a content hash and would differ between two runs.
    data = json.loads(text)
    pattern = rb":%d(?!\d)" % port
    for record in data["steps"]:
        for key in ("stdout", "stderr"):
            record[key] = base64.b64encode(re.sub(pattern, b":PORT", base64.b64decode(record[key]))).decode()
    return json.loads(re.sub(":%d(?!\\d)" % port, ":PORT", json.dumps(data, sort_keys=True)))


def stopped_at_absent_go(record):
    stderr = base64.b64decode(record["stderr"])
    return (record["exit"] == 1 and record["stdout"] == "" and ABSENT_MARKER in stderr
            and b"(os error 2)" in stderr)


LOCAL_TABLES = ("memories", "audit_log", "sync_oplog", "sync_state", "entities", "memory_entities", "memory_evidence")


def unchanged_locally(state, seeded):
    """No cursor, memory, audit, oplog or entity row differs from the seeded database (or from none)."""
    for path, tables in state.items():
        before = seeded.get(path, {})
        for name in LOCAL_TABLES:
            if tables.get(name, []) != before.get(name, []):
                return False
    return True


def run(go, rust, output, snapshot, isolated_env):
    records = []
    only = os.environ.get("SYNC_RELAY_ONLY")  # regex over scenario names (source-mutant controls only)
    for index, case in enumerate(build()):
        if only and not re.search(only, case["name"]):
            continue
        if os.environ.get("SYNC_RELAY_TRACE"):
            print("case", index, case["name"], flush=True)
        first = execute(go, case, go, output, snapshot, isolated_env)
        second = execute(rust, case, go, output, snapshot, isolated_env)
        record = dict(family="sync_relay", kind=case["kind"], index=index, name=case["name"])
        if case["kind"] == "native":
            went_native = not any(stopped_at_absent_go(step) for step in second["steps"])
            record.update(match=first == second and went_native, went_native=went_native,
                          go_requests=sum(len(step["requests"]) for step in first["steps"]), go=first, rust=second)
        else:
            go_requests = [step["requests"] for step in first["steps"]]
            rust_requests = [step["requests"] for step in second["steps"]]
            stopped = all(stopped_at_absent_go(step) for step in second["steps"])
            if case["post_get"]:
                # only read-only GETs, each one a request Go sent too, in Go's order
                sent = rust_requests[0]
                requests_ok = (len(go_requests[0]) >= 1 and len(sent) >= 1
                               and sent == go_requests[0][:len(sent)]
                               and all(request["method"] == "GET" for request in sent))
            else:
                requests_ok = all(not requests for requests in rust_requests)
            # Go did the real work: it is the oracle for what stays on Go.
            go_worked = bool(go_requests[0]) if case["post_get"] else True
            untouched = unchanged_locally(second["state"], second["seeded_state"])
            record.update(match=stopped and requests_ok and untouched and go_worked,
                          rust_stopped_at_absent_go=stopped, requests_as_expected=requests_ok,
                          cursor_untouched=untouched, go=first, rust=second)
        records.append(record)
    return records
