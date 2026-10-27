"""`memory sync` pulls that carry data to apply (#762 slice 3).

Scenarios of the ``sync_relay`` family (see ``sync_relay_cases.py`` for the
comparison rules): the loopback fake relay answers ``/api/sync/changes`` with
memories, tombstones and pagination cursors and the shipped Go CLI (immutable
oracle) and the native CLI must leave the same stdout, stderr, exit status,
relay requests and database state.

* ``native`` scenarios cover the proven shapes: new memories, the
  last-writer-wins rule against local rows and tombstones, remote deletes,
  mixed batches, multi-page pulls and the cursor/server-time handling.
* ``handoff`` scenarios are everything that must stay on Go. Beyond the rules
  of the family, the native process may not have written anything locally
  (no memory, audit row, oplog row or cursor) when it stops.
"""
import json
from urllib.parse import parse_qs, urlsplit

from sync_relay_server import http, json_reply, reply
from sync_relay_cases import FUTURE, scenario, step, static

T0 = "2026-10-26T01:00:00Z"
T1 = "2026-10-26T02:00:00Z"
T2 = "2026-10-26T03:00:00Z"
L0 = "2026-10-26 01:00:00 +0000 UTC"  # the same instants in the stored rendering
L1 = "2026-10-26 02:00:00 +0000 UTC"
L2 = "2026-10-26 03:00:00 +0000 UTC"
SERVER = "2026-10-26T04:00:00Z"
CREATED = "2026-10-25T00:00:00Z"
LONG_AGO = "2000-01-01T00:00:00Z"  # older than any wall clock a tombstone can carry
FAR = "2999-01-01T00:00:00Z"


def mem(identifier, updated=T0, drop=(), **fields):
    """One remote memory as JSON text."""
    entry = {"id": identifier, "content": "remote " + str(identifier), "scope": "global",
             "created_at": CREATED, "updated_at": updated}
    entry.update(fields)
    for key in drop:
        entry.pop(key, None)
    return json.dumps(entry, ensure_ascii=False)


def gone(identifier, at=T1):
    return json.dumps({"id": identifier, "deleted_at": at})


def body(memories=(), deleted=(), server=SERVER, cursor=None, extra=""):
    parts = ['"memories":[%s]' % ",".join(memories), '"deleted":[%s]' % ",".join(deleted)]
    if server is not None:
        parts.append('"server_time":"%s"' % server)
    if cursor is not None:
        parts.append('"next_cursor":%s' % json.dumps(cursor, ensure_ascii=False))
    return "{" + ",".join(parts) + extra + "}"


def answer(data):
    if isinstance(data, list):  # explicit relay steps
        return data
    if isinstance(data, bytes) and data.startswith(b"HTTP/"):
        return reply(data)
    return reply(json_reply(200, data))


def pages(table):
    """Answers by pagination cursor: ``{None: first_page, "c": second_page, ...}``."""
    def responder(request):
        query = parse_qs(urlsplit(request["target"]).query, keep_blank_values=True)
        data = table.get(query.get("cursor", [None])[0])
        if data is None:
            return reply(json_reply(400, '{"error":"unknown cursor","code":"E_CURSOR"}'))
        return answer(data)
    return responder


class Nth:
    """Answers by request number (the last reply repeats); fresh per execution."""

    def __init__(self, replies):
        self.replies = replies
        self.count = 0

    def fresh(self):
        return Nth(self.replies)

    def __call__(self, request):
        if not request["target"].startswith("/api/sync/"):
            # a foreign request (the request control's extra probe) must not advance the script
            return reply(http(404))
        self.count += 1
        return answer(self.replies[min(self.count, len(self.replies)) - 1])


def nth(replies):
    return Nth(replies)


def one(data):
    return static(json_reply(200, data))


def local(identifier, updated, content="local", metadata="{}"):
    return ("memory-id", identifier, updated, content, metadata)


def tomb(identifier, at):
    return ("tombstone-id", identifier, at)


def first_then_close(first):
    return lambda request: ([("send", json_reply(200, first))] if "cursor=" not in request["target"] else [("close",)])


def build():
    cases = []
    add = cases.append

    def native(name, steps, responder=None, **kw):
        known_defect = kw.pop("known_go_defect", None)
        case = scenario(name, steps, responder, **kw)
        if known_defect is not None:
            case["known_go_defect"] = known_defect
        add(case)

    def hand(name, steps, responder=None, post_get=True, **kw):
        add(scenario(name, steps, responder, kind="handoff", post_get=post_get, **kw))

    pull = step("--pull")
    pull_json = step("--pull", "--json")

    # --- new memories: shapes and renderings --------------------------------
    native("pull-new-memory", pull, one(body([mem("a")])))
    native("pull-new-memory-json", pull_json, one(body([mem("a")])))
    native("pull-new-memory-table-twice", [pull, pull], one(body([mem("a")])))
    native("pull-two-memories", pull, one(body([mem("a"), mem("b", T1)])))
    native("pull-same-id-newer-second", pull_json, one(body([mem("a", T0), mem("a", T1, content="second")])))
    native("pull-same-id-older-second", pull_json, one(body([mem("a", T1), mem("a", T0, content="second")])))
    native("pull-same-id-equal-second", pull_json, one(body([mem("a", T0), mem("a", T0, content="second")])))
    native("pull-minimal-entry", pull_json, one(body(['{"id":"x","updated_at":"%s"}' % T0])))
    native("pull-id-only-entry", pull_json, one(body(['{"id":"x"}'])))
    native("pull-null-fields", pull_json, one(body([mem("a", content=None, scope=None, metadata=None, importance=None,
                                                       access_count=None, created_at=None, kind=None)])))
    native("pull-all-fields", pull_json, one(body([mem(
        "full", embedding=[], embedding_source="src", embedding_model="mod", embedding_quantization="q",
        content_hash="given-hash", created_by="alice", updated_by="bob", created_session="s1", updated_session="s2",
        entities=["e1", "e2"], consolidation_status="consolidated", importance=0.75, valid_from=T0, valid_to=T2,
        superseded_by="other", kind="project", review_status="staged", decay_factor=0.5, retired_at=T1,
        tier="working", expires_at=T2, access_count=42, last_access=T1, prev_access=T0, evidence=None,
        metadata={"k": "v", "a": "b"}, unknown={"x": [1, 2, {"y": None}]})])))
    native("pull-defaults-from-empty", pull_json, one(body([mem("d", consolidation_status="", review_status="",
                                                             decay_factor=0, importance=0, content_hash="")])))
    for index, decay in enumerate([0, -1, -0.5, 2, 1, 0.5, 1.0000000001, 1e-9, 0.3]):
        native("pull-decay-%02d" % index, pull, one(body([mem("a", decay_factor=decay)])))
    for index, importance in enumerate(["0", "1", "0.5", "1e2", "-1", "0.1", "0.30000000000000004", "123456789.123456789",
                                        "1.7976931348623157e308", "5e-324", "4.9e-324", "1E5", "1e+5", "0.000001",
                                        "1e21", "12345678901234567890", "-0", "-0.0", "9007199254740993",
                                        "0.1e1", "1.5e-10", "2.2250738585072014e-308", "1e-400", "-1e-400", "0e0", "0.0000"]):
        native("pull-importance-%02d" % index, pull,
               one(body(['{"id":"a","updated_at":"%s","importance":%s}' % (T0, importance)])))
    for index, count in enumerate(["0", "1", "-3", "9223372036854775807", "-9223372036854775808", "-0", "7"]):
        native("pull-access-count-%02d" % index, pull,
               one(body(['{"id":"a","updated_at":"%s","access_count":%s}' % (T0, count)])))
    for index, status in enumerate(["raw", "consolidated", "archived", "weird status", ""]):
        native("pull-status-%02d" % index, pull, one(body([mem("a", consolidation_status=status)])))
    for index, tier in enumerate(["", "working", "long_term", "other"]):
        native("pull-tier-%02d" % index, pull, one(body([mem("a", tier=tier)])))
    native("pull-tier-absent", pull, one(body([mem("a")])))
    for index, review in enumerate(["staged", "approved", "pending", "odd"]):
        native("pull-review-%02d" % index, pull, one(body([mem("a", review_status=review)])))
    for index, scope in enumerate(["", "project", "agent", "user", "session", "x" * 300, "\u00fcn\u00ef"]):
        native("pull-scope-%02d" % index, pull, one(body([mem("a", scope=scope)])))
    for index, content in enumerate(["", "plain", "\u00fcn\u00efcode \u20ac \U0001F600", "line\nbreak\ttab\r\n",
                                     "<tag> & \"quote\" 'single' \\ /", "\u2028\u2029", "x" * 20000, "null",
                                     "{\"json\": [1,2]}", "tab\u0001ctrl\u001f", "\u007f"]):
        native("pull-content-%02d" % index, pull, one(body([mem("a", content=content)])))
    for index, identifier in enumerate(["a", "\u00fcn\u00ef\u20ac", "with space", "tombstone:x", "UPPER", "a/b?c=d&e",
                                        "\U0001F600", "x" * 500, "0"]):
        native("pull-id-%02d" % index, pull, one(body([mem(identifier)])))
    for index, metadata in enumerate([None, {}, {"b": "2", "a": "1"}, {"html": "<a href=\"x\">&</a>"},
                                      {"uni": "\u00fcn\u00ef \u20ac \U0001F600", "\u00fcn\u00ef": "k"},
                                      {"ctrl": "a\u0001\b\f\n\r\t\u001f\u007f"},
                                      {"sep": "\u2028\u2029"}, {"": ""}, {"sync_exclude": "true"},
                                      {"k": "v" * 5000}, {"slash": "a/b\\c", "quote": "\"q\""}]):
        native("pull-metadata-%02d" % index, pull_json, one(body([mem("a", metadata=metadata)])))
    native("pull-metadata-absent", pull, one(body([mem("a")])))
    native("pull-metadata-duplicate-keys", pull,
           one(body(['{"id":"a","updated_at":"%s","metadata":{"k":"first","k":"second","j":"x"}}' % T0])))
    native("pull-embedding-empty", pull, one(body([mem("a", embedding=[])])))
    native("pull-embedding-null", pull, one(body([mem("a", embedding=None)])))
    native("pull-embedding-absent", pull, one(body([mem("a")])))
    for index, spelling in enumerate(["2026-10-26T01:00:00Z", "2026-10-26T01:00:00.1Z", "2026-10-26T01:00:00.12Z",
                                      "2026-10-26T01:00:00.123Z", "2026-10-26T01:00:00.123456Z",
                                      "2026-10-26T01:00:00.123456789Z", "2026-10-26T01:00:00.100000000Z",
                                      "2026-10-26T01:00:00.000000001Z", "0001-01-01T00:00:00Z", "9999-12-31T23:59:59Z",
                                      "2024-02-29T23:59:59.999999999Z", "1970-01-01T00:00:00Z", "1969-12-31T23:59:59.5Z"]):
        native("pull-time-%02d" % index, pull_json,
               one(body([mem("a", spelling, created_at=spelling, valid_from=spelling, last_access=spelling)])))
    native("pull-time-created-absent", pull_json, one(body([mem("a", drop=("created_at",))])))
    native("pull-time-updated-absent", pull_json, one(body([mem("a", drop=("updated_at",))])))
    native("pull-time-pointer-nulls", pull_json,
           one(body([mem("a", valid_from=None, valid_to=None, expires_at=None, last_access=None, prev_access=None,
                         retired_at=None)])))
    native("pull-unknown-keys-ignored", pull, one(body([mem("a", embedding_binary="AAAA", whatever=1,
                                                             nested={"a": [{"b": None}]})],
                                                         extra=',"unknown":[1,{"a":2}]')))
    native("pull-token-flag", step("--pull", "--token", "tok-123"), one(body([mem("a")])))
    native("pull-token-env", step("--pull", env={"SYMBRAIN_MEMORY_SYNC_TOKEN": "from-env"}), one(body([mem("a")])))
    native("pull-insecure-flag-warns", step("--pull", "--allow-insecure-http"), one(body([mem("a")])))
    native("pull-timeout-flag", step("--pull", "--timeout=30s"), one(body([mem("a")])))
    native("pull-prefix-url", dict(args=["--remote={remote}/pre/fix/", "--pull"], env={}, responder=None),
           one(body([mem("a")])))
    native("pull-many-memories", pull_json,
           one(body([mem("m%03d" % number, "2026-10-26T01:%02d:%02dZ" % (number // 60, number % 60))
                     for number in range(300)])))
    native("pull-same-run-twice-is-idempotent", [pull, pull_json], one(body([mem("a"), mem("b", T1)], [gone("c")])))

    # --- last-writer-wins against local rows --------------------------------------
    for name, local_time, remote_time in [
            ("local-older", L0, T1), ("local-equal", L1, T1), ("local-newer", L2, T1),
            ("one-second-older", "2026-10-26 01:59:59 +0000 UTC", T1),
            ("one-second-newer", "2026-10-26 02:00:01 +0000 UTC", T1),
            ("nano-older", "2026-10-26 02:00:00.5 +0000 UTC", "2026-10-26T02:00:00.500000001Z"),
            ("nano-equal", "2026-10-26 02:00:00.5 +0000 UTC", "2026-10-26T02:00:00.5Z"),
            ("nano-newer", "2026-10-26 02:00:00.5 +0000 UTC", "2026-10-26T02:00:00.499999999Z"),
            ("micro-older", "2026-10-26 02:00:00.123456 +0000 UTC", "2026-10-26T02:00:00.123457Z"),
            ("micro-equal", "2026-10-26 02:00:00.123456 +0000 UTC", "2026-10-26T02:00:00.123456Z"),
            ("zero-local", "0001-01-01 00:00:00 +0000 UTC", T0),
            ("zero-remote", L0, "0001-01-01T00:00:00Z"),
            ("far-future-local", "9999-12-31 23:59:59 +0000 UTC", T2),
            ("far-future-remote", L2, "9999-12-31T23:59:59Z")]:
        native("lww-" + name, [pull_json, pull],
               one(body([mem("a", remote_time, content="remote wins", scope="project", metadata={"from": "remote"},
                             importance=0.9, created_by="rc", updated_by="ru", created_session="cs",
                             updated_session="us", valid_from=T0, tier="working", access_count=9, kind="user",
                             review_status="staged")])),
               seed=[local("a", local_time)])
    native("lww-update-keeps-created-and-validity-columns", pull_json,
           one(body([mem("a", T2, created_at="2020-01-01T00:00:00Z", valid_from="2020-01-01T00:00:00Z",
                         valid_to="2030-01-01T00:00:00Z", superseded_by="zzz", created_by="remote-creator",
                         created_session="remote-session")])), seed=[local("a", L0)])
    native("lww-update-clears-optional-columns", pull_json,
           one(body(['{"id":"a","updated_at":"%s","content":"c"}' % T2])),
           seed=[local("a", L0, metadata='{"keep":"me"}')])
    native("lww-three-local-rows-mixed", pull_json, one(body([mem("a", T1), mem("b", T1), mem("c", T1)])),
           seed=[local("a", L0), local("b", L2), local("c", L1)])
    native("lww-update-then-same-id-older", pull_json,
           one(body([mem("a", T2, content="newer"), mem("a", T1, content="older")])), seed=[local("a", L0)])
    native("lww-all-fields-update", pull_json, one(body([mem(
        "a", T2, embedding=[], embedding_source="src", embedding_model="mod", embedding_quantization="q",
        content_hash="given", updated_by="bob", updated_session="s2", consolidation_status="archived", importance=0.1,
        decay_factor=0.25, retired_at=T1, tier="working", expires_at=T2, access_count=3, last_access=T1,
        prev_access=T0, review_status="staged", kind="feedback", metadata={"z": "1"})])), seed=[local("a", L0)])

    # --- tombstones ---------------------------------------------------------------------
    for name, tombstone, remote_time in [
            ("newer-tombstone-blocks", L2, T1), ("equal-tombstone-blocks", L1, T1), ("older-tombstone-allows", L0, T1),
            ("nano-tombstone-blocks", "2026-10-26 02:00:00.000000001 +0000 UTC", T1),
            ("nano-tombstone-allows", "2026-10-26 01:59:59.999999999 +0000 UTC", T1),
            ("zero-tombstone-allows", "0001-01-01 00:00:00 +0000 UTC", T0)]:
        native("tombstone-" + name, [pull_json, pull], one(body([mem("a", remote_time)])), seed=[tomb("a", tombstone)])
    native("tombstone-other-id-does-not-block", pull_json, one(body([mem("a", T0)])), seed=[tomb("b", L2)])
    native("tombstone-then-local-recreated-allows", pull_json, one(body([mem("a", T2)])),
           seed=[tomb("a", L2), local("a", L0)])
    native("tombstone-with-local-row-blocks-update", pull_json, one(body([mem("a", T1, content="blocked")])),
           seed=[local("a", L0), tomb("a", L2)])
    native("tombstone-trigger-spelling-blocks", [pull_json, pull_json],
           nth([body([mem("a", LONG_AGO)], [gone("a", "2000-01-02T00:00:00Z")]),
                body([mem("a", "2001-01-01T00:00:00Z", content="again")])]))
    native("tombstone-trigger-spelling-allows-newer", [pull_json, pull_json],
           nth([body([mem("a", LONG_AGO)], [gone("a", "2000-01-02T00:00:00Z")]),
                body([mem("a", FAR, content="again")])]))

    # --- remote deletes ---------------------------------------------------------------------
    for name, local_time, deleted_at in [
            ("local-older", L0, T1), ("local-equal", L1, T1), ("local-newer", L2, T1),
            ("nano-older", "2026-10-26 02:00:00.5 +0000 UTC", "2026-10-26T02:00:00.500000001Z"),
            ("nano-newer", "2026-10-26 02:00:00.500000001 +0000 UTC", "2026-10-26T02:00:00.5Z"),
            ("zero-deleted-at", L1, "0001-01-01T00:00:00Z")]:
        native("delete-" + name, [pull_json, pull], one(body([], [gone("a", deleted_at)])), seed=[local("a", local_time)])
    # dcddcef0 loses absent-row tombstones (#854, fixed by Go PR #970).
    # Keep all observations exact except for the one required retained event.
    native("delete-missing-id", pull_json, one(body([], [gone("nope")])),
           known_go_defect="854-absent-tombstone")
    native("delete-missing-deleted-at", pull_json, one(body([], ['{"id":"a"}'])), seed=[local("a", L0)])
    native("delete-unicode-id", pull_json, one(body([], [gone("\u00fcn\u00ef\u20ac")])),
           seed=[local("\u00fcn\u00ef\u20ac", L0)])
    native("delete-three-ids", pull_json, one(body([], [gone("a"), gone("b", T0), gone("c")])),
           seed=[local("a", L0), local("b", L2), local("c", L1)])
    native("delete-same-id-twice", pull_json, one(body([], [gone("a"), gone("a")])), seed=[local("a", L0)])
    native("delete-unknown-keys-ignored", pull_json,
           one(body([], ['{"id":"a","deleted_at":"%s","by":"x"}' % T1])), seed=[local("a", L0)])
    native("delete-memory-with-metadata", pull_json, one(body([], [gone("a")])),
           seed=[local("a", L0, metadata='{"k":"v"}')])

    # --- mixed batches --------------------------------------------------------------------------
    native("mixed-batch", pull_json,
           one(body([mem("new1", T1), mem("old", T2, content="upd"), mem("skip", T0)],
                    [gone("del"), gone("keep", T0), gone("missing")])),
           seed=[local("old", L0), local("skip", L2), local("del", L0), local("keep", L2)])
    native("mixed-memory-then-delete-same-id", pull_json, one(body([mem("a", T0)], [gone("a", T1)])))
    native("mixed-memory-then-delete-same-id-older", pull_json, one(body([mem("a", T1)], [gone("a", T0)])))
    native("mixed-delete-then-memory-next-page", pull_json,
           pages({None: body([], [gone("a", T1)], cursor="p2"), "p2": body([mem("a", FAR, content="back")])}),
           seed=[local("a", L0)])
    native("mixed-memory-delete-memory-pages", pull_json,
           pages({None: body([mem("a", LONG_AGO)], [gone("a", "2000-01-02T00:00:00Z")], cursor="p2"),
                  "p2": body([mem("a", "2001-01-01T00:00:00Z", content="blocked")])}))

    # --- pagination -------------------------------------------------------------------------------
    native("pages-two", pull_json,
           pages({None: body([mem("a")], server="2026-10-26T01:10:00Z", cursor="c2"),
                  "c2": body([mem("b", T1)], [gone("a", T2)], server="2026-10-26T01:05:00Z")}))
    native("pages-two-table", pull, pages({None: body([mem("a")], cursor="c2"), "c2": body([mem("b", T1)])}))
    native("pages-three-with-empty-middle", pull_json,
           pages({None: body([mem("a")], cursor="c2"), "c2": body([], [], cursor="c3"), "c3": body([mem("b", T1)])}))
    native("pages-ten", pull_json,
           pages(dict([(None if page == 0 else "c%d" % page,
                        body([mem("m%d" % page, "2026-10-26T01:%02d:00Z" % page)],
                             cursor=("c%d" % (page + 1)) if page < 9 else None)) for page in range(10)])))
    native("pages-twenty-with-token", step("--pull", "--json", "--token=page-token"),
           pages(dict([(None if page == 0 else "c%d" % page,
                        body([mem("m%d" % page, "2026-10-26T02:%02d:00Z" % page)],
                             cursor=("c%d" % (page + 1)) if page < 19 else None)) for page in range(20)])))
    for index, cursor in enumerate(["a b", "a+b", "a/b?c=d&e", "\u00e4\u20ac", "%41", "~._-", "x" * 1000, "!*'()", ";:@$,",
                                    "\u0001ctl", "\U0001F600", "UPPER lower 123", "=", "&", "\\", "\"q\"", "<tag>"]):
        native("pages-cursor-%02d" % index, pull_json,
               pages({None: body([mem("a")], cursor=cursor), cursor: body([mem("b", T1)])}))
    native("pages-cursor-null-ends", pull_json, one(body([mem("a")], extra=',"next_cursor":null')))
    native("pages-cursor-empty-ends", pull_json, one(body([mem("a")], cursor="")))
    native("pages-stored-cursor-sent-as-since-on-first-page-only", pull_json,
           pages({None: body([mem("a", T2)], cursor="c2"), "c2": body([mem("b", T2)])}), seed=[("cursor", L0)])
    native("pages-same-cursor-repeated", pull_json,
           nth([body([mem("a", T0)], cursor="same"), body([mem("a", T1, content="p2")], cursor="same"),
                body([mem("a", T2, content="p3")])]))
    native("pages-server-time-decreases", pull_json,
           pages({None: body([mem("a")], server="2026-10-26T09:00:00Z", cursor="c2"),
                  "c2": body([mem("b", T1)], server="2026-10-26T01:00:00Z")}))
    native("pages-second-page-newest", pull_json,
           pages({None: body([mem("a")], server="2026-10-26T01:00:00Z", cursor="c2"),
                  "c2": body([mem("b", T1)], server="2026-10-26T05:00:00Z")}))
    native("pages-second-run-uses-stored-cursor", [pull_json, pull_json],
           nth([body([mem("a")], server="2026-10-26T01:10:00.7Z", cursor="c2"), body([mem("b", T1)], server=SERVER),
                body([], server=SERVER)]))
    native("pages-error-second-page-keeps-first", [pull_json, pull_json],
           pages({None: body([mem("a")], cursor="c2"), "c2": json_reply(500, '{"error":"boom","code":"E500"}')}))
    native("pages-error-third-page-keeps-two", [pull_json, pull],
           pages({None: body([mem("a")], cursor="c2"), "c2": body([mem("b", T1)], cursor="c3"),
                  "c3": json_reply(503, '{"error":"busy"}')}))
    native("pages-error-recovers-next-run", [pull_json, pull_json],
           nth([body([mem("a")], cursor="c2"), json_reply(500, '{"error":"x"}'), body([mem("a", T1, content="x")])]))
    native("pages-invalid-json-second-page", pull_json,
           pages({None: body([mem("a")], cursor="c2"), "c2": json_reply(200, b"<html>")}))
    native("pages-empty-body-second-page", pull_json,
           pages({None: body([mem("a")], cursor="c2"), "c2": json_reply(200, b"")}))
    native("pages-truncated-second-page", pull_json,
           pages({None: body([mem("a")], cursor="c2"),
                  "c2": [("send", b"HTTP/1.1 200 X\r\nContent-Length: 50\r\n\r\n" + b'{"server_time"'), ("close",)]}))
    native("pages-unknown-cursor-status", pull_json,
           nth([body([mem("a")], cursor="c2"), json_reply(404, '{"error":"gone cursor"}')]))
    native("pages-timeout-on-second-page", step("--pull", "--json", "--timeout=1s"),
           lambda request: ([("send", json_reply(200, body([mem("a")], cursor="c2")))]
                            if "cursor=" not in request["target"] else [("sleep", 4), ("send", json_reply(200, body([])))]))

    # --- server time and the stored cursor ------------------------------------------------------------
    native("clock-memory-newer-than-server", pull_json, one(body([mem("a", T2)], server=T0)))
    native("clock-server-newer-than-memory", pull_json, one(body([mem("a", T0)], server=T2)))
    native("clock-server-absent", pull_json, one(body([mem("a", T1)], server=None)))
    native("clock-delete-newest", pull_json, one(body([mem("a", T0)], [gone("b", T2)], server=T1)))
    native("clock-zero-everything", pull_json,
           one(body([mem("a", "0001-01-01T00:00:00Z")], server="0001-01-01T00:00:00Z")))
    native("clock-nano", pull_json, one(body([mem("a", "2026-10-26T01:00:00.123456789Z")], server=None)))
    native("clock-year-9999", pull_json, one(body([mem("a", "9999-12-31T23:59:59.999999999Z")], server=None)))
    native("clock-second-run-since-floors-to-seconds", [pull_json, pull_json],
           nth([body([mem("a", "2026-10-26T01:00:00.999999999Z")], server=None), body([])]))
    native("clock-two-runs-with-data", [pull_json, pull_json],
           nth([body([mem("a", T0)]), body([mem("a", T1, content="second")])]))
    native("clock-stored-cursor-newer-than-reply", [pull_json, pull_json], one(body([mem("a", T0)], server=T0)),
           seed=[("cursor", FUTURE)])

    # --- both directions: native only when the pull provokes no push ---------------------------------------
    native("both-data-older-than-cursor", [step(), step("--pull", "--json")],
           one(body([mem("a", T0)], [gone("b", T0)], server=T0)), seed=[("cursor", FUTURE), local("b", L0)])
    native("both-excluded-memory-is-not-pushed", step(),
           one(body([mem("a", T2, metadata={"sync_exclude": "true"})], server=T2)), seed=[("cursor", L0)])
    native("both-pages-older-than-cursor", step("--json"),
           pages({None: body([mem("a", T0)], cursor="c2"), "c2": body([mem("b", T0)])}), seed=[("cursor", FUTURE)])
    native("both-empty-pages-with-cursor", step("--json"), pages({None: body([], cursor="c2"), "c2": body([])}))
    native("both-failure-after-data-does-not-push", step("--json"),
           pages({None: body([mem("a", T2)], cursor="c2"), "c2": json_reply(500, '{"error":"x"}')}))

    # =============================== hand-offs =====================================================
    hand("handoff-both-new-memory-would-be-pushed", step(), one(body([mem("a", T2)])), seed=[("cursor", L0)])
    hand("handoff-both-new-memory-zero-cursor", step(), one(body([mem("a")])))
    hand("handoff-both-delete-creates-tombstone", step("--json"), one(body([], [gone("a", T2)])),
         seed=[("cursor", L0), local("a", L0)])
    hand("handoff-both-second-page-provokes-push", step("--json"),
         pages({None: body([mem("a", T0)], cursor="c2"), "c2": body([mem("b", T2)])}), seed=[("cursor", L1)])
    hand("handoff-both-update-would-be-pushed", step(), one(body([mem("a", T2)])),
         seed=[("cursor", L0), local("a", L0)])
    entries = [
        '{"id":5}', '{"id":null}', '{"id":""}', '{}', '{"content":"no id"}', 'null', '"text"', '5', '[]', 'true',
        '{"id":"a","content":5}', '{"id":"a","scope":["x"]}', '{"id":"a","metadata":{"k":5}}',
        '{"id":"a","metadata":{"k":null}}', '{"id":"a","metadata":[]}', '{"id":"a","metadata":"x"}',
        '{"id":"a","importance":"1"}', '{"id":"a","importance":1e999}', '{"id":"a","importance":true}',
        '{"id":"a","access_count":1.5}', '{"id":"a","access_count":1e2}', '{"id":"a","access_count":9223372036854775808}',
        '{"id":"a","access_count":"3"}', '{"id":"a","decay_factor":"x"}',
        '{"id":"a","updated_at":"bad"}', '{"id":"a","updated_at":5}',
        '{"id":"a","updated_at":"2026-10-26T03:00:00+02:00"}', '{"id":"a","updated_at":"2026-10-26t03:00:00Z"}',
        '{"id":"a","updated_at":"2026-10-26T03:00:00.1234567890Z"}', '{"id":"a","updated_at":"0000-01-01T00:00:00Z"}',
        '{"id":"a","created_at":"2026-10-26 03:00:00"}', '{"id":"a","valid_from":"x"}', '{"id":"a","expires_at":5}',
        '{"ID":"a"}', '{"Id":"a"}', '{"id":"a","Content":"x"}', '{"id":"a","id":"b"}',
        '{"id":"a","metadata":{},"metadata":{"k":"v"}}',
        '{"id":"a","embedding":[0.5]}', '{"id":"a","embedding":"x"}', '{"id":"a","embedding":[1,2,3]}',
        '{"id":"a","entities":[1]}', '{"id":"a","entities":"x"}', '{"id":"a","evidence":[{"text":"t"}]}',
        '{"id":"a","evidence":5}', '{"id":"a","consolidated_into_id":"missing-parent"}',
        '{"id":"a","content":"\\ud800"}', '{"id":"a","content":"\\udc00x"}', '{"id":"a\\u0000b"}',
        '{"id":"a","content":"x\\u0000y"}', '{"id":"a","metadata":{"k\\u0000":"v"}}',
        '{"id":"a","metadata":{"k":"v\\u0000"}}']
    for index, entry in enumerate(entries):
        hand("handoff-entry-%02d" % index, pull_json, one(body([entry])))
    hand("handoff-entry-after-good-one", pull_json, one(body([mem("good"), '{"id":5}'])))
    hand("handoff-entry-on-second-page", pull_json,
         pages({None: body([mem("good")], cursor="c2"), "c2": body(['{"id":5}'])}))
    hand("handoff-entry-bad-utf8", pull_json, one(b'{"memories":[{"id":"a","content":"\xff\xfe"}]}'))
    hand("handoff-entry-control-char-in-string", pull_json, one(b'{"memories":[{"id":"a","content":"x\ty"}]}'))
    tombstones = ['{"id":5}', 'null', '{"id":"a","deleted_at":"bad"}', '{"id":"a","deleted_at":5}',
                  '{"id":"a","deleted_at":"2026-10-26T03:00:00+02:00"}', '{}', '{"deleted_at":"%s"}' % T0,
                  '{"ID":"a"}', '{"id":"a","id":"b"}', '[]', '"a"', '{"id":""}']
    for index, entry in enumerate(tombstones):
        hand("handoff-tombstone-%02d" % index, pull_json, one(body([], [entry])))
    hand("handoff-memories-object", pull_json, one('{"memories":{}}'))
    hand("handoff-memories-string", pull_json, one('{"memories":"x"}'))
    hand("handoff-deleted-number", pull_json, one('{"deleted":5}'))
    hand("handoff-duplicate-memories-key", pull_json,
         one('{"memories":[%s],"memories":[%s],"server_time":"%s"}' % (mem("a"), mem("b"), SERVER)))
    hand("handoff-memories-then-null", pull_json,
         one('{"memories":[%s],"memories":null,"server_time":"%s"}' % (mem("a"), SERVER)))
    hand("handoff-duplicate-cursor-key", pull_json,
         pages({None: '{"next_cursor":"c","next_cursor":"d"}', "d": body([])}))
    hand("handoff-cursor-number", pull_json, one('{"next_cursor":5}'))
    hand("handoff-cursor-too-long", pull_json,
         pages({None: body([mem("a")], cursor="x" * 3000), "x" * 3000: body([])}))
    hand("handoff-folded-memories-key", pull_json, one('{"Memories":[%s]}' % mem("a")))
    hand("handoff-time-offset-on-later-page", pull_json,
         pages({None: body([mem("a")], cursor="c2"), "c2": '{"server_time":"2026-10-26T03:02:03+02:00"}'}))
    hand("handoff-redirect-on-second-page", pull_json,
         lambda request: (reply(json_reply(200, body([])))
                          if request["target"].startswith("/moved")
                          else reply(http(302, b"", {"Location": "/moved"})) if "cursor=" in request["target"]
                          else reply(json_reply(200, body([mem("a")], cursor="c2")))))
    hand("handoff-gzip-on-second-page", pull_json,
         pages({None: body([mem("a")], cursor="c2"), "c2": http(200, b"\x1f\x8b", {"Content-Encoding": "gzip"})}))
    hand("handoff-closed-on-second-page", pull_json, first_then_close(body([mem("a")], cursor="c2")))
    hand("handoff-seventy-pages", step("--pull", "--json"),
         pages(dict([(None if page == 0 else "c%d" % page,
                      body([mem("m%d" % page, "2026-10-26T01:%02d:00Z" % (page % 60))],
                           cursor=("c%d" % (page + 1)) if page < 69 else None)) for page in range(70)])))
    hand("handoff-failing-statement-foreign-key", pull_json,
         one(body([mem("first"), mem("child", consolidated_into_id="no-such-parent")])))
    hand("handoff-stored-time-odd-spelling", pull_json, one(body([mem("a", T2)])),
         seed=[local("a", "2026-10-26 03:00:00 +0200 +0200")])
    hand("handoff-tombstone-odd-spelling", pull_json, one(body([mem("a", T2)])),
         seed=[tomb("a", "2026-10-26 03:00:00 +0200 +0200")])
    return cases
