"""Additive Go-state EOF plans; source-only until real process allocation."""
from pathlib import Path
from byte_cases import request, transport

INPUTS = Path(__file__).with_name('eof-review-inputs')
ORIGINALS = ('minus', 'dot', 'exponent', 'exp-sign', 'true', 'false', 'null', 'escape', 'unicode')
PARTIAL = [('minus', b'-', 'in numeric literal')]
PARTIAL += [(name, token, 'after decimal point in numeric literal') for name, token in [('zero-dot', b'0.'), ('dot', b'1.')]]
PARTIAL += [(name, token, 'in exponent of numeric literal') for name, token in [('exponent', b'1e'), ('upper-exponent', b'1E'), ('exp-plus', b'1e+'), ('exp-minus', b'1e-'), ('signed-exp', b'-0E+')]]
PARTIAL += [(word + '-' + str(length), word[:length].encode(),
             f"in literal {word} (expecting '{word[length]}')")
            for word in ('true', 'false', 'null') for length in range(1, len(word))]
PARTIAL += [('escape', b'"abc\\', 'in string escape code')]
PARTIAL += [('unicode-' + str(length), b'"\\u' + b'0' * length,
             'in \\u hexadecimal character escape') for length in range(4)]
GENERIC = [('object', b'{'), ('array', b'['), ('string', b'"abc'),
           ('unicode-complete-unclosed-string', b'"\\u1234'), ('complete-number', b'1'),
           ('complete-literal', b'true'), ('complete-string', b'"abc"')]
VALID = [('minus', b'-1'), ('fraction', b'1.0'), ('exponent', b'1e+2'),
         ('true', b'true'), ('false', b'false'), ('null', b'null'),
         ('string', b'"abc"'), ('unicode', b'"\\u1234"')]
MODES = ('line', 'framed')
PARSE_IDS = {f'json-eof-original-{name}-{mode}' for name in ORIGINALS for mode in MODES}
PARSE_IDS |= {f'json-eof-{position}-{name}-{mode}' for position in ('meta', 'arguments', 'syntax-before-type') for name, _, _ in PARTIAL for mode in MODES}
PARSE_IDS |= {f'json-eof-generic-{name}-{mode}' for name, _ in GENERIC for mode in MODES}


def is_parse_case(name):
    return name in PARSE_IDS


def cases(root, incoming):
    argv = ['mcp', '--profile-file', str(root / 'profile.toml')]
    bootstrap = incoming('skills_list', {})
    rows = []
    for name in ORIGINALS:
        for mode in MODES:
            raw = (INPUTS / (name + ('.framed' if mode == 'framed' else '.line'))).read_bytes()
            body = raw.split(b'\r\n\r\n', 1)[1] if mode == 'framed' else raw[:-1]
            rows.append((f'json-eof-original-{name}-{mode}', argv,
                         transport(bootstrap, body, mode == 'framed'), None))
    start = b'{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{'
    prefixes = {
        'meta': start + b'"name":"skills_list","_meta":{"n":',
        'arguments': start + b'"name":"skills_list","arguments":{"ignored":',
        'syntax-before-type': start + b'"name":4,"name":"skills_list","_meta":[],"arguments":{"ignored":',
    }
    additional = [(f'{position}-{name}', prefix + token)
                  for position, prefix in prefixes.items() for name, token, _ in PARTIAL]
    additional += [('generic-' + name, prefixes['meta'] + token) for name, token in GENERIC]
    additional += [('valid-' + name, request(b'{"name":"skills_list","arguments":{},"_meta":{"n":' + token + b'}}')) for name, token in VALID]
    for name, body in additional:
        for mode in MODES:
            rows.append((f'json-eof-{name}-{mode}', argv,
                         transport(bootstrap, body, mode == 'framed'), None))
    return rows


def run_controls(report, checkpoint, binaries, root, retained, env, invoke, incoming):
    """Two genuine input changes after complete matching actual baselines."""
    import shutil
    from compare import matched, mcp_view
    from json_compare import parse_view
    rows = {name: (argv, data) for name, argv, data, _ in cases(root, incoming)}
    plans = [
        ('json-eof-exponent-input', 'json-eof-valid-exponent-line',
         'json-eof-meta-exp-plus-line', 'invalid character \' \' in exponent of numeric literal'),
        ('json-eof-escape-input', 'json-eof-valid-string-framed',
         'json-eof-meta-escape-framed', 'invalid character \' \' in string escape code'),
    ]
    for name, selector, mutant_selector, expected in plans:
        baseline = next(row for row in report['results'] if row['id'] == selector)
        assert baseline['matched'] and matched(baseline['go'], baseline['rust'], root, True)
        baseline_view = mcp_view(baseline['go'])
        assert baseline_view[1][2]['id'] == 3 and 'error' not in baseline_view[1][2]
        shutil.rmtree(root)
        shutil.copytree(retained, root, symlinks=True)
        argv, data = rows[mutant_selector]
        control = {'name': name, 'baseline': selector, 'mutant_input': mutant_selector,
                   'baseline_matched': True, 'stdin_hex': data.hex(), 'candidate': {}, 'rejected': False}
        report['controls'].append(control)
        try:
            record = invoke(binaries['rust'], argv, env, root, data, record=control['candidate'])
        finally:
            checkpoint()
        actual = parse_view(record)
        assert actual[1][2]['error'] == {'code': -32700, 'message': 'Parse error: ' + expected}
        assert record['stderr_hex'] == baseline['go']['stderr_hex']
        for field in ('files', 'raw_file_hashes', 'validated_skl004_locks'):
            assert record['filesystem'][field] == baseline['go']['filesystem'][field]
        control['rejected'] = baseline_view != actual
        assert control['rejected'], 'actual input EOF must change the accepted response'
        checkpoint()
