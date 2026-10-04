"""Exact syntax-error protocol view for additive JSON boundary cases only."""
import json
from compare import matched


def is_parse_case(name):
    if name.startswith('json-boundary-syntax-before-type-'):
        return True
    if name.startswith('json-review-novel-ignored-depth-'):
        return any(f'depth-{depth}-' in name for depth in (9998, 10001))
    return any(f'depth-9998-{position}-' in name for position in ('arguments', 'meta'))


def parse_view(record):
    out = bytes.fromhex(record['stdout_hex'])
    if out.startswith(b'Content-Length:'):
        responses = []
        while out:
            header, out = out.split(b'\r\n\r\n', 1)
            assert header.startswith(b'Content-Length: ')
            length = int(header.removeprefix(b'Content-Length: '))
            assert 0 < length <= len(out)
            responses.append(json.loads(out[:length]))
            out = out[length:]
    else:
        responses = [json.loads(line) for line in out.splitlines()]
    assert [row.get('id') for row in responses] == [1, 2, None]
    skills = [row for row in responses[1]['result']['tools'] if row['name'].startswith('skills_')]
    assert len(skills) == len({row['name'] for row in skills}) == 11
    responses[1]['result']['tools'] = skills
    reply = responses[2]
    assert reply['id'] is None and 'result' not in reply
    assert reply['error']['code'] == -32700
    assert isinstance(reply['error']['message'], str)
    assert reply['error']['message'].startswith('Parse error: ')
    assert record['exit'] == 0
    return record['exit'], responses, bytes.fromhex(record['stderr_hex'])


def json_matched(name, go, rust, root):
    if not is_parse_case(name):
        return matched(go, rust, root, True)
    return parse_view(go) == parse_view(rust) and go['filesystem']['files'] == rust['filesystem']['files']
