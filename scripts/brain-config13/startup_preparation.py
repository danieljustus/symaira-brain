"""Pure plan/comparator checks only; no SQLite, fixture, SDK or product access."""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import startup_cases as original
import startup_correction_cases as correction
import startup_replay as replay
import startup_state as state


def serial(value):
    return json.loads(json.dumps(value, default=lambda data: {'bytes_base64':state.b64(data)}))


def plans():
    old=list(original.cases()); new=list(correction.cases())
    assert new[:len(old)]==old
    assert len({case['id']for case in new})==len(new)
    return dict(original=len(old), total=len(new), added=len(new)-len(old),
                original_input_sha256=[dict(id=case['id'],sha256=hashlib.sha256(json.dumps(serial(case),sort_keys=True).encode()).hexdigest())for case in old],
                plans=serial(new))


def controls():
    trigger=('trigger','owned','memories',"CREATE TRIGGER owned AFTER INSERT ON memories BEGIN SELECT 'unchanged'; END")
    view=('view','v','memories','CREATE VIEW v AS SELECT id FROM memories')
    baseline=dict(sql=dict(raw_schema=[trigger,view],tables={}),actual_interval=[0,0])
    alternatives=[
        ('removed-trigger',[view]),
        ('changed-trigger',[('trigger','owned','memories',trigger[3].replace("'unchanged'","'changed'")),view]),
        ('removed-view',[trigger]),
        ('changed-view',[trigger,('view','v','memories',view[3].replace('SELECT id','SELECT scope'))]),
    ]
    results=[]
    for name, schemas in alternatives:
        mutant=copy.deepcopy(baseline); mutant['sql']['raw_schema']=schemas
        assert replay.sql_comparison(baseline)!=replay.sql_comparison(mutant)
        results.append(dict(control=name,rejected=True))
    equivalent=copy.deepcopy(baseline)
    equivalent['sql']['raw_schema'][0]=('trigger','owned','memories',"create /* owned formatting */ trigger owned after insert on memories begin select 'unchanged'; end")
    assert replay.sql_comparison(baseline)==replay.sql_comparison(equivalent)
    results.append(dict(control='formatting-only',equal=True))
    return results


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    record=dict(execution_kind=__doc__,unix=plans(),controls=controls())
    platform=os.name
    try:
        os.name='nt'
        record['windows_source_branch_projection']=plans()
        record['windows_source_branch_projection']['native_windows_executions']=0
    finally:os.name=platform
    record['runner_sha256']=state.sha(Path(__file__).read_bytes())
    args.out.write_text(json.dumps(record,indent=2)+'\n')


if __name__=='__main__':main()
