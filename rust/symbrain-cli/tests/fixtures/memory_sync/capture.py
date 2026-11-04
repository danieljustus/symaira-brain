#!/usr/bin/env python3
"""Oracle capture and live differential for `symbrain memory sync`.

Usage:
  capture.py --go GO_BIN --out DIR            freeze the Go observations (needs Go)
  capture.py --go GO_BIN --rust RUST_BIN      run both, print every difference
  capture.py --rust RUST_BIN --fixtures DIR   replay frozen observations without Go

Each scenario is run against a scripted local fake server in a disposable
HOME/XDG root. The observation records every HTTP request the binary sent
(method, request target, relevant headers, body; relay blobs are decrypted
because their nonces are random), stdout, stderr, the exit status and the
database after the run. Only the frozen Go observations are oracle output.
"""
import argparse, struct, base64, gzip, hashlib, json, os, re, shutil, socket, sqlite3, subprocess, sys, threading, time
from datetime import datetime, timezone, timedelta
from cryptography.hazmat.primitives.ciphers.aead import AESGCM

ROOT = "/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/builds/symaira-brain/MEM-SYNC/work"
HEADERS = ["authorization", "content-type", "user-agent", "accept-encoding", "referer", "accept"]


# ---------------------------------------------------------------- scenarios
def mem_sql(id, content, updated, scope="global", created=None, metadata="{}"):
    created = created or updated
    q = lambda v: "'" + v.replace("'", "''") + "'"
    return (
        "INSERT INTO memories (id, content, scope, metadata, embedding, created_at, updated_at) VALUES "
        f"({q(id)}, {q(content)}, {q(scope)}, {q(metadata)}, '[]', {q(created)}, {q(updated)})"
    )


def gotime(iso):
    """RFC 3339 UTC -> the Time.String() form the Go driver stores."""
    d = datetime.strptime(iso, "%Y-%m-%dT%H:%M:%SZ")
    return d.strftime("%Y-%m-%d %H:%M:%S +0000 UTC")


def tomb_sql(id, ts):
    return f"INSERT INTO sync_oplog (op, memory_id, ts) VALUES ('delete', '{id}', '{gotime(ts)}')"


def rmem(id, content, updated, **kw):
    m = {
        "id": id,
        "content": content,
        "scope": kw.pop("scope", "global"),
        "metadata": kw.pop("metadata", {}),
        "created_at": kw.pop("created_at", updated),
        "updated_at": updated,
        "consolidation_status": "raw",
        "importance": 0.5,
        "review_status": "approved",
        "tier": "long_term",
        "access_count": 1,
    }
    m.update(kw)
    return m


def page(memories=(), deleted=(), server_time="2026-10-20T12:00:00Z", next_cursor=None):
    body = {"memories": list(memories), "deleted": list(deleted), "server_time": server_time}
    if next_cursor:
        body["next_cursor"] = next_cursor
    return json.dumps(body, separators=(",", ":"))


def resp(body="", status=200, headers=None, **kw):
    r = {"status": status, "body": body, "headers": headers or {"Content-Type": "application/json"}}
    r.update(kw)
    return r


T1, T2, T3 = "2026-10-10T08:00:00Z", "2026-10-15T09:30:00Z", "2026-10-18T17:45:10Z"
SEED_BASIC = [
    mem_sql("l-1", "local one", gotime(T1)),
    mem_sql("l-2", "local two", gotime(T2)),
    tomb_sql("gone-1", T3),
]
PASS = "correct horse"

S = []  # (name, spec)


def scen(name, steps, seed=None, note="", files=None):
    S.append({"name": name, "seed": seed or [], "steps": steps, "note": note, "files": files or {}})


def step(argv, responses=(), env=None, relay_passphrase=None):
    return {"argv": argv, "responses": list(responses), "env": env or {}, "passphrase": relay_passphrase}


R = ["--remote", "{base}"]
RJ = ["--json"]

# pulls -------------------------------------------------------------------
scen("pull_json", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page(
    [rmem("r-1", "remote one", T2), rmem("r-2", "remote two\nwith <html> & unicode é", T3, metadata={"k": "v", "a": "b"}, kind="note")],
    [{"id": "l-1", "deleted_at": T3}]))])], SEED_BASIC)
scen("pull_table", [step(["memory", "sync", *R, "--pull"], [resp(page([rmem("r-1", "remote one", T2)], [], "2026-10-20T12:00:00.123456789Z"))])], SEED_BASIC)
scen("pull_paged", [step(["memory", "sync", *R, "--pull", *RJ], [
    resp(page([rmem("r-1", "one", T1)], [], T2, next_cursor="cur 1&x")),
    resp(page([rmem("r-2", "two", T2)], [{"id": "never-seen", "deleted_at": T2}], T3)),
])], [])
scen("pull_cursor_persisted", [
    step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("r-1", "one", T1)], [], T3))]),
    step(["memory", "sync", *R, "--pull", *RJ], [resp(page([], [], "2026-10-21T00:00:00Z"))]),
])
scen("both_json", [step(["memory", "sync", *R, *RJ], [
    resp(page([rmem("r-1", "remote one", T2)], [], T3)),
    resp(json.dumps({"applied": 2, "skipped": 0, "deleted": 1})),
])], SEED_BASIC)
scen("both_table_nothing_to_push", [step(["memory", "sync", *R], [resp(page([], [], T3))])])
scen("pull_offset_zones", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page(
    [rmem("r-z", "zoned", "2026-10-12T10:00:00+02:00", created_at="2026-10-12T10:00:00.5+02:00")], [{"id": "l-1", "deleted_at": "2026-10-19T00:00:00+05:30"}], "2026-10-20T12:00:00-03:00"))])], SEED_BASIC)

# push --------------------------------------------------------------------
scen("push_json", [step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 2, "skipped": 0, "deleted": 1}))])], SEED_BASIC)
scen("push_table_camel_counts", [step(["memory", "sync", *R, "--push"], [resp(json.dumps({"applied": 1, "skippedInvalidScope": 3, "deleted": 0}))])], SEED_BASIC)
scen("push_second_run_empty", [
    step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 2, "deleted": 1}))]),
    step(["memory", "sync", *R, "--push", *RJ], []),
], SEED_BASIC)
big = [mem_sql(f"m-{i:03d}", f"bulk {i}", gotime("2026-10-01T00:00:00Z").replace("00:00:00", f"00:{i // 60:02d}:{i % 60:02d}")) for i in range(205)]
scen("push_batches", [step(["memory", "sync", *R, "--push", *RJ], [
    resp(json.dumps({"applied": 200, "deleted": 0})), resp(json.dumps({"applied": 5, "deleted": 0}))])], big)
scen("push_sync_exclude", [step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 1}))])], [
    mem_sql("keep", "keep me", gotime(T1)),
    mem_sql("secret", "do not sync", gotime(T2), metadata='{"sync_exclude":"true"}'),
    mem_sql("meta", "with metadata", gotime(T3), metadata='{"a":"b","z":"y"}', scope="project:x"),
])

# tombstones / conflicts ----------------------------------------------------
tomb_seed = [
    mem_sql("fresh-local", "local newer", gotime(T3)),
    mem_sql("stale-local", "local older", gotime(T1)),
    tomb_sql("dead-1", T2),
]
scen("tombstone_conflicts", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page(
    [rmem("dead-1", "old resurrect", T1), rmem("dead-1b", "n/a", T3), rmem("fresh-local", "remote older", T2), rmem("stale-local", "remote newer", T3)],
    [{"id": "fresh-local", "deleted_at": T2}, {"id": "stale-local", "deleted_at": T2}, {"id": "dead-1", "deleted_at": T1}, {"id": "dead-2", "deleted_at": T1}, {"id": "dead-2", "deleted_at": T1}],
))])], tomb_seed)
scen("tombstone_resurrect", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("dead-1", "back", T3)], [], T3))])], tomb_seed)
scen("remote_tombstone_retained_pages", [step(["memory", "sync", *R, "--pull", *RJ], [
    resp(page([], [{"id": "x-1", "deleted_at": T2}], T2, next_cursor="p2")),
    resp(page([rmem("x-1", "late older", T1)], [], T3)),
])])

# relay -------------------------------------------------------------------
_KEYS = {}


def relay_blob(payload, passphrase=PASS):
    salt, nonce = bytes(range(16)), os.urandom(12)
    if passphrase not in _KEYS:
        _KEYS[passphrase] = hashlib.pbkdf2_hmac("sha256", passphrase.encode(), salt, 600000, 32)
    key = _KEYS[passphrase]
    ct = AESGCM(key).encrypt(nonce, json.dumps(payload, separators=(",", ":")).encode(), None)
    return base64.b64encode(b"\x01" + salt + nonce + ct).decode()


def relay_page(blobs, server_time="2026-10-20T12:00:00Z"):
    return json.dumps({"blobs": blobs, "server_time": server_time}, separators=(",", ":"))


RR = ["--encrypted-relay", "--relay-passphrase", PASS]
scen("relay_pull", [step(["memory", "sync", *R, *RR, "--pull", *RJ], [resp(relay_page([
    {"id": "r-1", "updated_at": T2, "blob": relay_blob({"memory": rmem("r-1", "relay remote", T2)})},
    {"id": "tombstone:l-1", "updated_at": T3, "blob": relay_blob({"deleted": {"id": "l-1", "deleted_at": T3}})},
    {"id": "empty", "updated_at": T3, "blob": relay_blob({})},
]))])], SEED_BASIC)
scen("relay_push", [step(["memory", "sync", *R, *RR, "--push", *RJ], [resp(json.dumps({"stored": 3, "skipped": 0}))], relay_passphrase=PASS)], SEED_BASIC)
scen("relay_both_table_env", [step(["memory", "sync", *R, "--encrypted-relay"], [
    resp(relay_page([{"id": "r-1", "updated_at": T2, "blob": relay_blob({"memory": rmem("r-1", "relay remote", T2)})}])),
    resp(json.dumps({"stored": 2, "skipped": 1}))], env={"SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE": PASS}, relay_passphrase=PASS)], SEED_BASIC)
scen("relay_wrong_passphrase", [step(["memory", "sync", *R, *RR, "--pull"], [resp(relay_page([
    {"id": "r-1", "updated_at": T2, "blob": relay_blob({"memory": rmem("r-1", "x", T2)}, "other")}]))])])
scen("relay_corrupt_payload", [step(["memory", "sync", *R, *RR, "--pull"], [resp(relay_page([
    {"id": "r-1", "updated_at": T2, "blob": relay_blob("not an object")}]))])])
scen("relay_short_blob", [step(["memory", "sync", *R, *RR, "--pull"], [resp(relay_page([
    {"id": "r-1", "updated_at": T2, "blob": base64.b64encode(b"short").decode()}]))])])
scen("relay_bad_base64", [step(["memory", "sync", *R, *RR, "--pull"], [resp('{"blobs":[{"id":"r","updated_at":"%s","blob":"%%%%"}],"server_time":"%s"}' % (T1, T1))])])
scen("relay_paged_cursor", [
    step(["memory", "sync", *R, *RR, "--pull", *RJ], [resp(relay_page([
        {"id": f"r-{i}", "updated_at": "2026-10-11T00:%02d:%02dZ" % (i // 60, i % 60), "blob": relay_blob({"memory": rmem(f"r-{i}", "c", "2026-10-11T00:%02d:%02dZ" % (i // 60, i % 60))})}
        for i in range(500)])), resp(relay_page([]))]),
    step(["memory", "sync", *R, *RR, "--pull", *RJ], [resp(relay_page([]))]),
])

# auth / http errors --------------------------------------------------------
scen("auth_token_flag", [step(["memory", "sync", *R, "--pull", "--token", "tok-123", *RJ], [resp(page([], [], T3))])])
scen("auth_token_env", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([], [], T3))], env={"SYMBRAIN_MEMORY_SYNC_TOKEN": "env-tok"})])
scen("auth_flag_beats_env", [step(["memory", "sync", *R, "--push", "--token", "flag-tok", *RJ], [resp(json.dumps({"applied": 0}))], env={"SYMBRAIN_MEMORY_SYNC_TOKEN": "env-tok"})], SEED_BASIC)
scen("auth_failure_401", [step(["memory", "sync", *R, "--pull", "--token", "bad"], [resp(json.dumps({"error": "invalid token", "code": "unauthorized"}), 401)])])
scen("auth_failure_403_nocode", [step(["memory", "sync", *R, "--push"], [resp(json.dumps({"error": "forbidden"}), 403)])], SEED_BASIC)
scen("http_500_text", [step(["memory", "sync", *R, "--pull"], [resp("boom", 500, {"Content-Type": "text/plain"})])])
scen("http_300_is_success", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([], [], T2), 300)])])
scen("http_204_empty", [step(["memory", "sync", *R, "--pull"], [resp("", 204, {})])])
scen("http_304", [step(["memory", "sync", *R, "--pull"], [resp("", 304, {})])])
scen("http_429_json_code", [step(["memory", "sync", *R, "--pull"], [resp(json.dumps({"error": "slow down", "code": "rate_limited"}), 429)])])
scen("relay_http_error", [step(["memory", "sync", *R, *RR, "--push"], [resp(json.dumps({"error": "nope", "code": "bad"}), 400)], relay_passphrase=PASS)], SEED_BASIC)
scen("apply_http_error_after_pull", [step(["memory", "sync", *R], [resp(page([rmem("r-1", "kept", T2)], [], T3)), resp(json.dumps({"error": "full"}), 507)])], SEED_BASIC)

# malformed pages -----------------------------------------------------------
for name, body in {
    "malformed_not_json": "<html>oops</html>",
    "malformed_truncated": '{"memories":[{"id":"a","content":"b"',
    "malformed_empty_body": "",
    "malformed_wrong_types": '{"memories":"nope","deleted":[],"server_time":"2026-10-20T12:00:00Z"}',
    "malformed_bad_time": '{"memories":[],"deleted":[],"server_time":"yesterday"}',
    "malformed_memory_field_type": '{"memories":[{"id":5,"content":"x"}],"deleted":[],"server_time":"2026-10-20T12:00:00Z"}',
    "malformed_array_top": "[1,2,3]",
    "malformed_null_top": "null",
    "malformed_trailing_garbage": page([], [], T3) + "garbage{{",
    "malformed_unknown_fields": '{"memories":[],"deleted":[],"server_time":"2026-10-20T12:00:00Z","extra":{"a":[1,2]}}',
    "malformed_empty_memory_id": '{"memories":[{"id":"","content":"x","scope":"global","metadata":{},"created_at":"2026-10-10T08:00:00Z","updated_at":"2026-10-10T08:00:00Z"}],"deleted":[],"server_time":"2026-10-20T12:00:00Z"}',
    "malformed_missing_times": '{"memories":[{"id":"m1","content":"x"}],"deleted":[{"id":"d1"}]}',
    "malformed_metadata_nonstring": '{"memories":[{"id":"m1","content":"x","metadata":{"a":1},"updated_at":"2026-10-10T08:00:00Z"}],"deleted":[]}',
}.items():
    scen(name, [step(["memory", "sync", *R, "--pull", *RJ], [resp(body)])])
scen("malformed_null_memory_element", [step(["memory", "sync", *R, "--pull"], [resp('{"memories":[null],"deleted":[],"server_time":"2026-10-20T12:00:00Z"}')])])
scen("malformed_apply_response", [step(["memory", "sync", *R, "--push"], [resp("not json")])], SEED_BASIC)
scen("malformed_apply_array", [step(["memory", "sync", *R, "--push"], [resp("[]")])], SEED_BASIC)
scen("malformed_relay_response", [step(["memory", "sync", *R, *RR, "--push"], [resp("{")], relay_passphrase=PASS)], SEED_BASIC)

# transport -----------------------------------------------------------------
scen("net_connection_refused", [step(["memory", "sync", "--remote", "http://127.0.0.1:{closed_port}", "--pull"], [])])
scen("net_connection_refused_localhost", [step(["memory", "sync", "--remote", "http://localhost:{closed_port}", "--push"], [])], SEED_BASIC)
scen("net_drop_connection", [step(["memory", "sync", *R, "--pull"], [resp(action="drop")])])
scen("net_truncated_body", [step(["memory", "sync", *R, "--pull"], [resp(page([], [], T3), action="truncate")])])
scen("net_timeout_stall", [step(["memory", "sync", *R, "--pull", "--timeout", "1s"], [resp(page([], [], T3), action="stall", stall=3)])])
scen("net_timeout_stall_body", [step(["memory", "sync", *R, "--pull", "--timeout", "1s"], [resp(page([], [], T3), action="stall_body", stall=3)])])
scen("net_timeout_zero", [step(["memory", "sync", *R, "--pull", "--timeout", "0s"], [])])
scen("net_timeout_negative", [step(["memory", "sync", *R, "--pull", "--timeout=-5s"], [])])
scen("net_redirect_302_get", [step(["memory", "sync", *R, "--pull", "--token", "t", *RJ], [
    resp("", 302, {"Location": "/moved/api/sync/changes?limit=500"}), resp(page([], [], T3))])])
scen("net_redirect_307_post", [step(["memory", "sync", *R, "--push", "--token", "t", *RJ], [
    resp("", 307, {"Location": "/elsewhere"}), resp(json.dumps({"applied": 2, "deleted": 1}))])], SEED_BASIC)
scen("net_redirect_302_post", [step(["memory", "sync", *R, "--push", *RJ], [
    resp("", 302, {"Location": "{base}/landing"}), resp(json.dumps({"applied": 2, "deleted": 1}))])], SEED_BASIC)
scen("net_redirect_loop", [step(["memory", "sync", *R, "--pull"], [resp("", 302, {"Location": "/again"})] * 12)])
scen("net_redirect_no_location", [step(["memory", "sync", *R, "--pull"], [resp("", 302, {})])])
scen("net_redirect_bad_scheme", [step(["memory", "sync", *R, "--pull"], [resp("", 302, {"Location": "ftp://x/y"})])])
scen("net_gzip_response", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("r-1", "gz", T2)], [], T3), gzip=True)])])
scen("net_gzip_invalid", [step(["memory", "sync", *R, "--pull"], [resp("this is not gzip", headers={"Content-Type": "application/json", "Content-Encoding": "gzip"})])])
scen("net_chunked_response", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("r-1", "chunky", T2)], [], T3), chunked=True)])])
scen("net_content_length_lie", [step(["memory", "sync", *R, "--pull"], [resp(page([], [], T3), action="short_length")])])
scen("net_prefix_trailing_slash", [step(["memory", "sync", "--remote", "{base}/prefix//", "--pull", *RJ], [resp(page([], [], T3))])])
scen("net_userinfo_basic_auth", [step(["memory", "sync", "--remote", "http://user:p%40ss@127.0.0.1:{port}", "--pull", *RJ], [resp(page([], [], T3))])])
scen("net_fragment_swallows_path", [step(["memory", "sync", "--remote", "{base}/x#frag", "--pull", *RJ], [resp(page([], [], T3))])])
scen("net_query_in_remote", [step(["memory", "sync", "--remote", "{base}/x?a=b", "--pull", *RJ], [resp(page([], [], T3))])])
scen("net_big_response", [step(["memory", "sync", *R, "--pull"], [resp('{"memories":[],"deleted":[],"server_time":"%s","pad":"{pad}"}' % T3, pad=11 << 20)])])

# flags / config ------------------------------------------------------------
scen("flag_missing_remote", [step(["memory", "sync", "--pull"], [])])
scen("flag_relay_without_passphrase", [step(["memory", "sync", *R, "--encrypted-relay"], [])])
scen("flag_unexpected_argument", [step(["memory", "sync", *R, "stray"], [])])
scen("flag_unknown_flag", [step(["memory", "sync", *R, "--bogus"], [])])
scen("flag_bad_timeout", [step(["memory", "sync", *R, "--timeout", "soon"], [])])
scen("flag_help", [step(["memory", "sync", "-h"], [])])
scen("flag_allow_insecure_warning", [step(["memory", "sync", *R, "--allow-insecure-http", "--pull", *RJ], [resp(page([], [], T3))])])
scen("flag_db_override", [step(["memory", "sync", *R, "--db", "{root}/custom/other.db", "--pull", *RJ], [resp(page([rmem("r-1", "x", T2)], [], T3))])])
scen("flag_db_unopenable", [step(["memory", "sync", *R, "--db", "{root}/missing-dir/no/other.db", "--pull"], [])])
scen("flag_pull_and_push_explicit", [step(["memory", "sync", *R, "--pull", "--push", *RJ], [resp(page([], [], T3)), resp(json.dumps({"applied": 2, "deleted": 1}))])], SEED_BASIC)
scen("flag_output_json_long", [step(["memory", "sync", *R, "--output", "json", "--pull"], [resp(page([], [], T3))])])
scen("flag_bad_stored_cursor", [step(["memory", "sync", *R, "--pull"], [])], ["INSERT INTO sync_state (remote, last_sync) VALUES ('{base}', 'not a time')"])

# richer rows, config, tokens ----------------------------------------------------
full = rmem("full-1", "everything set", T2, embedding=[0.25, -0.5, 0.125], embedding_source="ollama", embedding_model="nomic",
            embedding_quantization="f32", content_hash="abc123", created_by="alice", updated_by="bob", created_session="s1", updated_session="s2",
            consolidation_status="consolidated", consolidated_into_id="", importance=0.9, valid_from="2026-09-01T00:00:00Z", valid_to="2027-01-01T00:00:00+01:00",
            superseded_by="", kind="decision", review_status="staged", decay_factor=0.0, retired_at="2026-10-30T00:00:00Z", tier="working",
            expires_at="2026-12-01T00:00:00Z", access_count=7, last_access="2026-10-19T00:00:00Z", prev_access="2026-10-18T00:00:00Z", metadata={"k": "v", "é": "<&>"})
scen("pull_full_memory", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([full], [], T3))])])
scen("pull_full_memory_quantize", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([full], [], T3))])],
     files={"config/symmemory/config.toml": "[hybrid_search]\nquantize_to_binary = true\n"})
scen("pull_audit_disabled", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("l-1", "newer remote", T3), rmem("n-1", "new", T3)], [{"id": "l-2", "deleted_at": T3}], T3))])], SEED_BASIC,
     files={"config/symmemory/config.toml": "[retention]\naudit_log_enabled = false\n"})
scen("pull_audit_enabled", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("l-1", "newer remote", T3), rmem("n-1", "new", T3)], [{"id": "l-2", "deleted_at": T3}], T3))])], SEED_BASIC)
scen("pull_invalid_config_ignored", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("n-1", "new", T3)], [], T3))])],
     files={"config/symmemory/config.toml": "[hybrid_search]\nquantize_to_binary = \"maybe\"\n"})
scen("pull_db_from_config", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("n-1", "new", T3)], [], T3))])],
     files={"config/symmemory/config.toml": "[database]\npath = \"{root}/custom/cfg.db\"\n"})
scen("pull_update_existing", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("l-1", "edited remotely", T3, metadata={"a": "b"}, importance=0.1, tier="working", kind="note")], [], T3))])], SEED_BASIC)
scen("pull_equal_timestamp_not_applied", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("l-1", "same time", T1)], [], T3))])], SEED_BASIC)
scen("pull_stored_cursor_since", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([], [], T3))])],
     ["INSERT INTO sync_state (remote, last_sync) VALUES ('{base}', '2026-10-12 07:08:09.123456789 +0200 CEST')"])
scen("pull_stored_cursor_iso", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([], [], T3))])],
     ["INSERT INTO sync_state (remote, last_sync) VALUES ('{base}', '2026-10-12T07:08:09Z')"])
scen("push_stored_cursor_filters", [step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 1}))])],
     SEED_BASIC + ["INSERT INTO sync_state (remote, last_sync) VALUES ('{base}', '" + gotime(T2) + "')"])
scen("push_iso_updated_at", [step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 1}))])],
     [mem_sql("iso-1", "iso time", "2026-10-10T08:00:00Z"), mem_sql("iso-2", "iso frac", "2026-10-10T08:00:00.5+02:00")])
scen("push_full_row", [step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 1}))])], [
    "INSERT INTO memories (id, content, scope, metadata, embedding, created_at, updated_at, created_by, updated_by, created_session, updated_session, consolidation_status, importance, valid_from, valid_to, superseded_by, tier, expires_at, access_count, last_access, prev_access, review_status, kind, decay_factor, retired_at, embedding_source, embedding_model, embedding_quantization, content_hash, embedding_dim) VALUES ('full-l', 'every <field> & \"quoted\" \u00e9\\n', 'project:p', '{\"b\":\"2\",\"a\":\"1\"}', '[0.5,0.25]', '" + gotime(T1) + "', '" + gotime(T2) + "', 'alice', 'bob', 's1', 's2', 'consolidated', 0.75, '" + gotime(T1) + "', '" + gotime(T3) + "', 'other', 'working', '" + gotime(T3) + "', 5, '" + gotime(T2) + "', '" + gotime(T1) + "', 'staged', 'note', 0.5, '" + gotime(T3) + "', 'src', 'mod', 'f32', 'hash', 2)"])
scen("relay_push_batches", [step(["memory", "sync", *R, *RR, "--push", *RJ], [resp(json.dumps({"stored": 200})), resp(json.dumps({"stored": 5, "skipped": 0}))], relay_passphrase=PASS)], big)
scen("relay_push_full_row", [step(["memory", "sync", *R, *RR, "--push", *RJ], [resp(json.dumps({"stored": 1}))], relay_passphrase=PASS)], [
    mem_sql("only", "relay row", gotime(T1), metadata='{"k":"v"}'), tomb_sql("gone-9", T2)])
scen("relay_pull_cursor_query", [step(["memory", "sync", *R, *RR, "--pull", *RJ], [resp(relay_page([], T3))])],
     ["INSERT INTO sync_state (remote, last_sync) VALUES ('{base}', '2026-10-12 07:08:09.123456789 +0000 UTC')"])
scen("relay_server_time_precedence", [step(["memory", "sync", *R, *RR, *RJ], [resp(relay_page([], T1)), resp(json.dumps({"stored": 2}))], relay_passphrase=PASS)], SEED_BASIC)
scen("token_symvault_literal", [step(["memory", "sync", *R, "--pull", "--token", "symvault://secret/path"], [resp(page([], [], T3))])])
scen("token_newline_env", [step(["memory", "sync", *R, "--pull"], [resp(page([], [], T3))], env={"SYMBRAIN_MEMORY_SYNC_TOKEN": "bad\ntoken"})])
scen("token_with_space", [step(["memory", "sync", *R, "--pull", "--token", " spaced "], [resp(page([], [], T3))])])
scen("passphrase_env_flag_precedence", [step(["memory", "sync", *R, "--encrypted-relay", "--relay-passphrase", PASS, "--pull"], [resp(relay_page([]))], env={"SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE": "ignored"})])
scen("relay_empty_passphrase_env", [step(["memory", "sync", *R, "--encrypted-relay", "--pull"], [], env={"SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE": ""})])
scen("env_db_path", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem("n-1", "new", T3)], [], T3))])],
     files={})

REFUSALS2 = [
    "http://[::1]:{closed_port}", "http://[fe80::1%25lo0]:1/", "http://LOCALHOST:{closed_port}", "http://localhost.:1", "HTTPS", "https://[::1",
    "http://[::1]x", "http://[v1.x]/", "http://[::1]:", "http://127.0.0.1:/", "http://127.0.0.1:0/", "http://127.0.0.1:65535/", "http://user@127.0.0.1:{closed_port}", "http://:pw@127.0.0.1:{closed_port}",
    "http://127.0.0.1:{closed_port}?q=1", "http://127.0.0.1:{closed_port}/a b", "http://127.0.0.1:{closed_port}/%41", "http://127.0.0.1:{closed_port}/%", "https://127.0.0.1:{closed_port}",
    "http://\u00e9.test/", "\t", "http://127.0.0.1:{closed_port}/\u00e9", "http://127.0.0.1:{closed_port}#", "http://127.0.0.1:{closed_port}/?",
]
for i, r in enumerate(REFUSALS2):
    scen(f"url_{i:02d}", [step(["memory", "sync", "--remote", r, "--pull"], [])])

# argument spellings, odd content, numbers and times ----------------------------
scen("flag_single_dash_forms", [step(["memory", "sync", "-remote", "{base}", "-pull", "-json"], [resp(page([], [], T3))])])
scen("flag_equals_forms", [step(["memory", "sync", "--remote={base}", "--pull=true", "--push=false", "--output=json"], [resp(page([], [], T3))])])
scen("flag_global_json_first", [step(["--json", "memory", "sync", *R, "--pull"], [resp(page([], [], T3))])])
scen("flag_output_table_explicit", [step(["memory", "sync", *R, "--pull", "--output", "table"], [resp(page([], [], T3))])])
scen("flag_output_invalid", [step(["memory", "sync", *R, "--pull", "--output", "yaml"], [])])
scen("flag_timeout_forms", [step(["memory", "sync", *R, "--pull", "--timeout=1m30s", *RJ], [resp(page([], [], T3))])])
scen("flag_timeout_fraction", [step(["memory", "sync", *R, "--pull", "--timeout", "1.5s", *RJ], [resp(page([], [], T3))])])
scen("flag_double_dash", [step(["memory", "sync", *R, "--", "extra"], [])])
scen("flag_push_false_pull_false", [step(["memory", "sync", *R, "--pull=false", "--push=false", *RJ], [resp(page([], [], T3)), resp(json.dumps({"applied": 0}))])], SEED_BASIC)
odd = "line1\n\ttab \u2028 \u2029 \u0001 <b>&amp;</b> \U0001f600 \u00e9 \"q\" \\ back"
scen("push_odd_content", [step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 1}))])], [
    mem_sql("odd-1", odd, gotime(T1), metadata=json.dumps({"k\u00e9": "v\u2028<>&", "q\"": "\\"}))])
scen("push_float_importance", [step(["memory", "sync", *R, "--push", *RJ], [resp(json.dumps({"applied": 5}))])], [
    f"INSERT INTO memories (id, content, scope, metadata, embedding, created_at, updated_at, importance, decay_factor) VALUES ('f{i}', 'x', 'global', '{{}}', '[]', '{gotime(T1)}', '{gotime(T1)}', {v}, {d})"
    for i, (v, d) in enumerate([(1e-7, 1e21), (0.1 + 0.2, 0.30000000000000004), (123456789.123456789, 2.0), (1e21, 1e-5), (0.0, 0.00001234)])])
scen("pull_unicode_escapes", [step(["memory", "sync", *R, "--pull", *RJ], [resp(
    '{"memories":[{"id":"u1","content":"\\ud83d\\ude00 \\ud800 \\u00e9 \\/ \\b\\f","scope":"global","metadata":{},"created_at":"2026-10-10T08:00:00Z","updated_at":"2026-10-10T08:00:00Z"}],"deleted":[],"server_time":"2026-10-20T12:00:00Z"}')])])
scen("pull_time_forms", [step(["memory", "sync", *R, "--pull", *RJ], [resp(
    '{"memories":[{"id":"t1","content":"a","updated_at":"2026-10-10T08:00:00.123456789Z","created_at":"2026-10-10T08:00:00+05:45","valid_from":null,"valid_to":"2026-10-10T08:00:00.1-00:30"},'
    '{"id":"t2","content":"zero","updated_at":"0001-01-01T00:00:00Z"},{"id":"t3","content":"far","updated_at":"9999-12-31T23:59:59Z"}],"deleted":[{"id":"d1","deleted_at":"2026-10-11T00:00:00.5+01:00"}],"server_time":"2026-10-20T12:00:00.987654321+02:00"}')])])
scen("pull_time_lowercase_invalid", [step(["memory", "sync", *R, "--pull"], [resp('{"memories":[{"id":"t1","content":"a","updated_at":"2026-10-10t08:00:00z"}],"deleted":[],"server_time":"2026-10-20T12:00:00Z"}')])])
scen("pull_duplicate_keys_and_case", [step(["memory", "sync", *R, "--pull", *RJ], [resp(
    '{"MEMORIES":[{"ID":"c1","Content":"case","UPDATED_AT":"2026-10-10T08:00:00Z","id":"c1b","content":"dup1","content":"dup2"}],"Deleted":[],"Server_Time":"2026-10-20T12:00:00Z"}')])])
scen("pull_embedding_mismatch_types", [step(["memory", "sync", *R, "--pull"], [resp(
    '{"memories":[{"id":"e1","content":"a","embedding":["x"],"updated_at":"2026-10-10T08:00:00Z"}],"deleted":[],"server_time":"2026-10-20T12:00:00Z"}')])])
scen("pull_importance_string", [step(["memory", "sync", *R, "--pull"], [resp(
    '{"memories":[{"id":"e1","content":"a","importance":"high","access_count":1.5,"updated_at":"2026-10-10T08:00:00Z"}],"deleted":[],"server_time":"2026-10-20T12:00:00Z"}')])])
scen("pull_deleted_missing_time_zero", [step(["memory", "sync", *R, "--pull", *RJ], [resp('{"memories":[],"deleted":[{"id":"l-1"}],"server_time":"2026-10-20T12:00:00Z"}')])], SEED_BASIC)
scen("pull_many_rows_one_page", [step(["memory", "sync", *R, "--pull", *RJ], [resp(page([rmem(f"b-{i}", f"bulk {i}", "2026-10-10T08:%02d:00Z" % (i % 60)) for i in range(120)], [], T3))])])
scen("apply_counts_negative_and_float", [step(["memory", "sync", *R, "--push", *RJ], [resp('{"applied":-3,"deleted":2.5,"skipped":"x"}')])], SEED_BASIC)
scen("apply_counts_null", [step(["memory", "sync", *R, "--push", *RJ], [resp("null")])], SEED_BASIC)
scen("apply_counts_huge", [step(["memory", "sync", *R, "--push", *RJ], [resp('{"applied":99999999999999999999}')])], SEED_BASIC)
scen("relay_stored_string", [step(["memory", "sync", *R, *RR, "--push"], [resp('{"stored":"3"}')], relay_passphrase=PASS)], SEED_BASIC)
scen("relay_blob_null", [step(["memory", "sync", *R, *RR, "--pull", *RJ], [resp('{"blobs":[{"id":"x","updated_at":"2026-10-10T08:00:00Z","blob":null}],"server_time":"2026-10-20T12:00:00Z"}')])])
scen("relay_legacy_payload", [step(["memory", "sync", *R, *RR, "--pull", *RJ], [resp(relay_page([]))])])
scen("relay_tombstone_older_than_local", [step(["memory", "sync", *R, *RR, "--pull", *RJ], [resp(relay_page([
    {"id": "tombstone:l-2", "updated_at": T1, "blob": relay_blob({"deleted": {"id": "l-2", "deleted_at": T1}})},
    {"id": "l-1", "updated_at": T1, "blob": relay_blob({"memory": rmem("l-1", "same", T1)})}]))])], SEED_BASIC)

REFUSALS = [
    "ftp://x", "example.test", "http://example.test", "http://example.test:8080/x", "localhost:80", "http://", "http:///x", "https://",
    "://x", "ht tp://x", "http://[::1", "http://[::1]:99999x", "http://a b/", "http://exa%mple.com", "http://a%zz.com/", "http://user:pw@host@x/", "http://us er@host/",
    "http://h:80:90", "http://[1.2.3.4]/", "http://[zzz]/", "http://x\u0001y/", "http://x/\x7f", "HTTP://EXAMPLE.TEST", "http://127.0.0.1.example.test", "http://0x7f.0.0.1",
    "http://127.1", "http://[::ffff:127.0.0.1]:1/", "http://[::ffff:10.0.0.1]/", "mailto:a@b", "https:", "file:///etc/passwd", "http://192.168.0.1", "http://010.0.0.1/",
    "http://127.0.0.1:99999999999/", "http://localhost%00/", "http://ex\u00e4mple.test/", "http://[fe80::1%25en0]/", "javascript:alert(1)", "  http://x", "http://x/%zz",
]
for i, r in enumerate(REFUSALS):
    scen(f"refuse_{i:02d}", [step(["memory", "sync", "--remote", r, "--pull"], [])])
scen("refuse_allow_insecure_nonloopback_then_dns", [step(["memory", "sync", "--remote", "http://example.test", "--allow-insecure-http", "--pull"], [])], note="validation passes; the request fails to resolve")


# ------------------------------------------------------------------- engine
class Server:
    def __init__(self, responses, base_holder):
        self.responses = list(responses)
        self.requests = []
        self.lock = threading.Lock()
        self.sock = socket.socket()
        self.sock.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.sock.bind(("127.0.0.1", 0))
        self.sock.listen(16)
        self.port = self.sock.getsockname()[1]
        self.closed = False
        self.thread = threading.Thread(target=self.loop, daemon=True)
        self.thread.start()

    def loop(self):
        while not self.closed:
            try:
                conn, _ = self.sock.accept()
            except OSError:
                return
            threading.Thread(target=self.handle, args=(conn,), daemon=True).start()

    def read_request(self, conn):
        data = b""
        while b"\r\n\r\n" not in data:
            chunk = conn.recv(65536)
            if not chunk:
                return None
            data += chunk
        head, _, rest = data.partition(b"\r\n\r\n")
        lines = head.decode("latin-1").split("\r\n")
        method, target, _ = lines[0].split(" ", 2)
        headers = {}
        for line in lines[1:]:
            k, _, v = line.partition(":")
            headers[k.strip().lower()] = v.strip()
        body = rest
        if "content-length" in headers:
            need = int(headers["content-length"])
            while len(body) < need:
                chunk = conn.recv(65536)
                if not chunk:
                    break
                body += chunk
        elif headers.get("transfer-encoding", "").lower() == "chunked":
            buf = rest
            body = b""
            while True:
                while b"\r\n" not in buf:
                    buf += conn.recv(65536)
                size_line, _, buf = buf.partition(b"\r\n")
                size = int(size_line.split(b";")[0], 16)
                while len(buf) < size + 2:
                    buf += conn.recv(65536)
                body += buf[:size]
                buf = buf[size + 2:]
                if size == 0:
                    break
        return method, target, headers, body

    def handle(self, conn):
        try:
            conn.settimeout(15)
            req = self.read_request(conn)
            if req is None:
                return
            method, target, headers, body = req
            with self.lock:
                self.requests.append({"method": method, "target": target, "headers": headers, "body": body})
                r = self.responses.pop(0) if self.responses else resp(json.dumps({"error": "unscripted request"}), 500)
            self.reply(conn, r)
        except (OSError, ValueError):
            pass
        finally:
            try:
                conn.close()
            except OSError:
                pass

    def reply(self, conn, r):
        base = f"http://127.0.0.1:{self.port}"
        sub = lambda s: s.replace("{base}", base)
        action = r.get("action", "ok")
        if action == "drop":
            return
        body = sub(r["body"]).replace("{pad}", "x" * r.get("pad", 0)).encode()
        headers = {k: sub(v) for k, v in r["headers"].items()}
        if r.get("gzip"):
            body = gzip.compress(body)
            headers["Content-Encoding"] = "gzip"
        status = r["status"]
        reason = {200: "OK", 204: "No Content", 300: "Multiple Choices", 302: "Found", 304: "Not Modified", 307: "Temporary Redirect"}.get(status, "Status")
        head = f"HTTP/1.1 {status} {reason}\r\n"
        for k, v in headers.items():
            head += f"{k}: {v}\r\n"
        if action == "stall":
            time.sleep(r["stall"])
        if action == "short_length":
            head += f"Content-Length: {len(body) + 40}\r\nConnection: close\r\n\r\n"
            conn.sendall(head.encode() + body)
            return
        if action == "truncate":
            head += f"Content-Length: {len(body) + 100}\r\nConnection: close\r\n\r\n"
            conn.sendall(head.encode() + body[: len(body) // 2])
            return
        if action == "stall_body":
            head += f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n"
            conn.sendall(head.encode() + body[:5])
            time.sleep(r["stall"])
            return
        if r.get("chunked"):
            head += "Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n"
            out = head.encode()
            for i in range(0, len(body), 7):
                piece = body[i:i + 7]
                out += f"{len(piece):x}\r\n".encode() + piece + b"\r\n"
            out += b"0\r\n\r\n"
            conn.sendall(out)
            return
        head += f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n"
        conn.sendall(head.encode() + body)

    def close(self):
        self.closed = True
        self.sock.close()


def decrypt(blob_b64, passphrase):
    raw = base64.b64decode(blob_b64)
    if raw[0] == 1 and len(raw) >= 1 + 16 + 12 + 16:
        salt, nonce, ct = raw[1:17], raw[17:29], raw[29:]
    else:
        salt, nonce, ct = raw[:8], raw[8:20], raw[20:]
    key = hashlib.pbkdf2_hmac("sha256", passphrase.encode(), salt, 600000, 32)
    return AESGCM(key).decrypt(nonce, ct, None).decode()


def normalize_request(req, port, passphrase):
    out = {"method": req["method"], "target": req["target"]}
    h = {k: v for k, v in req["headers"].items() if k in HEADERS}
    host = req["headers"].get("host", "")
    h["host"] = host.replace(str(port), "{PORT}")
    if "referer" in h:
        h["referer"] = h["referer"].replace(str(port), "{PORT}")
    out["headers"] = h
    body = req["body"].decode("utf-8", "replace")
    if passphrase and "/api/sync/relay" in req["target"] and req["method"] == "POST":
        parsed = json.loads(body)
        blobs = []
        for b in parsed["blobs"]:
            blobs.append({"id": b["id"], "updated_at": b["updated_at"], "payload": decrypt(b["blob"], passphrase)})
        out["relay_blobs"] = blobs
    else:
        out["body"] = body.replace(str(port), "{PORT}")
    return out


def dump_db(path):
    if not os.path.exists(path):
        return None
    conn = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
    now = datetime.now(timezone.utc)
    def norm(v):
        if isinstance(v, float):
            return "real:" + struct.pack(">d", v).hex()
        if isinstance(v, bytes):
            return "hex:" + v.hex()
        if isinstance(v, str):
            m = re.match(r"^(\d{4}-\d\d-\d\d)[ T](\d\d:\d\d:\d\d)", v)
            if m:
                try:
                    d = datetime.strptime(m.group(1) + " " + m.group(2), "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc)
                    if abs(now - d) < timedelta(days=1):
                        return "<now>"
                except ValueError:
                    pass
        return v
    out = {}
    for table, order in [("memories", "id"), ("sync_state", "remote"), ("sync_oplog", "event_id")]:
        cur = conn.execute(f"SELECT * FROM {table} ORDER BY {order}")
        cols = [d[0] for d in cur.description]
        out[table] = [{c: norm(v) for c, v in zip(cols, row)} for row in cur]
    cur = conn.execute("SELECT action, memory_id, scope, session, actor, detail FROM audit_log ORDER BY action, memory_id")
    out["audit_log"] = [list(r) for r in cur]
    conn.close()
    return out


def sanitize(text, port, closed_port, root):
    text = text.replace(root, "{root}").replace(root.replace("/Volumes/1TB_NVMe_SN850X", "/Volumes/1TB_NVMe_SN850X"), "{root}")
    text = re.sub(r"goroutine \d+ \[running\]:[\s\S]*", "goroutine N [running]:\n<trace>\n", text)
    text = re.sub(r"\[signal [^\]]*\]", "[signal SIGSEGV]", text)
    text = text.replace(f":{closed_port}", ":{CLOSED}").replace(f":{port}", ":{PORT}")
    return text


def run_scenario(binary, scen_, tag, rust_env=None):
    root = os.path.join(ROOT, tag, scen_["name"])
    shutil.rmtree(root, ignore_errors=True)
    for d in ("home", "config", "data", "cache", "tmp", "project", "custom"):
        os.makedirs(os.path.join(root, d))
    env = {
        "PATH": "/usr/bin:/bin", "HOME": f"{root}/home", "XDG_CONFIG_HOME": f"{root}/config", "XDG_DATA_HOME": f"{root}/data",
        "XDG_CACHE_HOME": f"{root}/cache", "TMPDIR": f"{root}/tmp", "LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC",
    }
    env.update(rust_env or {})
    db = f"{root}/data/symbrain/memory/default.db"
    # create the schema with the binary under test
    subprocess.run([binary, "memory", "list"], env=env, cwd=f"{root}/project", capture_output=True, timeout=60)
    server = Server([], None)
    closed = socket.socket(); closed.bind(("127.0.0.1", 0)); closed_port = closed.getsockname()[1]; closed.close()
    base = f"http://127.0.0.1:{server.port}"
    if scen_["seed"] and os.path.exists(db):
        conn = sqlite3.connect(db)
        for sql in scen_["seed"]:
            conn.execute(sql.replace("{base}", base))
        conn.commit(); conn.close()
    for rel, content in scen_.get("files", {}).items():
        target = os.path.join(root, rel.replace("{root}", root))
        os.makedirs(os.path.dirname(target), exist_ok=True)
        open(target, "w").write(content)
    observations = []
    try:
        for st in scen_["steps"]:
            server.responses = list(st["responses"]); server.requests = []
            sub = lambda s: s.replace("{base}", base).replace("{port}", str(server.port)).replace("{closed_port}", str(closed_port)).replace("{root}", root)
            argv = [sub(a) for a in st["argv"]]
            senv = dict(env); senv.update(st["env"])
            t0 = time.time()
            p = subprocess.run([binary, *argv], env=senv, cwd=f"{root}/project", capture_output=True, timeout=180)
            elapsed = time.time() - t0
            time.sleep(0.05)
            obs = {
                "argv": st["argv"], "env": st["env"], "passphrase": st["passphrase"],
                "requests": [normalize_request(r, server.port, st["passphrase"]) for r in server.requests],
                "stdout": sanitize(p.stdout.decode("utf-8", "replace"), server.port, closed_port, root),
                "stderr": sanitize(p.stderr.decode("utf-8", "replace"), server.port, closed_port, root),
                "exit": p.returncode,
            }
            observations.append(obs)
    finally:
        server.close()
    dbdump = dump_db(db) if not any("--db" in s["argv"] for s in scen_["steps"]) else dump_db(f"{root}/custom/other.db")
    if dbdump:
        dbdump = json.loads(json.dumps(dbdump).replace(base, "http://127.0.0.1:{PORT}").replace(str(server.port), "{PORT}"))
    return {"name": scen_["name"], "seed": scen_["seed"], "note": scen_["note"], "files": scen_.get("files", {}), "steps": [
        {**{k: s[k] for k in ("argv", "env", "passphrase")}, "responses": spec["responses"]} for s, spec in zip(observations, scen_["steps"])
    ], "observed": observations, "db": dbdump}


def diff(a, b, path=""):
    out = []
    if type(a) != type(b):
        return [f"{path}: {a!r} != {b!r}"]
    if isinstance(a, dict):
        for k in sorted(set(a) | set(b)):
            if k not in a or k not in b:
                out.append(f"{path}.{k}: missing on one side ({a.get(k)!r} vs {b.get(k)!r})")
            else:
                out += diff(a[k], b[k], f"{path}.{k}")
    elif isinstance(a, list):
        if len(a) != len(b):
            out.append(f"{path}: len {len(a)} != {len(b)}\n   go: {a!r}\n   rs: {b!r}")
        else:
            for i, (x, y) in enumerate(zip(a, b)):
                out += diff(x, y, f"{path}[{i}]")
    elif a != b:
        out.append(f"{path}: {a!r} != {b!r}")
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--go"); ap.add_argument("--rust"); ap.add_argument("--out"); ap.add_argument("--fixtures")
    ap.add_argument("--only", default="")
    ap.add_argument("--missing", action="store_true", help="with --out: freeze only scenarios without a fixture")
    args = ap.parse_args()
    chosen = [s for s in S if args.only in s["name"]]
    if args.out:
        os.makedirs(args.out, exist_ok=True)
        for s in chosen:
            if args.missing and os.path.exists(os.path.join(args.out, s["name"] + ".json")):
                continue
            r = run_scenario(args.go, s, "go")
            json.dump(r, open(os.path.join(args.out, s["name"] + ".json"), "w"), indent=1, sort_keys=True, ensure_ascii=False)
            open(os.path.join(args.out, s["name"] + ".json"), "a").write("\n")
            print("froze", s["name"], [o["exit"] for o in r["observed"]])
        return
    bad = 0
    for s in chosen:
        if args.go:
            want = run_scenario(args.go, s, "go")
        else:
            want = json.load(open(os.path.join(args.fixtures, s["name"] + ".json")))
        got = run_scenario(args.rust, s, "rust")
        d = diff({"observed": want["observed"], "db": want["db"]}, {"observed": got["observed"], "db": got["db"]})
        if d:
            bad += 1
            print("DIFF", s["name"])
            for line in d[:12]:
                print("   ", line[:700])
        else:
            print("ok  ", s["name"])
    print("differences in", bad, "of", len(chosen))
    sys.exit(1 if bad else 0)


main()
