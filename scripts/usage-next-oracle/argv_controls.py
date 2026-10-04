"""Reject actual-process late-admission and double-normalization regressions."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile

def main():
    parser=argparse.ArgumentParser()
    for key in ['go','parent','rust','sentinel','output']:parser.add_argument('--'+key,type=Path,required=True)
    args=parser.parse_args();observed=[]
    with tempfile.TemporaryDirectory(prefix='usage-next-argv-controls-')as temporary:
        root=Path(temporary)
        for kind,binary,transform,case in [('late-admission',args.parent,'','unsupported-base\', \'absent\', 0'),('double-normalization',args.rust,"out=[];terminated=False\nfor value in values:\n if terminated:out.append(value)\n elif value=='--':terminated=True;out.append(value)\n else:out.append('-'+value[2:]if value.startswith('--')and len(value)>2 else value)\nvalues=out",'unsupported-base\', \'absent\', 7')]:
            wrapper=root/(kind+'.py')
            wrapper.write_text('import subprocess,sys\nvalues=sys.argv[2:]\n'+transform+'\n'+f'p=subprocess.run([{str(binary.resolve())!r},sys.argv[1],*values],capture_output=True)\n'+'sys.stdout.buffer.write(p.stdout);sys.stderr.buffer.write(p.stderr);sys.exit(p.returncode)\n')
            command=[sys.executable,str(Path(__file__).with_name('argv.py'))]
            for key,path in [('go',args.go),('parent',args.parent),('rust',wrapper),('sentinel',args.sentinel),('output',root/(kind+'.json'))]:command+=['--'+key,str(path.resolve())]
            result=subprocess.run(command,capture_output=True,timeout=60);error=result.stderr.decode(errors='replace')
            assert result.returncode and 'AssertionError'in error and case in error,(kind,result.returncode,error)
            observed.append(dict(id=kind,rejected=True,exit=result.returncode,wrapper_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest(),wrapper_source=wrapper.read_text(),stdout=result.stdout.decode(errors='replace'),stderr=error))
    args.output.write_text(json.dumps(dict(controls=observed,rejected=2,binaries={str(p.resolve()):hashlib.sha256(p.read_bytes()).hexdigest()for p in [args.go,args.parent,args.rust,args.sentinel]}),indent=2)+'\n');print('PASS2 meaningful early-argv controls')
if __name__=='__main__':main()
