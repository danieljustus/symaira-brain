"""Owned immutable-Go baseline before any Copilot/Kimi successor cutover."""
import base64
import json
import pathlib
import sys

cases=[]
def add(provider,name,files=None,env=None,home_mode="same"):
    cases.append(dict(id=provider+'-'+name,provider=provider,files={key:(value if isinstance(value,bytes)else value.encode()).hex()for key,value in (files or {}).items()},env=env or {},home_mode=home_mode))
copilot='.config/github-copilot/'
for row in json.loads(pathlib.Path('rust/symbrain-usage/tests/fixtures/copilot_file_token_oracle.json').read_text(encoding='utf-8'))['cases']:
    files={copilot+name:row[key]for key,name in [('apps_json','apps.json'),('hosts_json','hosts.json')]if key in row}
    add('copilot','frozen-'+row['id'],files)
for name,value in {
    'token-null-retained':'{"github.com:a":{"oauth_token":"retained","oauth_token":null}}',
    'entry-null-reset':'{"github.com:a":{"oauth_token":"old"},"github.com:a":null}',
    'unicode-fold':'{"github.com:a":{"oauth_toKen":"unicode-token"}}',
    'unknown-overflow':'{"github.com:a":{"oauth_token":"retained","metadata":1e1000}}',
    'wrong-entry-skipped':'{"github.com:a":{"oauth_token":42},"github.com:b":{"oauth_token":"retained"}}',
    'prefix-priority':'{"enterprise":{"oauth_token":"other"},"github.com:a":{"oauth_token":"preferred"}}',
    'same-prefix-tokens':'{"github.com:a":{"oauth_token":"same"},"github.com:b":{"oauth_token":"same"}}',
    'different-prefix-tokens':'{"github.com:a":{"oauth_token":"one"},"github.com:b":{"oauth_token":"two"}}',
    'fallback-same-tokens':'{"one":{"oauth_token":"same"},"two":{"oauth_token":"same"}}',
    'surrogate':'{"github.com:a":{"oauth_token":"test-\\ud800-token"}}',
    'invalid-utf8':b'{"github.com:a":{"oauth_token":"test-\xed\xa0\x80-token"}}',
    'deep-unknown':'{"github.com:a":{"oauth_token":"retained","metadata":'+'['*200+'0'+']'*200+'}}',
}.items():add('copilot',name,{copilot+'apps.json':value})
add('copilot','environment-preempts-ambiguous-files',{copilot+'apps.json':'{"a":{"oauth_token":"one"},"b":{"oauth_token":"two"}}'},env={'COPILOT_ACCESS_TOKEN':'synthetic-env-token'})
add('copilot','environment-error-preempts-files',{copilot+'apps.json':'{"a":{"oauth_token":"file-token"}}'},env={'COPILOT_ACCESS_TOKEN':'env://USAGE_LOCAL_FILES_ABSENT'})
for scheme in ['env://ABSENT','symvault://test/token','vault://test/token','keychain://test/account']:
    add('copilot','literal-'+scheme.split(':')[0],{copilot+'apps.json':json.dumps({'github.com:a':{'oauth_token':scheme}})})
for row in json.loads(pathlib.Path('rust/symbrain-usage/tests/fixtures/kimi_file_token_oracle.json').read_text(encoding='utf-8'))['cases']:
    files={'.kimi-code/credentials/kimi-code.json':row['contents']}if row['file_present']else{}
    add('kimi','frozen-'+row['id'],files)
for name,value in {
    'null-scalar-retained':'{"access_token":"retained","access_token":null}',
    'wrong-before-correct':'{"access_token":42,"access_token":"discarded"}',
    'root-null':'null','root-array':'[]','root-empty':'{}',
    'unicode-fold':'{"acceſſ_toKen":"unicode-token"}',
    'unknown-overflow':'{"access_token":"retained","metadata":1e1000}',
    'surrogate':'{"access_token":"test-\\ud800-token"}',
    'invalid-utf8':b'{"access_token":"test-\xed\xa0\x80-token"}',
    'deep-unknown':'{"access_token":"retained","metadata":'+'['*200+'0'+']'*200+'}',
}.items():add('kimi',name,{'.kimi-code/credentials/kimi-code.json':value})
for name,device in {'ascii':'  owned-device\r\n','unicode-whitespace':'\u0085\u2003owned-device\u3000','unicode-value':'owned-π','invalid-utf8':b'owned-\xff-device','oversize':b'x'*65537}.items():
    add('kimi','device-'+name,{'.kimi-code/credentials/kimi-code.json':'{"access_token":"cli-token"}','.kimi-code/device_id':device})
add('kimi','current-precedes-legacy',{'.kimi-code/credentials/kimi-code.json':'{"access_token":"current"}','.kimi/credentials/kimi-code.json':'{"access_token":"legacy"}'})
add('kimi','empty-current-suppresses-legacy',{'.kimi-code/credentials/kimi-code.json':'{}','.kimi/credentials/kimi-code.json':'{"access_token":"legacy"}'})
add('kimi','legacy-only',{'.kimi/credentials/kimi-code.json':'{"access_token":"legacy"}'})
add('kimi','explicit-home',{'.kimi-code/credentials/kimi-code.json':'{"access_token":"ignored"}','owned-kimi/credentials/kimi-code.json':'{"access_token":"explicit"}'},env={'KIMI_CODE_HOME':'$HOME/owned-kimi'})
add('kimi','empty-override',{'.kimi-code/credentials/kimi-code.json':'{"access_token":"current"}'},env={'KIMI_CODE_HOME':''})
add('kimi','all-three-strategies',{'.kimi-code/credentials/kimi-code.json':'{"access_token":"cli-token"}','.kimi-code/device_id':'owned-device'},env={'KIMI_CODE_API_KEY':'api-token','KIMI_AUTH_TOKEN':'web-token'})
add('kimi','error-api-retains-cli',{'.kimi-code/credentials/kimi-code.json':'{"access_token":"cli-token"}'},env={'KIMI_CODE_API_KEY':'env://USAGE_LOCAL_FILES_ABSENT'})
add('kimi','home-userprofile-disagreement',{'.kimi-code/credentials/kimi-code.json':'{"access_token":"home-token"}'},home_mode='different')
for provider,var,key in [('kimi','KIMI_CODE_BASE_URL','KIMI_CODE_API_KEY'),('nous','HERMES_PORTAL_BASE_URL','NOUS_PORTAL_ACCESS_TOKEN'),('openrouter','OPENROUTER_API_URL','OPENROUTER_API_KEY')]:
    for name,value in {'public-prefix':'https://api.example.test/prefix','uppercase-host':'https://API.example.test/prefix','query-fragment':'https://api.example.test/base?q=1#fragment','escaped-path':'https://api.example.test/a%20b','unicode-path':'https://api.example.test/π','http-rejected':'http://api.example.test','userinfo-rejected':'https://user:pass@api.example.test'}.items():
        add(provider,'base-'+name,env={var:value,key:'synthetic-token'})
for name,value in {'canonical':'wrk_owned123','hyphen':'wrk_owned-id','unicode':'wrk_π','slash':'workspace/owned'}.items():
    add('opencode','workspace-'+name,env={'OPENCODE_WORKSPACE_ID':value,'OPENCODE_COOKIE':'synthetic-cookie'})
add('opencode','workspace-without-cookie',env={'OPENCODE_WORKSPACE_ID':'workspace/owned'})
for name,payload in {'overflow-positive':'{"exp":1e19}','overflow-negative':'{"exp":-1e19}','upper-i64-boundary':'{"exp":9223372036854775808}','lower-i64-boundary':'{"exp":-9223372036854775808}','numeric-json-overflow':'{"exp":1e1000}'}.items():
    jwt='header.'+base64.urlsafe_b64encode(payload.encode()).decode().rstrip('=')+'.signature'
    add('nous','jwt-'+name,{'hermes/auth.json':json.dumps({'providers':[{'id':'nous','invoke_jwt':jwt}]})},env={'HERMES_HOME':'$HOME/hermes'})
assert len(cases)==len({row['id']for row in cases})
pathlib.Path(sys.argv[1]).write_text(json.dumps(cases,indent=2)+'\n', encoding='utf-8')
print('Go-only local/source baseline:',len(cases),'cases')
