"""Recreate original test inputs; retain real old CLI outcomes, not lost test DBs."""
from pathlib import Path
from contextlib import closing
import hashlib, importlib.util, json, shutil, sqlite3

repo=Path('/workspace/symaira-memory649-integrated')
out=Path('/tmp/symaira-memory649-original252-case-state');out.mkdir()
spec=importlib.util.spec_from_file_location('historical_replay',repo/'scripts/memory-historical-oracle/replay.py')
replay=importlib.util.module_from_spec(spec);spec.loader.exec_module(replay)
resources=sorted((repo/'rust/symbrain-memory/src/migration/sql').glob('*.sql'))
assert resources[20].stem=='022_entity_relation_provenance'
assert resources[4].stem=='005_indexes'
cli=repo/'target/debug/symbrain'
assert hashlib.sha256(cli.read_bytes()).hexdigest()=='356d240a0bbd23f7c62fd6e4cfc3908145810a07d53d4d02fba8ea1ccb05ebf2'
cases=[('rule-abort',4,"CREATE TRIGGER reject_rule BEFORE UPDATE ON rules BEGIN SELECT RAISE(ABORT,'owned rule control'); END;",None),
       ('relation-abort',20,"CREATE TRIGGER reject_relation BEFORE UPDATE ON entity_relations BEGIN SELECT RAISE(ABORT,'owned relation control'); END;",None),
       ('rule-ignore',4,"CREATE TRIGGER ignore_rule BEFORE UPDATE ON rules BEGIN SELECT RAISE(IGNORE); END;",None),
       ('relation-ignore',20,"CREATE TRIGGER ignore_relation BEFORE UPDATE ON entity_relations BEGIN SELECT RAISE(IGNORE); END;",None),
       ('relation-time-ignore',20,"CREATE TRIGGER ignore_relation_time BEFORE UPDATE OF updated_at ON entity_relations BEGIN SELECT RAISE(IGNORE); END;",None),
       ('marker-ignore',4,"CREATE TRIGGER ignore_marker BEFORE INSERT ON schema_migrations BEGIN SELECT RAISE(IGNORE); END;",None),
       ('oplog-missing-default',20,"CREATE TRIGGER reject_marker BEFORE INSERT ON schema_migrations BEGIN SELECT RAISE(ABORT,'marker must not be reached'); END;",""),
       ('oplog-wrong-default',20,"CREATE TRIGGER reject_marker BEFORE INSERT ON schema_migrations BEGIN SELECT RAISE(ABORT,'marker must not be reached'); END;"," DEFAULT 'not-a-timestamp'"),
       ('late-index',20,"CREATE INDEX idx_query_log_actor ON rules(content);",None)]
records=[]
for name,end,mutation,default in cases:
    root=out/name;root.mkdir();dbpath=root/'memory.db';statements=["PRAGMA foreign_keys=ON; CREATE TABLE schema_migrations(version TEXT PRIMARY KEY, applied_at DATETIME DEFAULT CURRENT_TIMESTAMP);"]
    with closing(sqlite3.connect(dbpath)) as db,db:
        db.executescript(statements[0])
        for index,resource in enumerate(resources[:end+1]):
            sql=resource.read_text();db.executescript(sql);statements.append(sql)
            db.execute('INSERT INTO schema_migrations(version) VALUES (?)',(resource.stem,))
            if index==0:
                seed="INSERT INTO memories(id,content,scope,metadata,embedding,created_at,updated_at) VALUES ('memory','running quickly','global','{}','[]','2000-01-02 03:04:05','2000-01-02 03:04:05'); INSERT INTO rules(id,content,scope,metadata,created_at) VALUES ('rule','keep unchanged','global','{}','2000-01-02 03:04:05');"
            elif index==7:
                seed="INSERT INTO entities(id,name,created_at,updated_at) VALUES ('a','Alice','2000-01-02 03:04:05','2000-01-02 03:04:05'),('b','Bob','2000-01-02 03:04:05','2000-01-02 03:04:05');"
            elif index==19:
                seed="INSERT INTO entity_relations(from_entity_id,to_entity_id,relation_type,created_at) VALUES ('a','b','knows','2000-01-02 03:04:05'),('b','a','knows','2000-01-02 03:04:05');"
            else:
                seed=''
            db.executescript(seed);statements.append(seed)
        if default is not None:
            invalid=(repo/'rust/symbrain-memory/src/migration/sql/023_sync_oplog.sql').read_text().replace(" DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))",default)
            db.executescript(invalid);statements.append(invalid)
        db.executescript(mutation);statements.append(mutation)
        precondition=dict(inclusive_end=end,last_applied=resources[end].stem,marker_count=db.execute('SELECT count(*) FROM schema_migrations').fetchone()[0],relation_columns=list(db.execute('PRAGMA table_info(entity_relations)')))
        if end==20:
            precondition['relations']=list(db.execute('SELECT from_entity_id,to_entity_id,relation_type,id,created_at,updated_at FROM entity_relations ORDER BY from_entity_id'))
            assert precondition['marker_count']==21 and any(row[1]=='id' for row in precondition['relation_columns'])
            assert all(row[3] and row[4]==row[5] for row in precondition['relations'])
    (root/'recreated-fixture.sql').write_text('\n'.join(statements))
    shutil.copyfile(dbpath,root/'before.db');before=replay.snapshot(dbpath)
    process=replay.execute(cli,root,dbpath);after=replay.snapshot(dbpath)
    result=dict(case=name,scope='Fresh recreated original source inputs, Python SQLite seed; actual original252 Rust CLI constructor outcome. Not recovered original Rust-test database or its exact UUID/timestamp bytes.',python_seed_SQLite=sqlite3.sqlite_version,precondition=precondition,before=before,process=process,after=after)
    replay.save(root/'result.json',result);records.append(dict(case=name,exit=process['exit'],precondition=precondition,whole_state_unchanged=before==after))
    if name in ('relation-abort','relation-ignore','relation-time-ignore'):
        assert process['exit']==0
    else:
        assert process['exit']!=0 and before==after
replay.save(out/'receipt.json',dict(source='25292fe2d3349fe3b716783310096accb1f63007',actual_cli_sha256=replay.digest(cli),records=records,files={str(p.relative_to(out)):replay.digest(p) for p in out.rglob('*') if p.is_file()},original_test_DBs_removed_by_owned_fixture_Drop=True,reconstructed_input_not_original_test_state=True))
print(json.dumps(dict(recreated_inputs=len(records),actual_original252_CLI_outcomes=len(records),all_original_source_inputs_intent_explicit=True)))
