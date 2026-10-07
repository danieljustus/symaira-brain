#!/usr/bin/env python3
"""Source-bound, actual-process historical repair comparison; no live user data."""

import argparse
from contextlib import closing
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
from provider import activate
activate()
import sqlite3
import stat
import subprocess
import time
from checker import preflight

FROZEN = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
CREATED = "2000-01-01 00:00:00 +0000 UTC"
SHADOWS = {"memories_fts", "memories_fts_config", "memories_fts_data",
           "memories_fts_docsize", "memories_fts_idx"}
UUID4 = re.compile(r"^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$")


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, value):
    path.write_bytes((json.dumps(value, indent=2, sort_keys=True) + "\n").encode())


def cell(value):
    return {"blob_hex": value.hex()} if isinstance(value, bytes) else value


def snapshot(path):
    with closing(sqlite3.connect(path)) as db:
        tables = {}
        schema = list(db.execute("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name"))
        for kind, name, _, definition in schema:
            if kind != "table":
                continue
            quoted = '"' + name.replace('"', '""') + '"'
            columns = list(db.execute("PRAGMA table_info(" + quoted + ")"))
            rows = [dict(zip([entry[1] for entry in columns], map(cell, row)))
                    for row in db.execute("SELECT * FROM " + quoted)]
            indexes = {}
            for index in db.execute("PRAGMA index_list(" + quoted + ")"):
                iq = '"' + index[1].replace('"', '""') + '"'
                raw_keys = list(db.execute("PRAGMA index_xinfo(" + iq + ")"))
                indexes[index[1]] = dict(unique=index[2], origin=index[3], partial=index[4],
                                        raw_keys=raw_keys, keys=[list(row[:1] + row[2:]) for row in raw_keys])
            tables[name] = dict(columns=columns, rows=sorted(rows, key=lambda row: json.dumps(row, sort_keys=True)),
                                foreign_keys=list(db.execute("PRAGMA foreign_key_list(" + quoted + ")")), indexes=indexes,
                                without_rowid=db.execute("SELECT wr FROM pragma_table_list WHERE schema='main' AND name=?", (name,)).fetchone()[0], sql=definition)
        return dict(tables=tables, schema=schema, integrity=db.execute("PRAGMA integrity_check").fetchone()[0], main_file_mode=stat.S_IMODE(path.stat().st_mode))


def fixture(path, resources, count):
    with closing(sqlite3.connect(path)) as db, db:
        db.execute("CREATE TABLE schema_migrations(version TEXT PRIMARY KEY,applied_at DATETIME DEFAULT CURRENT_TIMESTAMP)")
        for resource in resources[:count]:
            db.executescript(resource.read_text())
            db.execute("INSERT INTO schema_migrations(version,applied_at) VALUES (?,?)", (resource.stem, CREATED))
            db.commit()
        db.execute("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES ('legacy-memory','running runners historical repair','global','{}','[]',?,?)", (CREATED, CREATED))
        db.execute("INSERT INTO rules(id,content,scope,metadata,created_at) VALUES ('legacy-rule','retain rule','global','{}',?)", (CREATED,))
        if any(row[1] == "updated_at" for row in db.execute("PRAGMA table_info(rules)")):
            db.execute("UPDATE rules SET updated_at=?", (CREATED,))
        names = {row[0] for row in db.execute("SELECT name FROM sqlite_schema WHERE type='table'")}
        if "entities" in names:
            db.executemany("INSERT INTO entities(id,name,aliases,created_at,updated_at) VALUES (?,?,?,?,?)",
                           [("a", "Alpha", '["Al"]', CREATED, CREATED), ("b", "Beta", "[]", CREATED, CREATED)])
        if "entity_relations" in names:
            if any(row[1] == "id" for row in db.execute("PRAGMA table_info(entity_relations)")):
                db.executemany("INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_at,id,updated_at) VALUES ('a','b',?,?,?,?)",
                               [("first", CREATED, "00000000-0000-4000-8000-000000000001", CREATED),
                                ("second", CREATED, "00000000-0000-4000-8000-000000000002", CREATED)])
            else:
                db.executemany("INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_at) VALUES ('a','b',?,?)", [("first", CREATED), ("second", CREATED)])


def canonical(sql):
    parts = re.split(r"('(?:[^']|'')*'|\"(?:[^\"]|\"\")*\")", sql or "")
    result = "".join(part if index % 2 else re.sub(r"\s+", "", part).lower()
                     for index, part in enumerate(parts)).rstrip(";")
    return re.sub(r"^(create(?:uniqueindex|index|virtualtable|table|trigger))ifnotexists", r"\1", result)


def execute(binary, root, path):
    (root / "home").mkdir(exist_ok=True)
    environment = dict(HOME=str(root / "home"), USERPROFILE=str(root / "home"), PATH="", TZ="UTC", LANG="C.UTF-8",
                       SYMBRAIN_GO_BINARY=str(root / "absent-fallback"))
    for suffix in ("CONFIG", "DATA", "CACHE", "STATE"):
        environment["XDG_" + suffix + "_HOME"] = str(root / suffix.lower())
    # Windows process startup may need system paths; no user/config variables
    # are inherited. All Memory/Brain credentials and paths stay disposable.
    for key in ("SystemRoot", "WINDIR"):
        if key in os.environ:
            environment[key] = os.environ[key]
    start = time.time()
    process = subprocess.run([str(binary), "memory", "list", "--db", str(path), "--json"],
                             cwd=root, env=environment, capture_output=True, timeout=30, check=False)
    return dict(exit=process.returncode, stdout_hex=process.stdout.hex(), stderr_hex=process.stderr.hex(),
                start=start, end=time.time(), argv=[str(binary), "memory", "list", "--db", str(path), "--json"], environment=environment)


def inspect(path):
    with closing(sqlite3.connect(path)) as db:
        ids = [row[0] for row in db.execute("SELECT id FROM memories_fts WHERE memories_fts MATCH 'run' ORDER BY id")]
        db.execute("INSERT INTO memories_fts(memories_fts,rank) VALUES ('integrity-check',1)")
        return dict(run_ids=ids, foreign_key_errors=list(db.execute("PRAGMA foreign_key_check")))


def bind_rows(name, rows, initial, process, bindings):
    output = []
    initial_rows = {row.get("version"): row for row in initial.get("tables", {}).get("schema_migrations", {}).get("rows", [])}
    old_relations = initial.get("tables", {}).get("entity_relations", {}).get("rows", [])
    blank = {(row["from_entity_id"], row["to_entity_id"], row["relation_type"]) for row in old_relations if not row.get("id")}
    for row in rows:
        row = dict(row)
        if name == "schema_migrations" and row["version"] not in initial_rows:
            stamp = datetime.strptime(row["applied_at"], "%Y-%m-%d %H:%M:%S").replace(tzinfo=timezone.utc).timestamp()
            assert int(process["start"]) <= stamp <= int(process["end"]), row
            bindings.setdefault("new_ledger_timestamps", {})[row["version"]] = row["applied_at"]
            row["applied_at"] = {"new_timestamp_in_actual_process_interval": True}
        if name == "entity_relations":
            key = (row["from_entity_id"], row["to_entity_id"], row["relation_type"])
            if key in blank:
                assert UUID4.fullmatch(row["id"]), row
                bindings.setdefault("generated_relation_ids", {})[json.dumps(key)] = row["id"]
                row["id"] = {"generated_uuid4_for_unchanged_triple": list(key)}
        output.append(row)
    generated = bindings.get("generated_relation_ids", {}).values()
    assert len(set(generated)) == len(generated), bindings
    return sorted(output, key=lambda row: json.dumps(row, sort_keys=True))


def compare(before, go, native, go_process, native_process):
    assert go_process["exit"] == native_process["exit"] == 0
    for stream in ("stdout_hex", "stderr_hex"):
        assert go_process[stream] == native_process[stream], stream
    assert go["integrity"] == native["integrity"] == "ok"
    if os.name != "nt":
        assert go["main_file_mode"] == native["main_file_mode"] == 0o600
    bindings = {"go": {}, "native": {}}
    assert set(go["tables"]) == set(native["tables"]), "table identities"
    for name, expected in go["tables"].items():
        actual = native["tables"][name]
        assert expected["without_rowid"] == actual["without_rowid"], name
        assert sorted(expected["foreign_keys"]) == sorted(actual["foreign_keys"]), name
        # Approved flattened native column order is retained. Declarations are
        # compared by name; native/Go historical base columns stay unchanged.
        ec = {row[1]: list(row[2:]) for row in expected["columns"]}
        ac = {row[1]: list(row[2:]) for row in actual["columns"]}
        if name == "query_log" and ac.get("created_at", [None, None, None])[2] == "CURRENT_TIMESTAMP":
            ac["created_at"][2] = "datetime('now')"  # Precisely documented equivalent existing default.
        assert ec == ac, (name, ec, ac)
        for index, definition in expected["indexes"].items():
            comparable = {key: value for key, value in definition.items() if key != "raw_keys"}
            if definition["origin"] == "c":
                found = actual["indexes"].get(index, {})
                assert {key: value for key, value in found.items() if key != "raw_keys"} == comparable, (name, index)
            else:
                assert comparable in [{key: value for key, value in entry.items() if key != "raw_keys"} for entry in actual["indexes"].values()], (name, index)
        if name not in SHADOWS:
            er = bind_rows(name, expected["rows"], before, go_process, bindings["go"])
            ar = bind_rows(name, actual["rows"], before, native_process, bindings["native"])
            assert er == ar, (name, er, ar)
    go_objects = {row[1]: canonical(row[3]) for row in go["schema"] if row[0] in ("index", "trigger") and row[3]}
    native_objects = {row[1]: canonical(row[3]) for row in native["schema"] if row[0] in ("index", "trigger") and row[3]}
    assert set(native_objects) - set(go_objects) == {"idx_entity_relations_id"}
    for name, definition in go_objects.items():
        assert native_objects[name] == definition, name
    return bindings


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("go", "go-source", "native", "output"):
        parser.add_argument("--" + flag, type=Path, required=True)
    args = parser.parse_args()
    args.go, args.native, args.go_source, args.output = (path.resolve() for path in (args.go, args.native, args.go_source, args.output))
    repo = Path(__file__).resolve().parents[2]
    # A workflow may supply a git archive without .git. Each input is bound to
    # the immutable object in the real checkout, not an archive directory label.
    revision = FROZEN
    resources = sorted((args.go_source / "internal/memory/db/migrations").glob("*.sql"))
    assert len(resources) == 37
    owned = repo / "rust/symbrain-memory/src/migration/sql"
    for resource in resources:
        frozen_bytes = subprocess.check_output(["git", "show", FROZEN + ":internal/memory/db/migrations/" + resource.name], cwd=repo)
        assert (owned / resource.name).read_bytes() == resource.read_bytes() == frozen_bytes
    args.output.mkdir()  # Refuse reuse of earlier evidence/process roots.
    receipt = dict(frozen_go=revision, source=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo).decode().strip(),
                   dirty_source=subprocess.check_output(["git", "status", "--porcelain"], cwd=repo).decode(),
                   binaries={str(args.go): digest(args.go), str(args.native): digest(args.native)},
                   resources={str(path): digest(path) for path in resources}, python_sqlite=sqlite3.sqlite_version,
                   checker=preflight(), cases=[])
    save(args.output / "receipt.json", receipt)
    for count, resource in enumerate(resources, 1):
        root = args.output / resource.stem
        root.mkdir()
        seed = root / "seed.db"
        fixture(seed, resources, count)
        before = snapshot(seed)
        results = {}
        for label, binary in (("go", args.go), ("native", args.native)):
            child = root / label
            child.mkdir()
            database = child / "memory.db"
            shutil.copyfile(seed, database)
            process = execute(binary, child, database)
            result = dict(before=before, process=process, after=snapshot(database))
            save(child / "result.json", result)
            results[label] = result
        bindings = compare(before, results["go"]["after"], results["native"]["after"],
                           results["go"]["process"], results["native"]["process"])
        checks = {label: inspect(root / label / "memory.db") for label in results}
        assert checks["go"] == checks["native"] == dict(run_ids=["legacy-memory"], foreign_key_errors=[])
        child = root / "native"
        reopened = execute(args.native, child, child / "memory.db")
        after_reopen = snapshot(child / "memory.db")
        save(child / "reopen.json", dict(process=reopened, after=after_reopen))
        assert reopened["exit"] == 0
        assert reopened["stdout_hex"] == results["native"]["process"]["stdout_hex"]
        assert reopened["stderr_hex"] == results["native"]["process"]["stderr_hex"]
        assert after_reopen == results["native"]["after"], "all FTS shadow/app cells on reopen"
        receipt["cases"].append(dict(prefix=count, version=resource.stem, bindings=bindings, checks=checks))
        save(args.output / "receipt.json", receipt)
    receipt["files"] = {str(path.relative_to(args.output)): digest(path) for path in args.output.rglob("*") if path.is_file() and path.name != "receipt.json"}
    save(args.output / "receipt.json", receipt)
    print(json.dumps(dict(pairs=len(receipt["cases"]), native_reopens=len(receipt["cases"]))))


if __name__ == "__main__":
    main()
