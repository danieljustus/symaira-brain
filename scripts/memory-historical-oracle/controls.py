#!/usr/bin/env python3
"""Real SQL/child controls for intentional repair and whole-store rollback."""

import argparse
from contextlib import closing
import json
from pathlib import Path
import shutil
from provider import activate
activate()
import sqlite3

import replay


CASES = (
    ("native-false-completion", 37, "DROP INDEX idx_entity_relations_relation_id; CREATE INDEX idx_entity_relations_id ON entity_relations(id); UPDATE rules SET updated_at=NULL; UPDATE entity_relations SET id='',updated_at=NULL;", "repair-provenance"),
    ("healthy-current-null", 37, "UPDATE rules SET updated_at=NULL; UPDATE entity_relations SET updated_at=NULL;", "preserve-null"),
    ("claimed-old-fts", 37, "DROP TRIGGER memories_ai; DROP TRIGGER memories_ad; DROP TRIGGER memories_au; DROP TABLE memories_fts;", "repair-fts"),
    ("ignored-rule-backfill", 5, "CREATE TRIGGER ignore_rule BEFORE UPDATE ON rules BEGIN SELECT RAISE(IGNORE); END;", "rule timestamp"),
    ("ignored-marker-insert", 5, "CREATE TRIGGER ignore_marker BEFORE INSERT ON schema_migrations BEGIN SELECT RAISE(IGNORE); END;", "migration marker"),
    ("unknown-fts-owner", 37, "DROP TRIGGER memories_ai; DROP TRIGGER memories_ad; DROP TRIGGER memories_au; DROP TABLE memories_fts; CREATE TABLE memories_fts(payload TEXT); INSERT INTO memories_fts VALUES ('must survive');", "memories_fts"),
    ("late-owned-index-collision", 21, "CREATE INDEX idx_query_log_actor ON rules(content);", "idx_query_log_actor"),
    ("nonempty-identity-collision", 37, "DROP INDEX idx_entity_relations_relation_id; UPDATE entity_relations SET id='caller-collision';", "UNIQUE"),
    ("custom-sync-trigger", 21, "CREATE TRIGGER trg_memories_oplog_insert AFTER INSERT ON memories BEGIN SELECT 'must survive'; END;", "trg_memories_oplog_insert"),
)


def assert_effect(effect, before, result, database):
    if effect.startswith("repair-") or effect == "preserve-null":
        assert result["process"]["exit"] == 0, result["process"]
        with closing(sqlite3.connect(database)) as db:
            if effect == "repair-provenance":
                assert db.execute("SELECT count(*) FROM rules WHERE updated_at=created_at").fetchone()[0] == 1
                rows = list(db.execute("SELECT id,created_at,updated_at,valid_from,valid_until FROM entity_relations"))
                assert len(rows) == 2 and len({row[0] for row in rows}) == 2
                for identity, created, updated, start, end in rows:
                    assert replay.UUID4.fullmatch(identity) and created == updated == replay.CREATED
                    assert start is None and end is None
            elif effect == "repair-fts":
                assert replay.inspect(database) == dict(run_ids=["legacy-memory"], foreign_key_errors=[])
                definition = db.execute("SELECT sql FROM sqlite_schema WHERE name='memories_fts'").fetchone()[0]
                assert "tokenize='porter unicode61'" in replay.canonical(definition)
            else:
                assert db.execute("SELECT count(*) FROM rules WHERE updated_at IS NULL").fetchone()[0] == 1
                assert db.execute("SELECT count(*) FROM entity_relations WHERE updated_at IS NULL AND valid_from IS NULL AND valid_until IS NULL").fetchone()[0] == 2
                assert result["after"]["tables"]["entity_relations"]["rows"] == before["tables"]["entity_relations"]["rows"]
    else:
        assert result["process"]["exit"] != 0, result["process"]
        diagnostic = bytes.fromhex(result["process"]["stderr_hex"]).decode("utf-8", errors="replace")
        assert effect.lower() in diagnostic.lower(), diagnostic
        assert result["after"] == before, "whole owned state must roll back"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("go", "go-source", "native", "output"):
        parser.add_argument("--" + flag, type=Path, required=True)
    args = parser.parse_args()
    args.go, args.native, args.go_source, args.output = (path.resolve() for path in (args.go, args.native, args.go_source, args.output))
    resources = sorted((args.go_source / "internal/memory/db/migrations").glob("*.sql"))
    assert len(resources) == 37
    args.output.mkdir()
    receipt = dict(binaries={str(args.go): replay.digest(args.go), str(args.native): replay.digest(args.native)},
                   resources={str(path): replay.digest(path) for path in resources}, cases=[])
    replay.save(args.output / "receipt.json", receipt)
    for name, count, mutation, effect in CASES:
        root = args.output / name
        root.mkdir()
        seed = root / "seed.db"
        replay.fixture(seed, resources, count)
        with closing(sqlite3.connect(seed)) as db, db:
            db.executescript(mutation)
            if effect == "repair-fts":
                db.executescript((args.go_source / "internal/memory/db/migrations/016_fts5.sql").read_text())
        before = replay.snapshot(seed)
        pair = {}
        for label, binary in (("go", args.go), ("native", args.native)):
            child = root / label
            child.mkdir()
            database = child / "memory.db"
            shutil.copyfile(seed, database)
            result = dict(mutation=mutation, before=before, process=replay.execute(binary, child, database), after=replay.snapshot(database))
            replay.save(child / "result.json", result)
            pair[label] = result
            if label == "native":
                assert_effect(effect, before, result, database)
        # Go's marker shortcut remains an observed difference, never rewritten
        # as though frozen Go repaired false completion or guaranteed rollback.
        receipt["cases"].append(dict(name=name, effect=effect, go_exit=pair["go"]["process"]["exit"], native_exit=pair["native"]["process"]["exit"]))
        replay.save(args.output / "receipt.json", receipt)
    receipt["files"] = {str(path.relative_to(args.output)): replay.digest(path) for path in args.output.rglob("*") if path.is_file() and path.name != "receipt.json"}
    replay.save(args.output / "receipt.json", receipt)
    print(json.dumps(dict(actual_child_pairs=len(receipt["cases"]), actual_mutations=len(CASES))))


if __name__ == "__main__":
    main()
