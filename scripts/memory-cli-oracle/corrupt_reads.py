"""Malformed stored JSON retains the exact Go error, never an empty native success."""
from contextlib import closing
import base64
import http.server
import json
from pathlib import Path
import sqlite3
import tempfile
import threading


def diagnostic_contract(go, without_go, verb, field, value, root):
    errors = {
        "broken": "invalid character 'b' looking for beginning of value",
        "": "unexpected end of JSON input",
        "[]": "json: cannot unmarshal array into Go value of type map[string]string",
        '{"n":1}': "json: cannot unmarshal number into Go value of type string",
        '{"n":1,"n":"later"}': "json: cannot unmarshal number into Go value of type string",
        "{}": "json: cannot unmarshal object into Go value of type []float32",
        '["x"]': "json: cannot unmarshal string into Go value of type float32",
        "[1e39]": "json: cannot unmarshal number 1e39 into Go value of type float32",
    }
    if value in ("null", '{"n":null}', "[null]", '{"n":"value"}'):
        expected = b""
    else:
        error = ("json: cannot unmarshal bool into Go value of type " +
                 ("map[string]string" if field == "metadata" else "[]float32")) if value == "false" else errors[value]
        operation = {"list": "list memories", "rules": "list rules", "search": "search memories"}[verb]
        expected = f"symbrain memory {verb}: {operation}: {error}\n".encode()
    go_matches = (go["exit"] == (1 if expected else 0)
                  and base64.b64decode(go["stderr"]) == expected)
    if value == '{"n":"value"}':
        return go_matches, without_go == go
    stderr = base64.b64decode(without_go["stderr"])
    absent = (without_go["exit"] == 1 and not base64.b64decode(without_go["stdout"])
              and stderr.startswith(f"symbrain: start Go fallback {root / 'absent-go'}: ".encode())
              and stderr.endswith(b"(os error 2)\n"))
    return go_matches, absent


def run(go, rust, output, snapshot, isolated_env):
    records = []

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            self.rfile.read(int(self.headers["Content-Length"]))
            body = json.dumps({"data": [{"index": 0, "embedding": [0.25] * 768}]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format, *args):
            pass

    metadata = ["broken", "", "[]", "false", '{"n":1}',
                '{"n":1,"n":"later"}', "null", '{"n":null}']
    inputs = [(verb, "metadata", value) for verb in ("list", "rules") for value in metadata]
    inputs += [("search", "metadata", value) for value in metadata[:6]]
    inputs += [("search", "embedding", value) for value in
               ("broken", "", "{}", "false", '["x"]', "[1e39]", "[null]")]
    # An otherwise valid store must still succeed without any Go fallback.
    inputs += [("list", "metadata", '{"n":"value"}'),
               ("rules", "metadata", '{"n":"value"}')]
    server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        for index, (verb, field, value) in enumerate(inputs):
            with tempfile.TemporaryDirectory(prefix="memory-cli-corrupt-") as temporary:
                root = Path(temporary)
                env = isolated_env(root)
                allowed = {"SystemRoot", "SYSTEMROOT", "WINDIR", "COMSPEC", "PATHEXT",
                           "TEMP", "TMP", "TMPDIR", "HOME", "USERPROFILE", "PATH",
                           "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME",
                           "XDG_STATE_HOME", "SYMBRAIN_GO_BINARY"}
                env = {key: value for key, value in env.items() if key in allowed}
                path = root / "memory.db"
                config = root / "config/symmemory/config.toml"
                config.parent.mkdir(parents=True)
                config.write_text(f'ollama.url="http://127.0.0.1:{server.server_port}/v1/embeddings"\n'
                                  'ollama.model="owned-test-model"\n')
                assert output(go, ["list", "--db", str(path)], root, env)["exit"] == 0
                if verb == "search":
                    seeded = output(go, ["set", "probe", "--kind", "reference", "--db", str(path)], root, env)
                    assert seeded["exit"] == 0, seeded
                with closing(sqlite3.connect(path)) as database, database:
                    if verb == "rules":
                        database.execute("INSERT INTO rules(id,content,scope,metadata,created_at,updated_at) "
                                         "VALUES('probe','probe','global',?,"
                                         "'2000-01-01 00:00:00 +0000 UTC',"
                                         "'2000-01-01 00:00:00 +0000 UTC')", ('{}',))
                    elif verb == "list":
                        database.execute("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) "
                                         "VALUES('probe','probe','global',?,'[]',"
                                         "'2000-01-01 00:00:00 +0000 UTC',"
                                         "'2000-01-01 00:00:00 +0000 UTC')", ('{}',))
                    # Fault injection only in this disposable fixture: the
                    # oplog's json_extract otherwise prevents malformed JSON
                    # from being stored. Restore every exact trigger before
                    # either real CLI process sees the database.
                    table = "rules" if verb == "rules" else "memories"
                    triggers = database.execute("SELECT name,sql FROM sqlite_schema "
                                                "WHERE type='trigger' AND tbl_name=? "
                                                "AND sql LIKE '%json_extract%' ORDER BY name",
                                                (table,)).fetchall()
                    for name, _ in triggers:
                        database.execute('DROP TRIGGER "' + name.replace('"', '""') + '"')
                    assert field in ("metadata", "embedding")
                    database.execute(f"UPDATE {table} SET {field}=?,"
                                     "created_at='2000-01-01 00:00:00 +0000 UTC',"
                                     "updated_at='2000-01-01 00:00:00 +0000 UTC'", (value,))
                    for _, sql in triggers:
                        database.execute(sql)
                    restored = database.execute("SELECT name,sql FROM sqlite_schema "
                                                "WHERE type='trigger' AND tbl_name=? "
                                                "AND sql LIKE '%json_extract%' ORDER BY name",
                                                (table,)).fetchall()
                    assert restored == triggers
                args = [verb, *(["probe"] if verb == "search" else []), "--db", str(path), "--json"]
                before = snapshot(root)
                with closing(sqlite3.connect(path)) as database:
                    before_triggers = database.execute("SELECT name,sql FROM sqlite_schema "
                                                       "WHERE type='trigger' ORDER BY name").fetchall()
                first = output(go, args, root, env)
                native_only = output(rust, args, root, env)
                valid = value == '{"n":"value"}'
                candidate_env = dict(env, SYMBRAIN_GO_BINARY=str(go)) if not valid else env
                second = output(rust, args, root, candidate_env)
                unchanged = before == snapshot(root)
                with closing(sqlite3.connect(path)) as database:
                    after_triggers = database.execute("SELECT name,sql FROM sqlite_schema "
                                                      "WHERE type='trigger' ORDER BY name").fetchall()
                trigger_definitions_unchanged = before_triggers == after_triggers
                # Go also accepts null string-map/vector values. Their exact
                # representation is deliberately retained on Go in this slice.
                go_contract, unavailable_fails = diagnostic_contract(
                    first, native_only, verb, field, value, root)
                records.append(dict(family="corrupt_reads", index=index, verb=verb,
                                    field=field, stored_json=value, go=first, rust=second,
                                    rust_without_go=native_only, fallback_available=not valid,
                                    native_positive_control=valid,
                                    database_state_unchanged=unchanged,
                                    trigger_definitions_unchanged=trigger_definitions_unchanged,
                                    go_contract_expected=go_contract,
                                    absent_fallback_fails_closed=unavailable_fails,
                                    match=first == second and unchanged and trigger_definitions_unchanged
                                    and go_contract and unavailable_fails))
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=5)
    return records
