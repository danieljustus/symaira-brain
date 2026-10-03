"""Expand the preserved Go-only baseline; no frozen fixture writes."""
import json
import pathlib
import subprocess
import sys
import tempfile

with tempfile.TemporaryDirectory() as scratch:
    path = pathlib.Path(scratch) / 'baseline.json'
    subprocess.run([sys.executable, 'scripts/usage-local-files-baseline/cases.py', str(path)], check=True)
    cases = [row for row in json.loads(path.read_text()) if row['provider'] in ('copilot', 'kimi') and '-base-' not in row['id']]
assert len(cases) == 66
for row in cases:
    row['gate'] = ''
    if row['id'] in ('copilot-frozen-multiple-github-tokens-have-map-order-dependent-choice', 'copilot-frozen-multiple-fallback-tokens-have-map-order-dependent-choice', 'copilot-different-prefix-tokens'):
        row['gate'] = 'distinct eligible Go map tokens'
    if row['id'] in ('kimi-device-unicode-value', 'kimi-device-invalid-utf8'):
        row['gate'] = 'unproven device header bytes'

def add(provider, name, files=None, env=None):
    cases.append(dict(id=provider+'-port-'+name, provider=provider, files={key:value.encode().hex() for key,value in (files or {}).items()}, env=env or {}, home_mode='same', gate=''))

copilot = '.config/github-copilot/'
host = '{"github.com:a":{"oauth_token":"hosts-token"}}'
for name, apps in {
    'root-null-hosts': 'null', 'root-array-hosts': '[]',
    'wrong-before-correct-hosts': '{"github.com:a":{"oauth_token":42,"oauth_token":"discarded"}}',
    'duplicate-poison-hosts': '{"github.com:a":{"oauth_token":"discarded"},"github.com:a":{"oauth_token":42}}',
    'unknown-root-overflow': '{"ignored":1e1000,"github.com:a":{"oauth_token":"retained"}}',
    'malformed-missing': '{',
}.items():
    files = {copilot+'apps.json': apps}
    if name.endswith('hosts'): files[copilot+'hosts.json'] = host
    add('copilot', name, files)
for arrays in (9999, 10000):
    add('copilot', 'depth-'+str(arrays+1), {copilot+'apps.json': '{"github.com:a":{"oauth_token":"retained"},"ignored":'+'['*arrays+'0'+']'*arrays+'}'})
for size in (65536, 65537):
    prefix = '{"github.com:a":{"oauth_token":"retained"},"padding":"'
    value = prefix + 'x'*(size-len(prefix)-2) + '"}'
    add('copilot', 'size-'+str(size), {copilot+'apps.json':value, copilot+'hosts.json':host})

kimi = '.kimi-code/credentials/kimi-code.json'
for name, value in {
    'ignored-refresh-type': '{"access_token":"retained","refresh_token":42}',
    'quoted-containers': '{"access_token":"retained","ignored":"{[\\\"\\\"]}"}',
    'literal-env': '{"access_token":"env://ABSENT"}',
    'literal-vault': '{"access_token":"vault://test/token"}',
    'literal-keychain': '{"access_token":"keychain://test/account"}',
}.items(): add('kimi', name, {kimi:value})
for arrays in (9999, 10000):
    add('kimi', 'depth-'+str(arrays+1), {kimi:'{"access_token":"retained","ignored":'+'['*arrays+'0'+']'*arrays+'}'})
for size in (65536, 65537):
    prefix = '{"access_token":"retained","padding":"'
    add('kimi', 'size-'+str(size), {kimi:prefix+'x'*(size-len(prefix)-2)+'"}'})
add('kimi', 'both-env-errors-missing', {kimi:'{}'}, {'KIMI_CODE_API_KEY':'env://USAGE_LOCAL_FILES_ABSENT','KIMI_AUTH_TOKEN':'env://USAGE_LOCAL_FILES_ABSENT'})
assert len(cases) == len({row['id']for row in cases}) == 86
pathlib.Path(sys.argv[1]).write_text(json.dumps(cases, indent=2)+'\n')
print('Copilot/Kimi source-bound cases:', len(cases))
