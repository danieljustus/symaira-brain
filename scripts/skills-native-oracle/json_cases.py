"""Additive complete-JSON/metadata inputs; comparison and old vectors unchanged."""
from pathlib import Path
from byte_cases import request, transport

INPUTS = Path(__file__).with_name('json-review-inputs')


def cases(root, incoming):
    argv = ["mcp", "--profile-file", str(root / "profile.toml")]
    bootstrap = incoming("skills_list", {})
    rows = []
    # Exact twenty-six independent review wires, with unmodified IDs/body bytes.
    for path in sorted(INPUTS.iterdir()):
        framed = path.suffix == '.framed'
        raw = path.read_bytes()
        body = raw.split(b'\r\n\r\n', 1)[1] if framed else raw[:-1]
        rows.append(('json-review-' + path.stem + ('-framed' if framed else '-line'),
                     argv, transport(bootstrap, body, framed), None))
    extra = []
    for depth in (100, 128, 9996, 9997, 9998):
        nested = b'[' * depth + b'0' + b']' * depth
        for position in ('arguments', 'meta', 'envelope'):
            params = b'{"name":"skills_list","arguments":{}}'
            outer = b''
            if position == 'arguments': params = b'{"name":"skills_list","arguments":{"ignored":' + nested + b'}}'
            if position == 'meta': params = b'{"name":"skills_list","arguments":{},"_meta":{"n":' + nested + b'}}'
            if position == 'envelope': outer = b'"ignored":' + nested + b','
            extra.append((f'depth-{depth}-{position}', request(params, outer)))
    long = b'0.' + b'0' * 10000 + b'1e100000'
    numbers = [('negative-long', b'-' + long), ('negative-zero', b'-0'),
               ('negative-underflow', b'-1e-9999'), ('positive-underflow', b'1e-9999'),
               ('negative-overflow', b'-1e9999'), ('finite-max', b'1.7976931348623157e308'),
               ('boundary-overflow', b'1.7976931348623159e308'),
               ('least-subnormal', b'5e-324'), ('tie-underflow', b'2.4703282292062327e-324')]
    for name, token in numbers:
        extra.append(('float-' + name, request(b'{"name":"skills_list","arguments":{},"_meta":{"n":' + token + b'}}')))
    for name, content in [
            ('quoted-containers', br'{"n":"[{\\\"}]","x":0}'),
            ('duplicate-first-negative-overflow', br'{"n":-1e9999,"n":0,"x":[1e9999]}'),
            ('duplicate-first-positive-overflow', br'{"n":1e9999,"n":-1e9999}'),
            ('numeric-key-is-not-number', br'{"1e9999":"1e9999","0x1p2":false}'),
            ('nested-first-error', br'{"n":[{"first":-1e9999}],"second":1e9999}')]:
        extra.append((name, request(b'{"name":"skills_list","arguments":{},"_meta":' + content + b'}')))
    # Syntax errors precede typed names/meta even though an eventual name is valid.
    for name, content in [('plus', b'+1'), ('leading-zero', b'01'), ('hex', b'0x1p2'),
                          ('underscore', b'1_0'), ('nan', b'NaN'), ('inf', b'Inf'),
                          ('trailing-comma', b'[1,]'), ('missing-colon', b'{"n" 1}')]:
        extra.append(('syntax-before-type-' + name, request(b'{"name":4,"name":"skills_list","_meta":[],"arguments":' + content + b'}')))
    for name, body in extra:
        for framed in (False, True):
            rows.append(('json-boundary-' + name + ('-framed' if framed else '-line'),
                         argv, transport(bootstrap, body, framed), None))
    return rows


def run_controls(report, checkpoint, binaries, root, retained, env, invoke, incoming):
    # Mutate actual candidate input after a full actual Go/native matching
    # baseline. No stored result, parser or comparator mutation is used.
    import shutil
    from compare import matched, mcp_view
    from json_compare import parse_view
    rows = {name: (argv, data) for name, argv, data, _ in cases(root, incoming)}
    controls = [
        ('json-depth-input', 'json-boundary-depth-9997-arguments-line',
         'json-boundary-depth-9998-arguments-line', -32700),
        ('json-float-input', 'json-review-novel-meta-sdk-long-finite-line',
         'json-review-novel-meta-actual-overflow-line', -32602),
    ]
    for name, selector, mutant_selector, expected_code in controls:
        baseline = next(row for row in report['results'] if row['id'] == selector)
        assert baseline['matched'] and matched(baseline['go'], baseline['rust'], root, True)
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
        baseline_view = mcp_view(baseline['go'])
        actual = parse_view(record) if expected_code == -32700 else mcp_view(record)
        assert record['exit'] == 0 and actual[1][2]['error']['code'] == expected_code
        assert record['filesystem']['files'] == baseline['go']['filesystem']['files']
        control['rejected'] = baseline_view != actual
        assert control['rejected'], 'actual input mutation must change the accepted response'
        checkpoint()
