#!/usr/bin/env python3
"""Actual Go/native `symguard scan` discovery decode boundaries (#770 slice 2).

Every case writes one client config into a disposable HOME, runs the frozen Go
handler and the native binary for `--format=json` and `--format=table`, and
compares stdout, stderr, exit and file state.  Go ranges over a map when it
reports per-entry findings, so the order of the finding lines after the
`symguard scan: N finding(s)` header is not stable between Go runs; those
lines are compared as a sorted multiset (the only normalisation).  Only the
lines Go emits from a map range (`server "..." is missing ...` / `has unknown
type ...`) are sorted, and only within a run of consecutive such lines; the
source-level finding lines (parse errors, `config file not found`) keep their
order, which is the fixed source order.
"""
import json
import os
import re
from pathlib import Path
import stat
import subprocess
import sys
import tempfile

import replay

CLIENT_PATHS = {
    'hermes': '.config/hermes/config.json',
    'cursor': '.cursor/mcp.json',
    'vscode': '.vscode/mcp.json',
    'opencode': '.config/opencode/config.json',
    'claude-desktop': 'Library/Application Support/Claude/claude_desktop_config.json'
    if sys.platform == 'darwin' else '.config/claude/claude_desktop_config.json',
}

SERVER = '"s":{"command":"run"}'

# Go's yaml.v3 behaviour that the native YAML dependency does not reproduce yet (#770 residual).
# These cases are executed against both binaries but never counted as parity.
RESIDUAL_YAML = {
    'top-level-array': 'root is not a mapping: yaml.v3 unmarshal-error text',
    'top-level-string': 'root is not a mapping: yaml.v3 unmarshal-error text',
    'top-level-number': 'root is not a mapping: yaml.v3 unmarshal-error text',
    'yaml-scalar-document': 'root is not a mapping: yaml.v3 unmarshal-error text',
    'yaml-sequence-document': 'root is not a mapping: yaml.v3 unmarshal-error text',
    'yaml-non-string-key-entry': 'non-string entry key: yaml.v3 rejects the whole map',
    'yaml-null-key-entry': 'null entry key: yaml.v3 rejects the whole map',
    'yaml-merge-key': 'merge key <<: not applied by the native parser',
    'yaml-duplicate-key': 'duplicate mapping key: yaml.v3 wording',
    'yaml-malformed': 'YAML syntax error wording',
    'yaml-tab-indent': 'YAML syntax error wording',
    'yaml-multi-document': 'second YAML document: yaml.v3 reads the first, native rejects',
}


TRUNCATION_DOCUMENTS = [
    ('value-kinds', b'{"mcpServers":{"s":{"command":"a\\u00e9\\ud83d\\ude00\\n\\"\\/","args":["x",null],'
                    b'"env":{"K":"v"},"n":-1.5e+3,"m":0,"o":10.25E-2,"t":true,"f":false,"z":null}}}'),
    ('escapes', b'{"Servers":true,"mcpServers":{"s":{"url":"u\\\\\\t\\b\\f\\r","type":"sse"},"t":[]}}'),
]


def j(body, key='mcpServers'):
    return ('{"%s":{%s}}' % (key, body)).encode()


def cases():
    rows = []

    def add(ident, data, client='cursor', contract='parity', extra=None):
        rows.append(dict(id=ident, client=client, data=data, contract=contract, extra=extra or {}))

    # --- valid documents, every client -------------------------------------------------
    for client in CLIENT_PATHS:
        key = 'mcp' if client == 'opencode' else 'mcpServers'
        add('client-' + client, j('"fixture":{"command":"/fixture/bin","args":["--a","b"],'
                                  '"env":{"TOKEN":"SYNTHETIC_SECRET_SENTINEL"}}', key), client)
    add('all-clients-at-once', b'', 'cursor', extra={'all': True})
    # --- field shapes ------------------------------------------------------------------
    add('url-only', j('"r":{"url":"https://example.test/mcp"}'))
    add('url-type-sse', j('"r":{"url":"https://example.test/sse","type":"sse"}'))
    add('url-type-mixed-case', j('"r":{"url":"u","type":"SSE"}'))
    add('transport-field', j('"r":{"command":"c","transport":"sse"}'))
    add('type-local', j('"r":{"command":"c","type":"LOCAL"}'))
    add('type-remote', j('"r":{"command":"c","type":"Remote"}'))
    add('command-and-url', j('"r":{"command":"c","url":"u"}'))
    add('args-null', j('"r":{"command":"c","args":null}'))
    add('args-null-element', j('"r":{"command":"c","args":["a",null,"b"]}'))
    add('args-empty', j('"r":{"command":"c","args":[]}'))
    add('env-null', j('"r":{"command":"c","env":null}'))
    add('env-null-value', j('"r":{"command":"c","env":{"A":null,"B":"x"}}'))
    add('env-empty', j('"r":{"command":"c","env":{}}'))
    add('environment-merge', j('"r":{"command":"c","env":{"A":"1","B":"2"},"environment":{"A":"3","C":"4"}}'), 'opencode')
    add('environment-on-cursor', j('"r":{"command":"c","environment":{"A":"1"}}'))
    add('opencode-unknown-type', j('"r":{"command":"c","type":"stdio"}', 'mcp'), 'opencode')
    add('opencode-unknown-type-url', j('"r":{"url":"u","type":"stdio"}', 'mcp'), 'opencode')
    add('cursor-unknown-type', j('"r":{"command":"c","type":"stdio"}'))
    add('unknown-fields-ignored', j('"r":{"command":"c","extra":{"x":[1,2]},"disabled":true}'))
    add('no-servers', j(''))
    add('servers-null', b'{"mcpServers":null}')
    add('entry-null', j('"r":null'))
    add('entry-empty-object', j('"r":{}'))
    add('missing-command-many', j('"b":{},"a":{"args":["x"]},"c":null,"d":{"env":{"K":"V"}}'))
    add('missing-command-with-valid', j('"z":{},"a":{"command":"ok"},"m":{"url":""}'))
    # --- case-insensitive and folded field names (encoding/json) -----------------------
    add('field-case-insensitive', j('"r":{"Command":"c","ARGS":["a"],"Url":"u","TYPE":"remote","Env":{"A":"b"}}'))
    add('field-case-environment', j('"r":{"command":"c","ENVIRONMENT":{"A":"b"},"Transport":"sse"}'))
    add('field-fold-long-s', '{"mcpServers":{"r":{"command":"c","arg\u017f":["a"],"tran\u017fport":"sse"}}}'.encode())
    add('field-unicode-escape-name', j('"r":{"comm\\u0061nd":"c"}'))
    add('field-duplicate-string-last-wins', j('"r":{"command":"a","command":"b"}'))
    add('field-duplicate-env-merges', j('"r":{"command":"c","env":{"A":"1"},"env":{"B":"2"}}'))
    add('field-duplicate-args-replaces', j('"r":{"command":"c","args":["a","b"],"args":["z"]}'))
    # Go decodes a duplicate `args` into the existing slice: null elements keep the old
    # slot, a regrown slice re-exposes stale backing slots, null drops the backing array.
    for ident, args in [
        ('null-element-keeps', '"args":["a"],"ARGS":[null]'),
        ('stale-slots-reexposed', '"args":["a","b","c"],"args":["x"],"args":[null,null]'),
        ('stale-slots-grow-past-len', '"args":["a","b"],"args":["x"],"args":["y",null,null]'),
        ('null-drops-backing', '"args":["a"],"args":null,"args":[null]'),
        ('empty-replaces-backing', '"args":["a","b"],"args":[],"args":[null,null]'),
        ('null-then-longer', '"args":["a","b"],"args":null,"args":["x","y","z"]'),
        ('shorter-then-null-mix', '"args":["a","b","c"],"args":[null,"y"],"args":[null,null,null,null]'),
        ('three-way-case-folded', '"args":["a","b"],"Args":["x"],"ARGS":[null,"q",null]'),
        ('type-error-element', '"args":["a"],"args":[1]'),
        ('wrong-type-after-array', '"args":["a","b"],"args":"x"'),
    ]:
        add('field-duplicate-args-' + ident, j('"r":{"command":"c",' + args + '}'))
    add('field-duplicate-case-mixed', j('"r":{"command":"a","COMMAND":"b"}'))
    add('server-duplicate-name', j('"r":{"command":"a","args":["x"]},"r":{"url":"u"}'))
    add('server-names-sorted', j('"b":{"command":"1"},"a":{"command":"2"},"B":{"command":"3"},"é":{"command":"4"},"":{"command":"5"}'))
    add('server-name-html', j('"a<b>&":{"command":"c<&>","args":["x\u2028y","\u2029"],"env":{"K<&>":"v"}}'))
    add('server-name-unicode-escapes', j('"\\u00e9\\ud83d\\ude00":{"command":"\\u0000\\u001f"}'))
    add('server-name-lone-surrogate', j('"\\ud800":{"command":"c"}'))
    add('command-invalid-utf8', b'{"mcpServers":{"r":{"command":"a\xffb\xe2\x82c"}}}')
    add('table-multibyte', j('"é雪":{"command":"/p/é","args":["雪"]}'))
    # --- key lookup --------------------------------------------------------------------
    add('key-mcp_servers', j(SERVER, 'mcp_servers'))
    add('key-mcp-on-cursor', j(SERVER, 'mcp'))
    add('key-mcpServers-on-opencode', j(SERVER), 'opencode')
    add('key-priority', b'{"mcp":{"x":{"command":"mcp"}},"mcpServers":{"s":{"command":"std"}}}')
    add('key-priority-opencode', b'{"mcp":{"x":{"command":"mcp"}},"mcpServers":{"s":{"command":"std"}}}', 'opencode')
    add('key-duplicate-top-level', b'{"mcpServers":{"a":{"command":"1"}},"mcpServers":{"b":{"command":"2"}}}')
    add('key-null-falls-through-no', b'{"mcpServers":null,"mcp":{"x":{"command":"m"}}}')
    add('key-case-differs', b'{"MCPSERVERS":{"s":{"command":"c"}}}')
    add('key-not-found', b'{"other":1}')
    add('key-not-found-empty', b'{}')
    # --- document envelope -------------------------------------------------------------
    add('leading-whitespace', b'  \n\t{"mcpServers":{"s":{"command":"c"}}}  \n')
    add('jsonc-comments', b'{ // line\n "mcpServers": { /* block */ "s": {"command":"https://x/y"} } }')
    add('jsonc-unterminated-block', b'{"mcpServers":{"s":{"command":"c"}}} /* open')
    add('jsonc-slash-at-end', b'{"mcpServers":{"s":{"command":"c"}}}/')
    add('jsonc-comment-in-string', b'{"mcpServers":{"s":{"command":"//not /* a comment"}}}')
    add('jsonc-escaped-quote', b'{"mcpServers":{"s":{"command":"a\\"//b"}}}')
    add('jsonc-trailing-comma', b'{"mcpServers":{"s":{"command":"c"},}}')
    add('utf8-bom-json', b'\xef\xbb\xbf{"mcpServers":{"s":{"command":"c"}}}')
    add('empty-file', b'')
    add('whitespace-file', b'   \n')
    add('top-level-array', b'[]')
    add('top-level-string', b'"x"')
    add('top-level-null', b'null')
    add('top-level-number', b'42')
    add('trailing-garbage', b'{"mcpServers":{}} x')
    add('two-documents', b'{"mcpServers":{}}{"mcpServers":{}}')
    # --- JSON syntax errors (Go checkValid messages) -----------------------------------
    for ident, text in [
        ('syntax-unterminated-object', '{"mcpServers":{"s":{"command":"c"}}'),
        ('syntax-bare-word', '{"mcpServers":{"s":{"command":c}}}'),
        ('syntax-missing-colon', '{"mcpServers" {}}'),
        ('syntax-missing-comma', '{"a":1 "b":2}'),
        ('syntax-single-quote', "{'mcpServers':{}}"),
        ('syntax-bad-escape', '{"mcpServers":{"s":{"command":"a\\qb"}}}'),
        ('syntax-bad-unicode-escape', '{"mcpServers":{"s":{"command":"a\\u12g4"}}}'),
        ('syntax-control-char', '{"mcpServers":{"s":{"command":"a\tb"}}}'),
        ('syntax-literal-truncated', '{"mcpServers":{"s":{"command":tru}}}'),
        ('syntax-number-leading-zero', '{"mcpServers":{"s":{"command":01}}}'),
        ('syntax-number-no-fraction', '{"mcpServers":{"s":{"command":1.}}}'),
        ('syntax-number-bad-exponent', '{"mcpServers":{"s":{"command":1e}}}'),
        ('syntax-minus-alone', '{"a":-}'),
        ('syntax-nan', '{"a":NaN}'),
        ('syntax-trailing-comma-array', '{"a":[1,]}'),
        ('syntax-key-not-string', '{1:2}'),
        ('syntax-nested-open', '{"mcpServers":{"s":{"args":["a"'),
        ('syntax-lone-open', '{'),
        ('syntax-quote-char', '{"a":"\'}\'"}x'),
        ('syntax-non-ascii-char', '{"a":é}'),
        ('syntax-nul-byte', '{"a":\x00}'),
        ('syntax-del-char', '{"a":\x7f}'),
        ('syntax-backslash-char', '{"a":\\}'),
        ('syntax-double-quote-char', '{"a" "b"}'),
    ]:
        add(ident, text.encode())
    add('syntax-invalid-utf8-char', b'{"a":\xff}')
    add('syntax-after-comment-strip', b'{"mcpServers":{} // trailing\n')
    # Go's scanner.eof() feeds a trailing space, so EOF inside a token is reported as
    # `invalid character ' ' <context>`; structural/in-string EOF stays
    # `unexpected end of JSON input`.  Every prefix of each document is compared.
    for name, document in TRUNCATION_DOCUMENTS:
        for cut in range(len(document)):
            add('truncated-%s-%03d' % (name, cut), document[:cut])
    for ident, text in [
        ('minus', '{"a":-'), ('zero', '{"a":0'), ('digit', '{"a":1'), ('dot', '{"a":1.'),
        ('fraction', '{"a":1.5'), ('exp', '{"a":1e'), ('exp-sign', '{"a":1e+'), ('exp-digit', '{"a":1e5'),
        ('t', '{"a":t'), ('tr', '{"a":tr'), ('tru', '{"a":tru'), ('true', '{"a":true'),
        ('f', '{"a":f'), ('fa', '{"a":fa'), ('fal', '{"a":fal'), ('fals', '{"a":fals'),
        ('n', '{"a":n'), ('nu', '{"a":nu'), ('nul', '{"a":nul'), ('null', '{"a":null'),
        ('backslash', '{"a":"\\'), ('u0', '{"a":"\\u'), ('u1', '{"a":"\\u1'),
        ('u2', '{"a":"\\u12'), ('u3', '{"a":"\\u123'), ('u4', '{"a":"\\u1234'),
        ('surrogate-pair-u', '{"a":"\\ud83d\\u'), ('surrogate-pair-u2', '{"a":"\\ud83d\\ude'),
        ('key-backslash', '{"a\\'), ('key-u', '{"a\\u00'), ('in-string', '{"a":"abc'),
        ('after-key', '{"a"'), ('after-colon', '{"a":'), ('after-value', '{"a":1'),
        ('after-comma', '{"a":1,'), ('array-open', '{"a":['), ('array-comma', '{"a":[1,'),
        ('literal-in-array', '{"a":[tru'), ('number-in-array', '{"a":[-'),
        ('servers-tru', '{"Servers":tru'), ('servers-escape', '{"mcpServers":{"s":{"command":"a\\u00'),
    ]:
        add('truncated-token-' + ident, text.encode())
    # --- JSON type errors (Go UnmarshalTypeError, first error in document order) -------
    for ident, body in [
        ('type-servers-array', '"mcpServers":[]'),
        ('type-servers-string', '"mcpServers":"x"'),
        ('type-servers-number', '"mcpServers":3'),
        ('type-servers-bool', '"mcpServers":true'),
        ('type-entry-string', '"mcpServers":{"s":"x"}'),
        ('type-entry-array', '"mcpServers":{"s":[]}'),
        ('type-entry-number', '"mcpServers":{"s":1.5}'),
        ('type-entry-bool', '"mcpServers":{"s":false}'),
        ('type-command-number', '"mcpServers":{"s":{"command":42}}'),
        ('type-command-object', '"mcpServers":{"s":{"command":{}}}'),
        ('type-command-bool', '"mcpServers":{"s":{"command":true}}'),
        ('type-command-array', '"mcpServers":{"s":{"command":["x"]}}'),
        ('type-url-number', '"mcpServers":{"s":{"url":1e3}}'),
        ('type-type-number', '"mcpServers":{"s":{"command":"c","type":1}}'),
        ('type-transport-bool', '"mcpServers":{"s":{"command":"c","transport":false}}'),
        ('type-args-string', '"mcpServers":{"s":{"command":"c","args":"x"}}'),
        ('type-args-object', '"mcpServers":{"s":{"command":"c","args":{}}}'),
        ('type-args-number-element', '"mcpServers":{"s":{"command":"c","args":["a",7]}}'),
        ('type-args-object-element', '"mcpServers":{"s":{"command":"c","args":[{}]}}'),
        ('type-env-array', '"mcpServers":{"s":{"command":"c","env":[]}}'),
        ('type-env-string', '"mcpServers":{"s":{"command":"c","env":"x"}}'),
        ('type-env-number-value', '"mcpServers":{"s":{"command":"c","env":{"A":1}}}'),
        ('type-env-bool-value', '"mcpServers":{"s":{"command":"c","env":{"A":true}}}'),
        ('type-env-object-value', '"mcpServers":{"s":{"command":"c","env":{"A":{}}}}'),
        ('type-environment-number-value', '"mcpServers":{"s":{"command":"c","environment":{"A":1}}}'),
        ('type-first-error-doc-order', '"mcpServers":{"z":{"command":1},"a":{"url":2}}'),
        ('type-first-error-field-order', '"mcpServers":{"s":{"url":2,"command":1}}'),
        ('type-error-then-null-entry', '"mcpServers":{"s":{"command":1},"t":null}'),
        ('type-first-error-in-env-then-args', '"mcpServers":{"s":{"env":{"K":1},"args":"x"}}'),
        ('type-big-number', '"mcpServers":{"s":{"command":123456789012345678901234567890}}'),
        ('type-negative-number', '"mcpServers":{"s":{"command":-0.5e-3}}'),
        ('type-error-field-case-folded', '"mcpServers":{"s":{"COMMAND":1}}'),
        ('type-duplicate-field-error-then-ok', '"mcpServers":{"s":{"command":1,"command":"c"}}'),
        ('type-servers-valid-then-bad-second-key', '"mcpServers":{"s":{"command":"c"}},"mcp":[]'),
    ]:
        add(ident, ('{' + body + '}').encode())
    add('type-opencode-key-array', b'{"mcp":[]}', 'opencode')
    add('type-opencode-entry-string', b'{"mcp":{"s":"x"}}', 'opencode')
    # --- YAML paths --------------------------------------------------------------------
    for ident, text in [
        ('yaml-basic', 'mcpServers:\n  s:\n    command: run\n    args: [a, b]\n    env:\n      K: v\n'),
        ('yaml-url', 'mcpServers:\n  s:\n    url: https://x/y\n    type: sse\n'),
        ('yaml-key-mcp_servers', 'mcp_servers:\n  s:\n    command: run\n'),
        ('yaml-key-not-found', 'other: 1\n'),
        ('yaml-empty-servers', 'mcpServers: {}\n'),
        ('yaml-null-servers', 'mcpServers:\n'),
        ('yaml-servers-not-object', 'mcpServers: [1]\n'),
        ('yaml-servers-scalar', 'mcpServers: x\n'),
        ('yaml-entry-scalar-skipped', 'mcpServers:\n  s: x\n  t:\n    command: ok\n'),
        ('yaml-entry-null-skipped', 'mcpServers:\n  s:\n  t:\n    command: ok\n'),
        ('yaml-entry-missing-command', 'mcpServers:\n  s:\n    args: [x]\n  t:\n    command: ok\n'),
        ('yaml-typed-scalars-dropped', "mcpServers:\n  s:\n    command: run\n    args: [a, 2, true, null, 'x']\n    env: {A: 1, B: yes, C: 'q', D: null}\n"),
        ('yaml-command-number-dropped', 'mcpServers:\n  s:\n    command: 42\n    url: u\n'),
        ('yaml-environment-ignored', 'mcpServers:\n  s:\n    command: run\n    environment:\n      A: b\n    env:\n      C: d\n'),
        ('yaml-non-string-key-entry', 'mcpServers:\n  1:\n    command: a\n  s:\n    command: b\n'),
        ('yaml-anchor-alias', 'x: &d\n  command: run\nmcpServers:\n  s: *d\n'),
        ('yaml-merge-key', 'x: &d\n  command: run\nmcpServers:\n  s:\n    <<: *d\n    args: [a]\n'),
        ('yaml-duplicate-key', 'mcpServers:\n  s:\n    command: a\n    command: b\n'),
        ('yaml-malformed', 'mcpServers: [unclosed\n'),
        ('yaml-tab-indent', 'mcpServers:\n\ts:\n\t\tcommand: run\n'),
        ('yaml-document-start', '---\nmcpServers:\n  s:\n    command: run\n'),
        ('yaml-multi-document', 'mcpServers:\n  s:\n    command: a\n---\nmcpServers:\n  t:\n    command: b\n'),
        ('yaml-scalar-document', 'just text\n'),
        ('yaml-sequence-document', '- a\n- b\n'),
        ('yaml-json-flow', 'mcpServers: {s: {command: run}}\n'),
        ('yaml-after-json-key-miss', '{other: 1}\n'),
        ('yaml-flow-with-key', '{mcpServers: {s: {command: run, args: [a]}}}\n'),
        ('yaml-flow-key-not-found', '{other: {s: {command: run}}}\n'),
        ('yaml-flow-is-json-error', '{"mcpServers":{"s":{"command":"c"},}}\n'),
        ('yaml-comment-only', '# nothing\n'),
        ('yaml-bom', '\ufeffmcpServers:\n  s:\n    command: run\n'),
        ('yaml-quoted-unicode', 'mcpServers:\n  "é":\n    command: "\\u00e9<&>"\n'),
        ('yaml-multiline-string', 'mcpServers:\n  s:\n    command: |\n      run\n      two\n'),
        ('yaml-null-key-entry', 'mcpServers:\n  ~:\n    command: a\n'),
    ]:
        add(ident, text.encode())
    # --- I/O ---------------------------------------------------------------------------
    add('io-directory', b'', contract='host-wording' if os.name == 'nt' else 'parity', extra={'directory': True})
    add('io-missing-file', b'', extra={'missing': True})
    if os.name != 'nt':
        add('io-symlink-loop', b'', extra={'loop': True})
        if hasattr(os, 'geteuid') and os.geteuid() != 0:
            add('io-permission-denied', b'', extra={'chmod': 0})
    for row in rows:
        if row['id'] in RESIDUAL_YAML:
            row['contract'] = 'residual-yaml'
    return rows


def prepare(root, case):
    env = replay.setup(root, 'empty')
    home = root / 'home'
    targets = CLIENT_PATHS if case['extra'].get('all') else {case['client']: CLIENT_PATHS[case['client']]}
    for client, rel in targets.items():
        path = home / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        if case['extra'].get('all'):
            key = 'mcp' if client == 'opencode' else 'mcpServers'
            data = ('{"%s":{"%s-b":{"command":"%s"},"%s-a":{"url":"u","env":{"K":"V"}}}}' % (key, client, client, client)).encode()
        else:
            data = case['data']
        if case['extra'].get('directory'):
            path.mkdir()
        elif case['extra'].get('loop'):
            path.symlink_to(path.name)
        elif 'chmod' in case['extra']:
            path.write_bytes(b'{}')
            path.chmod(case['extra']['chmod'])
        elif case['extra'].get('missing'):
            pass
        else:
            path.write_bytes(data)
    return env


MAP_RANGED = re.compile(rb'^  \w+ \[[^\]]*\] .*: server ".*" (is missing both command and url|has unknown type .*)$')


def order_map_ranged(body):
    """Sort each run of consecutive map-ranged finding lines of one source; keep the rest in order."""
    out, run = [], []
    for line in body + [None]:
        if line is not None and MAP_RANGED.match(line):
            run.append(line)
            continue
        out.extend(sorted(run))
        run = []
        if line is not None:
            out.append(line)
    return out


def split_findings(stderr):
    """Return (header, finding lines with map-ranged runs sorted, rest) for a scan stderr stream."""
    lines = stderr.split(b'\n')
    if lines and lines[0].startswith(b'symguard scan: ') and lines[0].endswith(b' finding(s)'):
        count = int(lines[0].split()[2])
        body = lines[1:1 + count]
        return lines[0], order_map_ranged(body), lines[1 + count:]
    return None, None, lines


def snapshot(root):
    """Mode and bytes of every regular file; unreadable files record only their mode."""
    files = {}
    for path in sorted(root.rglob('*')):
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode):
            continue
        try:
            raw = path.read_bytes().hex()
        except OSError:
            raw = 'unreadable'
        name = str(path.relative_to(root)).replace('\\', '/')
        files[name] = dict(mode=stat.S_IMODE(info.st_mode) if os.name != 'nt' else None,
                           raw_hex=raw, normalized_hex=raw)
    return files


def observe(binary, case, root, native, fmt):
    env = prepare(root, case)
    before = snapshot(root)
    command = [str(binary), 'scan', '--format=' + fmt]
    if binary.suffix == '.py':
        command.insert(0, sys.executable)
    p = subprocess.run(command, cwd=root / 'project', env=env, capture_output=True, timeout=10)
    files = snapshot(root)
    assert files == before, 'scan changed private input state'
    return dict(exit_code=p.returncode, stdout_hex=p.stdout.hex(), stderr_hex=p.stderr.hex(),
                normalized_stdout_hex=replay.normalized_stream(p.stdout, root, ['scan'], native, True).hex(),
                normalized_stderr_hex=replay.normalized_stream(p.stderr, root, ['scan'], native, False).hex(),
                files=files)


def comparable(o):
    header, findings, rest = split_findings(bytes.fromhex(o['normalized_stderr_hex']))
    return o['exit_code'], o['normalized_stdout_hex'], header, findings, rest, {
        k: (v['mode'], v['normalized_hex']) for k, v in o['files'].items()}


def residual(case, go, native):
    """Known unported yaml.v3 shape: exit code must still agree; never a parity pass."""
    assert go['exit_code'] == native['exit_code'] == 0, 'exit code differs'
    if comparable(go) == comparable(native):
        return 'matched'
    assert case['id'] in RESIDUAL_YAML
    return 'residual-yaml: ' + RESIDUAL_YAML[case['id']]


def host_wording(go, native):
    def strip(o):
        text = bytes.fromhex(o['normalized_stderr_hex'])
        lines = [l.split(b'read failed:')[0] + b'read failed:' if b'read failed:' in l else l for l in text.split(b'\n')]
        return o['exit_code'], o['normalized_stdout_hex'], sorted(lines)
    assert strip(go) == strip(native), 'stdout/exit/frame differs beyond host wording'
    return 'host-wording'


def compare(case, go, native):
    if case['contract'] == 'residual-yaml':
        return residual(case, go, native)
    if case['contract'] == 'host-wording':
        return host_wording(go, native)
    assert comparable(go) == comparable(native), 'stdout/stderr/exit/state differs'
    assert b'SYNTHETIC_SECRET_SENTINEL' not in bytes.fromhex(native['stdout_hex'])
    assert b'SYNTHETIC_SECRET_SENTINEL' not in bytes.fromhex(native['stderr_hex'])
    return 'matched'


MUTANTS = [
    ('reorder-servers', 'json-servers', None),
    ('drop-finding', 'finding-count', 'missing-command-many'),
    ('unescape-html', 'html', 'server-name-html'),
    ('hide-parse-error', 'parse-error', 'syntax-bare-word'),
    ('case-sensitive-fields', 'field-case', 'field-case-insensitive'),
    ('accept-type-error', 'type-error', 'type-command-number'),
    ('reorder-source-findings', 'source-order', 'syntax-bare-word'),
    ('truncated-token-as-eof', 'eof-context', 'truncated-token-tru'),
]


def mutate(mode, stdout, stderr, code):
    if mode == 'reorder-servers':
        doc = json.loads(stdout)
        doc['servers'].reverse()
        return (json.dumps(doc, indent=2, ensure_ascii=False) + '\n').encode(), stderr, code
    if mode == 'drop-finding':
        lines = stderr.split(b'\n')
        return stdout, b'\n'.join(lines[:1] + lines[2:]), code
    if mode == 'unescape-html':
        return stdout.replace(b'\\u003c', b'<').replace(b'\\u003e', b'>').replace(b'\\u0026', b'&'), stderr, code
    if mode == 'hide-parse-error':
        return stdout, b'', code
    if mode == 'case-sensitive-fields':
        return b'{\n  "servers": []\n}\n', stderr, code
    if mode == 'accept-type-error':
        return b'{\n  "servers": []\n}\n', b'', 0
    if mode == 'reorder-source-findings':
        lines = stderr.split(b'\n')
        count = int(lines[0].split()[2])
        return stdout, b'\n'.join(lines[:1] + lines[1:1 + count][::-1] + lines[1 + count:]), code
    if mode == 'truncated-token-as-eof':
        return stdout, stderr.replace(b"invalid character ' ' in literal true (expecting 'e')",
                                      b'unexpected end of JSON input'), code
    raise AssertionError(mode)


def run_mutants(go, native, root, selected):
    rows = []
    for index, (mode, _, ident) in enumerate(MUTANTS):
        if ident is None:
            ident = 'client-cursor'
            case = dict(id=ident, client='cursor', contract='parity', extra={},
                        data=j('"a":{"command":"1"},"b":{"command":"2"}'))
        else:
            case = selected[ident]
        base = root / ('mutant-' + str(index))
        base.mkdir()
        wrapper = base / 'mutant.py'
        wrapper.write_text(("import subprocess,sys\nsys.path.insert(0,%r)\nimport scan_boundaries as m\n"
                            "p=subprocess.run([%r,*sys.argv[1:]],capture_output=True)\n"
                            "o,e,c=m.mutate(%r,p.stdout,p.stderr,p.returncode)\n"
                            "sys.stdout.buffer.write(o);sys.stderr.buffer.write(e);sys.exit(c)\n")
                           % (str(Path(__file__).resolve().parent), str(native), mode), encoding='utf-8')
        left = observe(go, case, base / 'go', False, 'json')
        right = observe(wrapper, case, base / 'native', True, 'json')
        try:
            compare(case, left, right)
        except AssertionError as error:
            rows.append(dict(id=mode, case=ident, rejected=True, diagnostic=str(error),
                             wrapper_sha256=replay.digest(wrapper.read_bytes())))
        else:
            raise AssertionError('accepted actual scan mutation: ' + mode)
    return rows


def main():
    go, native, report = map(Path, sys.argv[1:4])
    go, native = go.resolve(strict=True), native.resolve(strict=True)
    selected = cases()
    ids = [c['id'] for c in selected]
    assert len(set(ids)) == len(ids), 'duplicate case id'
    rows = []
    with tempfile.TemporaryDirectory(prefix='guard770-scan-') as raw:
        root = Path(raw)
        for index, case in enumerate(selected):
            for fmt in ['json', 'table']:
                left = observe(go, case, root / str(index) / fmt / 'go', False, fmt)
                right = observe(native, case, root / str(index) / fmt / 'native', True, fmt)
                try:
                    disposition = compare(case, left, right)
                except AssertionError as error:
                    disposition = 'failed: ' + str(error)
                rows.append(dict(id=case['id'] + ':' + fmt, client=case['client'], contract=case['contract'],
                                 input_hex=case['data'].hex(), disposition=disposition, go=left, native=right))
        repeats = {}
        for ident in ['missing-command-many']:
            repeats[ident] = sorted({observe(go, next(c for c in selected if c['id'] == ident),
                                             root / 'repeat' / str(i), False, 'json')['normalized_stderr_hex']
                                     for i in range(12)})
        mutants = run_mutants(go, native, root, {c['id']: c for c in selected})
    failures = [r['id'] + ': ' + r['disposition'] for r in rows if r['disposition'].startswith('failed')]
    output = dict(candidate_head=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=replay.ROOT, text=True).strip(),
                  candidate_dirty=bool(subprocess.check_output(['git', 'status', '--porcelain'], cwd=replay.ROOT)),
                  cases=len(selected), process_pairs=len(rows), matched=sum(r['disposition'] == 'matched' for r in rows),
                  residual=sum(r['disposition'].startswith('residual') for r in rows),
                  distinct_go_stderr_orderings=repeats, mutants=mutants,
                  binaries_sha256=dict(go=replay.digest(go.read_bytes()), native=replay.digest(native.read_bytes())),
                  results=rows,
                  limits=['finding lines are compared as a sorted multiset because Go ranges over a map',
                          'only runs of consecutive map-ranged finding lines of one source are sorted; source-level lines keep order',
                          'YAML parser messages and unsupported YAML shapes: see disposition of yaml-* cases',
                          'native Linux/macOS/Windows CI still required'])
    report.write_text(json.dumps(output, indent=2) + '\n', encoding='utf-8')
    if failures and os.environ.get('SCAN_EXPLORE'):
        print('\n'.join(failures))
    assert not failures, '\n'.join(failures)
    print(f'scan boundaries: {len(selected)} cases x2 formats, {output["matched"]}/{len(rows)} full matches, '
          f'{output["residual"]} explicit yaml residuals; {len(mutants)} actual mutants rejected')


if __name__ == '__main__':
    main()
