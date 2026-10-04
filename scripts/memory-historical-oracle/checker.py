#!/usr/bin/env python3
"""Reject defective SQLite checkers before evaluating actual migration effects."""

import argparse
from contextlib import closing
import hashlib
import json
from pathlib import Path
from provider import activate, identity
activate()
import sqlite3
import sys
import tempfile


def preflight():
    with tempfile.TemporaryDirectory(prefix="memory-sqlite-checker-") as directory:
        root = Path(directory)
        with closing(sqlite3.connect(root / "default.db")) as db, db:
            db.executescript("CREATE TABLE historical(id TEXT NOT NULL);"
                             "INSERT INTO historical VALUES ('retained');"
                             "ALTER TABLE historical ADD COLUMN importance REAL NOT NULL DEFAULT 0.5;")
            value = list(db.execute("SELECT id,importance,typeof(importance) FROM historical"))
            integrity = list(db.execute("PRAGMA integrity_check"))
            assert value == [("retained", 0.5, "real")], value
            assert integrity == [("ok",)], (
                "SQLite checker misreports a historical REAL NOT NULL DEFAULT; "
                "use the verified checker engine rather than a Python-version-only pin", integrity)
            db.execute("CREATE VIRTUAL TABLE terms USING fts5(content, tokenize='porter unicode61')")
            db.execute("INSERT INTO terms(content) VALUES ('running')")
            assert list(db.execute("SELECT content FROM terms WHERE terms MATCH 'run'")) == [("running",)]
            db.execute("INSERT INTO terms(terms,rank) VALUES ('integrity-check',1)")
            source_id = db.execute("SELECT sqlite_source_id()").fetchone()[0]
            options = [row[0] for row in db.execute("PRAGMA compile_options")]

        # Only this disposable control is deliberately corrupted. Reopen so
        # SQLite reloads the changed declaration; a real NULL must remain red.
        damaged = root / "damaged.db"
        with closing(sqlite3.connect(damaged)) as db, db:
            db.executescript("CREATE TABLE damaged(id INTEGER PRIMARY KEY,importance REAL);"
                             "INSERT INTO damaged VALUES (1,NULL);"
                             "PRAGMA writable_schema=ON;"
                             "UPDATE sqlite_schema SET sql='CREATE TABLE damaged(id INTEGER PRIMARY KEY,importance REAL NOT NULL)' WHERE name='damaged';"
                             "PRAGMA writable_schema=OFF;")
        with closing(sqlite3.connect(damaged)) as db:
            rejected = list(db.execute("PRAGMA integrity_check"))
            assert rejected == [("NULL value in damaged.importance",)], rejected
        return dict(python=sys.version, executable=sys.executable,
                    sqlite_version=sqlite3.sqlite_version, sqlite_source_id=source_id,
                    compile_options=options, historical_default=dict(rows=value, integrity=integrity),
                    real_null_control=rejected, fts5_porter_and_integrity="passed",
                    checker_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                    go_or_native_product_execution=False, sqlite_provider=identity())


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = preflight()
    with args.output.open("x", encoding="utf-8") as output:
        output.write(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
