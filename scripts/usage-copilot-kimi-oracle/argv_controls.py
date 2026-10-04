"""Mutate actual native argv diagnostics and require exact process rejection."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['go', 'parent', 'rust', 'output']:
        parser.add_argument('--' + name, type=Path, required=True)
    parser.add_argument('--parent-source', required=True)
    args = parser.parse_args()
    mutations = [('bad-syntax', "error=error.replace(b'bad flag syntax: ',b'flag provided but not defined: -')", 'copilot-original-2'),
                 ('positional-hex', "error=error.replace(br'\\xff',br'\\xFF')", 'copilot-original-7')]
    if sys.platform != 'win32':
        mutations.append(('lossy-flag', "error=error.decode('utf8',errors='replace').encode()", 'copilot-original-5'))
    observations = []
    with tempfile.TemporaryDirectory(prefix='usage768-argv-mutants-') as temporary:
        root = Path(temporary)
        for name, change, case in mutations:
            # Windows cannot launch the raw ff positional; use an ASCII control escape instead.
            if sys.platform == 'win32' and name == 'positional-hex':
                change, case = "error=error.replace(br'\\x01',br'\\xAA')", 'copilot-expanded-24'
            wrapper = root / (name + '.py')
            wrapper.write_text('import subprocess,sys\n'
                f'p=subprocess.run([{str(args.rust.resolve())!r},*sys.argv[1:]],capture_output=True)\n'
                'sys.stdout.buffer.write(p.stdout)\nerror=p.stderr\n' + change + '\n'
                'sys.stderr.buffer.write(error)\nsys.exit(p.returncode)\n')
            command = [sys.executable, str(Path(__file__).with_name('argv.py')),
                       '--go', str(args.go), '--parent', str(args.parent), '--rust', str(wrapper),
                       '--parent-source', args.parent_source, '--output', str(root / (name + '.json'))]
            result = subprocess.run(command, capture_output=True, timeout=60)
            error = result.stderr.decode(errors='replace')
            assert result.returncode and 'AssertionError' in error and case in error, (name, result.returncode, error)
            observations.append(dict(name=name, case=case, rejected=True, exit=result.returncode,
                                     stdout=result.stdout.decode(errors='replace'), stderr=error,
                                     wrapper_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest()))
    args.output.write_text(json.dumps(dict(rejected=len(observations), controls=observations,
        binary_sha256={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [('go', args.go), ('parent', args.parent), ('rust', args.rust)]}), indent=2) + '\n')
    print('Usage argv:', len(observations), 'actual native diagnostic mutations rejected')


if __name__ == '__main__':
    main()
