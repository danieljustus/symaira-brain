"""Source-only additive plan and portable admission predicates; zero products."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
from unittest.mock import patch
import startup_correction_cases as previous
import startup_final_cases as current
import startup_fixture_admission as admission
import startup_preparation as preparation

PARENT = '8afb9a9bf3c266478adea2f154bee9d5e0e1ebe1'


def serial(value):
    return json.loads(json.dumps(value, default=lambda data: {'bytes_base64':base64.b64encode(data).decode()}))


def plans():
    source = subprocess.check_output(['git','show',PARENT+':scripts/brain-config13/startup_correction_cases.py'])
    namespace = {'__name__':'immutable_parent_correction_cases'}
    exec(compile(source,'immutable-parent-correction-cases','exec'),namespace)
    result = {}
    platform = os.name
    try:
        for branch in ['posix','nt']:
            os.name = branch
            old = list(namespace['cases']()); prior = list(previous.cases()); new = list(current.cases())
            assert old == prior == new[:len(old)]
            assert len(set(case['id'] for case in new)) == len(new)
            assert (len(old),len(new)) == ((81,94) if branch == 'posix' else (69,82))
            result[branch] = dict(previous=len(old),total=len(new),runtime_executions=0,
                old_prefix_exact=True, plans=serial(new),
                input_sha256=[dict(id=case['id'],sha256=hashlib.sha256(json.dumps(serial(case),sort_keys=True).encode()).hexdigest()) for case in new])
    finally: os.name = platform
    return result


def admission_controls():
    rows=[]
    component=os.fsdecode(admission.RAW_COMPONENT)
    for platform,name,errno,expected in [
        ('darwin',component,92,True), ('darwin',component,84,False),
        ('linux',component,92,False), ('win32',component,92,False),
        ('darwin','blocked',92,False), ('darwin','blocked-ä',92,False),
        ('darwin',os.fsdecode(b'origin-\xe2\x82'),92,False),
    ]:
        error=OSError(errno,'owned predicate only')
        path=Path(name)
        with patch.object(admission.sys,'platform',platform):
            assert admission.unavailable(path,error) is expected
        rows.append(dict(platform=platform,component_hex=os.fsencode(name).hex(),errno=errno,
            allows_UNEXECUTED=expected,actual_kernel_probe=False))
    return rows


def width_projection():
    rows=[]
    for count in [100,1000,10000,100000]:
        parents=[None]+[0]*count
        first=[None]*len(parents);following=[None]*len(parents)
        for index in range(len(parents)-1,-1,-1):
            if parents[index] is not None:
                parent=parents[index];following[index]=first[parent];first[parent]=index
        indices=[];index=first[0]
        while index is not None:
            indices.append(index);index=following[index]
        assert indices == list(range(1,count+1))
        rows.append(dict(records=count,source_projected_link_build_visits=len(parents),
            actual_python_link_visits=len(indices),prior_source_projected_predicates=count*len(parents),
            Rust_executions=0,measured_Rust_walltime=False))
    return rows


def time_projection():
    rows=[]
    for zone,expected in [('+25:xx','hour-range'),('+25:x1','hour-range'),('?25:xx','hour-range'),
                         ('+24:xx','cannot-parse'),('+x5:61','cannot-parse'),
                         ('+25:61','minute-range'),('?24:60','cannot-parse'),('+24:60','accepted')]:
        digit=lambda text: text.isascii() and text.isdigit()
        hours=int(zone[1:3]) if digit(zone[1:3]) else None
        minutes=int(zone[4:6]) if hours is not None and digit(zone[4:6]) else None
        result=('minute-range' if minutes is not None and minutes>60 else
                'hour-range' if hours is not None and hours>24 else
                'cannot-parse' if minutes is None or zone[0] not in '+-' else 'accepted')
        assert result==expected
        rows.append(dict(zone=zone,source_projection=result,SDK_or_Rust_executions=0))
    return rows


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--out',type=Path,required=True)
    args=parser.parse_args()
    assert not args.out.exists()
    result=dict(kind=__doc__,plans=plans(),admission_predicate_controls=admission_controls(),
        width_projection=width_projection(),time_projection=time_projection(),
        original_schema_comparator_controls=preparation.controls(),
        executions=dict(compiler=0,Cargo=0,SDK=0,product=0,SQLite=0,network=0,target=0,ports=0))
    args.out.write_text(json.dumps(result,indent=2)+'\n')


if __name__=='__main__': main()
