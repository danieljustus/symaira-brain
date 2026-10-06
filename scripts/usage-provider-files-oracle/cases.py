"""Source-bound Claude typed-map and Codex generic-map file contracts."""
import json
import pathlib
import sys

cases=[]
def add(provider,name,value,present=True,override="default",env_token="",ambiguous=False):
    data=value if isinstance(value,bytes)else value.encode()
    cases.append(dict(id=provider+"-"+name,provider=provider,data_hex=data.hex(),file_present=present,override=override,env_token=env_token,ambiguous=ambiguous))
for case in json.loads(pathlib.Path("rust/symbrain-usage/tests/fixtures/claude_file_token_oracle.json").read_text(encoding='utf-8'))["cases"]:
    add("claude","frozen-"+case["id"],case["contents"],ambiguous="possible_tokens"in case)
for name,value in {
    "root-null":"null","root-array":"[]","root-string":'"bad"',"root-empty":"{}",
    "accounts-null":'{"oauthAccount":null}',"accounts-array":'{"oauthAccount":[]}',
    "account-null":'{"oauthAccount":{"default":null}}',"account-scalar":'{"oauthAccount":{"default":4}}',
    "string-null-retained":'{"oauthAccount":{"default":{"accessToken":"retained","accessToken":null}}}',
    "account-null-reset":'{"oauthAccount":{"default":{"accessToken":"discarded"},"default":null}}',
    "account-empty-reset":'{"oauthAccount":{"default":{"accessToken":"discarded"},"default":{}}}',
    "root-null-reset":'{"oauthAccount":{"default":{"accessToken":"discarded"}},"oauthAccount":null,"oauthAccount":{"spare":{"accessToken":"new"}}}',
    "root-empty-merge":'{"oauthAccount":{"default":{"accessToken":"retained"}},"oauthAccount":{}}',
    "root-replace-account":'{"oauthAccount":{"default":{"accessToken":"discarded"}},"oauthAccount":{"default":{},"spare":{"accessToken":"new"}}}',
    "wrong-before-correct":'{"oauthAccount":{"default":{"accessToken":4,"accessToken":"new"}}}',
    "wrong-hidden-account":'{"oauthAccount":{"default":{"accessToken":"good"},"unused":{"accessToken":false}}}',
    "case-fold-null":'{"OAUTHACCOUNT":{"default":{"ACCESSTOKEN":"retained","accessToken":null}}}',
    "unicode-fold":'{"oauthAccount":{"default":{"acceſſToKen":"unicode-token"}}}',
    "unknown-overflow":'{"oauthAccount":{"default":{"accessToken":"retained","metadata":1e1000}},"unknown":1e1000}',
    "surrogate":'{"oauthAccount":{"default":{"accessToken":"test-\\ud800-token"}}}',
    "invalid-utf8":b'{"oauthAccount":{"default":{"accessToken":"test-\xed\xa0\x80-token"}}}',
    "identical-fallback-tokens":'{"oauthAccount":{"work":{"accessToken":"same"},"spare":{"accessToken":"same"}}}',
    "default-empty-single-fallback":'{"oauthAccount":{"default":{"accessToken":""},"spare":{"accessToken":"fallback"}}}',
    "default-case-sensitive":'{"oauthAccount":{"DEFAULT":{"accessToken":"only"}}}',
    "deep-unknown":'{"oauthAccount":{"default":{"accessToken":"retained"}},"ignored":'+'['*200+'0'+']'*200+'}',
}.items():add("claude",name,value)
for name,depth in [("depth-boundary",9999),("depth-overflow",10000)]:
    add("claude",name,'{"oauthAccount":{"default":{"accessToken":"retained"}},"ignored":'+'['*depth+'0'+']'*depth+'}')
for scheme in ["env://ABSENT","symvault://test/token","vault://test/token","keychain://test/account"]:
    add("claude","literal-"+scheme.split(':')[0],json.dumps({"oauthAccount":{"default":{"accessToken":scheme}}}))
claude='{"oauthAccount":{"default":{"accessToken":"file-token"}}}'
add("claude","env-precedence",claude,env_token="synthetic-env-token")
add("claude","env-error-precedence",claude,env_token="env://USAGE_FILES_ABSENT")
add("claude","missing-file","",present=False)
add("claude","oversize","x"*65537)
add("claude","boundary",claude+' '*(65536-len(claude)))
for name,value in {
    "root-null":"null","root-array":"[]","root-string":'"bad"',"root-empty":"{}",
    "top-token":'{"access_token":"top"}',"nested-token":'{"tokens":{"access_token":"nested"}}',
    "top-preferred":'{"access_token":"top","tokens":{"access_token":"nested"}}',
    "top-empty-fallback":'{"access_token":"","tokens":{"access_token":"nested"}}',
    "top-number-fallback":'{"access_token":4,"tokens":{"access_token":"nested"}}',
    "top-null-fallback":'{"access_token":null,"tokens":{"access_token":"nested"}}',
    "root-last-wins":'{"access_token":"old","access_token":"new"}',
    "root-null-clears":'{"access_token":"old","access_token":null}',
    "nested-last-wins":'{"tokens":{"access_token":"old","access_token":"new"}}',
    "nested-null-clears":'{"tokens":{"access_token":"old","access_token":null}}',
    "map-replaced-not-merged":'{"tokens":{"access_token":"old"},"tokens":{"metadata":4}}',
    "case-sensitive-top":'{"ACCESS_TOKEN":"ignored","tokens":{"access_token":"nested"}}',
    "case-sensitive-nested":'{"TOKENS":{"access_token":"ignored"},"tokens":{"ACCESS_TOKEN":"ignored"}}',
    "unknown-overflow":'{"access_token":"discarded","unknown":1e1000}',
    "known-overflow-before-correct":'{"access_token":1e1000,"access_token":"discarded"}',
    "unknown-large-finite":'{"access_token":"retained","unknown":1e308}',
    "unknown-underflow":'{"access_token":"retained","unknown":1e-1000}',
    "unknown-large-integer":'{"access_token":"retained","unknown":18446744073709551616}',
    "surrogate":'{"access_token":"test-\\ud800-token"}',
    "invalid-utf8":b'{"access_token":"test-\xed\xa0\x80-token"}',
    "truncated":'{"access_token":"discarded"',
    "deep-unknown":'{"access_token":"retained","ignored":'+'['*200+'0'+']'*200+'}',
    "depth-boundary":'{"access_token":"retained","ignored":'+'['*9999+'0'+']'*9999+'}',
    "depth-overflow":'{"access_token":"discarded","ignored":'+'['*10000+'0'+']'*10000+'}',
    "number-text-is-string":'{"access_token":"retained","ignored":"1e1000 [{ \\\" 1e1000"}',
}.items():add("codex",name,value)
for scheme in ["env://ABSENT","symvault://test/token","vault://test/token","keychain://test/account"]:
    add("codex","literal-"+scheme.split(':')[0],json.dumps({"tokens":{"access_token":scheme}}))
for override in ["explicit","empty"]:add("codex","home-"+override,'{"tokens":{"access_token":"override-token"}}',override=override)
codex='{"access_token":"file-token"}'
add("codex","env-precedence",codex,env_token="synthetic-env-token")
add("codex","env-error-precedence",codex,env_token="env://USAGE_FILES_ABSENT")
add("codex","missing-file","",present=False)
add("codex","oversize","x"*65537)
add("codex","boundary",codex+' '*(65536-len(codex)))
assert len({row['id']for row in cases})==len(cases)
pathlib.Path(sys.argv[1]).write_text(json.dumps(cases,indent=2)+'\n', encoding='utf-8')
print('Provider file cases:',len(cases),'Claude:',sum(x['provider']=='claude'for x in cases),'Codex:',sum(x['provider']=='codex'for x in cases))
