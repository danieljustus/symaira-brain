#!/usr/bin/env python3
"""Owned SQL default census/invariants; not a compiled Go/Rust acceptance gate."""

import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import subprocess

FROZEN = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
BASE = "6ae73a7eef3442e2b4211a56043941339fb5a1d6"


def digest(raw):
    return hashlib.sha256(raw).hexdigest()


def schema(source):
    return re.search(r'const SCHEMA: &str = r"(.*?)";', source, re.S).group(1)


def columns(db):
    result = {}
    names = [row[0] for row in db.execute("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")]
    for table in names:
        quoted = '"' + table.replace('"', '""') + '"'
        result[table] = {row[1]: dict(kind=row[2], not_null=row[3], default=row[4], pk=row[5])
                         for row in db.execute("PRAGMA table_info(" + quoted + ")")}
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[2]
    args.output.mkdir()  # A previous result is never overwritten.
    owned = repo / "rust/symbrain-memory/src/migration/sql"
    resources = sorted(owned.glob("*.sql"))
    assert len(resources) == 37
    bindings = {}
    added = set()
    for resource in resources:
        raw = resource.read_bytes()
        frozen = subprocess.check_output(["git", "show", FROZEN + ":internal/memory/db/migrations/" + resource.name], cwd=repo)
        assert raw == frozen
        bindings[str(resource.relative_to(repo))] = digest(raw)
        added.update(re.findall(r"ALTER TABLE (\w+) ADD COLUMN (\w+)", raw.decode()))
    source_path = repo / "rust/symbrain-memory/src/schema.rs"
    legacy = subprocess.check_output(["git", "show", BASE + ":rust/symbrain-memory/src/schema.rs"], cwd=repo).decode()
    sources = {"frozen-go": "CREATE TABLE schema_migrations(version TEXT PRIMARY KEY, applied_at DATETIME DEFAULT CURRENT_TIMESTAMP);\n" + "\n".join(p.read_text() for p in resources),
               "native-known-base": schema(legacy), "native-successor": schema(source_path.read_text())}
    observations = {}
    for owner, ddl in sources.items():
        (args.output / (owner + ".sql")).write_text(ddl)
        with closing(sqlite3.connect(args.output / (owner + ".db"))) as db:
            db.executescript(ddl)
            observations[owner] = columns(db)
    census = []
    for table, expected in observations["frozen-go"].items():
        for column, declaration in expected.items():
            default = declaration["default"]
            current = observations["native-successor"][table][column]["default"]
            prior = observations["native-known-base"][table][column]["default"]
            assert current == default, (table, column, current, default)
            if prior != default:
                assert (table, column, prior, default) == ("query_log", "created_at", "CURRENT_TIMESTAMP", "datetime('now')")
            census.append(dict(table=table, column=column, source="ALTER" if (table, column) in added else "CREATE", go_default=default, known_native_default=prior, successor_default=current))
    # Only a fixed, historically observed pair is evaluated; never execute a
    # caller default to decide whether arbitrary expressions are equivalent.
    with closing(sqlite3.connect(args.output / "timestamp-equivalence.db")) as db:
        db.executescript("CREATE TABLE timestamps(a DATETIME DEFAULT CURRENT_TIMESTAMP,b DATETIME DEFAULT (datetime('now'))); INSERT INTO timestamps DEFAULT VALUES;")
        equivalence = db.execute("SELECT a,b FROM timestamps").fetchone()
        assert equivalence[0] == equivalence[1]
    probes = []
    original = "ts DATETIME NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))"
    prepared = sources["native-successor"]
    assert prepared.count(original) == 1
    for name, replacement in [("unchanged", original), ("missing", "ts DATETIME NOT NULL"), ("wrong", "ts DATETIME NOT NULL DEFAULT 'not-a-timestamp'")]:
        ddl = prepared.replace(original, replacement)
        (args.output / ("oplog-" + name + ".sql")).write_text(ddl)
        with closing(sqlite3.connect(args.output / ("oplog-" + name + ".db"))) as db:
            db.executescript("PRAGMA foreign_keys=ON;" + ddl)
            error = None
            try:
                db.execute("INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES ('owned','owned SQL invariant','global','{}','[]','2000-01-01','2000-01-01')")
                db.commit()
            except sqlite3.Error as failure:
                error = dict(text=str(failure), code=failure.sqlite_errorcode, name=failure.sqlite_errorname)
                db.rollback()
            probes.append(dict(case=name, error=error, columns=columns(db)["sync_oplog"], memory_rows=list(db.execute("SELECT id FROM memories")), oplog_rows=list(db.execute("SELECT op,memory_id,ts FROM sync_oplog"))))
    assert probes[0]["error"] is None and len(probes[0]["oplog_rows"]) == 1
    assert probes[1]["error"]["name"] == "SQLITE_CONSTRAINT_NOTNULL"
    assert probes[1]["memory_rows"] == probes[1]["oplog_rows"] == []
    assert probes[2]["error"] is None and probes[2]["oplog_rows"][0][2] == "not-a-timestamp"
    for path in [source_path, repo / "rust/symbrain-memory/src/migration/defaults.rs", repo / "rust/symbrain-memory/src/migration/facts.rs", repo / "rust/symbrain-memory/src/migration/runner.rs", Path(__file__)]:
        bindings[str(path.relative_to(repo))] = digest(path.read_bytes())
    receipt = dict(scope="actual owned Python-SQLite invariants only; no compiled Go/Rust runtime", source_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo).decode().strip(), source_dirty=subprocess.check_output(["git", "status", "--porcelain"], cwd=repo).decode(), sqlite_version=sqlite3.sqlite_version, frozen_go=FROZEN, native_base=BASE, sources=bindings, census=census, timestamp_equivalence=equivalence, original_control_cases=probes,
                   files={str(p.relative_to(args.output)): digest(p.read_bytes()) for p in args.output.iterdir() if p.is_file()})
    (args.output / "receipt.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(dict(owned_columns=len(census), default_columns=sum(row["go_default"] is not None for row in census), known_default_differences=sum(row["go_default"] != row["known_native_default"] for row in census), actual_SQL_controls=len(probes), scope=receipt["scope"])))


if __name__ == "__main__":
    main()
