"""Supplement historical immutable fixtures with typed Hermes/JWT contracts."""
import base64
import json
import pathlib
import sys

frozen = pathlib.Path("rust/symbrain-usage/tests/fixtures/nous_file_token_oracle.json")
cases = [{"id": "frozen-" + case["id"], "data_hex": case.get("contents", "").encode().hex(), "file_present": case["file_present"]} for case in json.loads(frozen.read_text(encoding='utf-8'))["cases"]]
def add(name, value):
    data = value if isinstance(value, bytes) else value.encode()
    cases.append({"id": name, "data_hex": data.hex(), "file_present": True})
def token(payload):
    data = payload if isinstance(payload, bytes) else payload.encode()
    return "header." + base64.urlsafe_b64encode(data).decode().rstrip("=") + ".signature"
def jwt(name, payload):
    add("jwt-" + name, json.dumps({"providers": [{"id": "nous", "invoke_jwt": token(payload)}]}))

for name, value in {
    "root-null": "null", "root-array": "[]", "root-string": '"bad"',
    "providers-null": '{"providers":null}', "providers-empty": '{"providers":[]}',
    "providers-bool": '{"providers":true}', "provider-null": '{"providers":[null]}',
    "provider-string": '{"providers":["bad"]}',
    "duplicate-null-string": '{"providers":[{"id":"nous","access_token":"retained","access_token":null}]}',
    "alias-null-string": '{"providers":[{"ID":"nous","ACCESS_TOKEN":"retained","access_token":null}]}',
    "duplicate-empty-string": '{"providers":[{"id":"nous","access_token":"old","access_token":""}]}',
    "wrong-type-before-correct": '{"providers":[{"id":"nous","access_token":4,"access_token":"new"}]}',
    "wrong-type-unrelated": '{"providers":[{"id":"other","access_token":4},{"id":"nous","access_token":"new"}]}',
    "first-empty-nous": '{"providers":[{"id":"nous"},{"id":"nous","access_token":"later"}]}',
    "reuse-provider-slice": '{"providers":[{"id":"nous","access_token":"retained"}],"PROVIDERS":[{"id":"nous"}]}',
    "reuse-null-entry": '{"providers":[{"id":"nous","access_token":"retained"}],"providers":[null]}',
    "truncate-provider-slice": '{"providers":[{"id":"other"},{"id":"nous","access_token":"discarded"}],"providers":[{"id":"other"}]}',
    "reset-provider-slice": '{"providers":[{"id":"nous","access_token":"discarded"}],"providers":null,"providers":[{"id":"nous"}]}',
    "reset-empty-slice": '{"providers":[{"id":"nous","access_token":"discarded"}],"providers":[],"providers":[{"id":"nous"}]}',
    "unicode-field-fold": '{"providerſ":[{"id":"nous","acceſſ_toKen":"unicode-fold"}]}',
    "unknown-overflow-number": '{"providers":[{"id":"nous","access_token":"retained","unknown":1e1000}],"version":1e1000}',
    "escaped-surrogate": '{"providers":[{"id":"nous","access_token":"test-\\ud800-token"}]}',
    "unknown-surrogate": '{"providers":[{"id":"nous","access_token":"retained","unknown":"\\ud800"}]}',
    "invalid-utf8": b'{"providers":[{"id":"nous","access_token":"test-\xed\xa0\x80-token"}]}',
    "regular-file-boundary": '{"providers":[{"id":"nous","access_token":"retained"}],"padding":"' + 'x' * (65536 - len('{"providers":[{"id":"nous","access_token":"retained"}],"padding":""}')) + '"}',
    "oversized-file": 'x' * 65537,
}.items(): add("file-" + name, value)
for name, payload in {
    "canonical-future": '{"exp":4102444800}', "canonical-expired": '{"exp":1}',
    "case-folded": '{"EXP":4102444800}', "escaped-key": '{"e\\u0078p":4102444800}',
    "duplicate-expired-live": '{"exp":1,"EXP":4102444800}',
    "duplicate-live-expired": '{"exp":4102444800,"EXP":1}',
    "null-after-live": '{"exp":4102444800,"exp":null}',
    "live-after-null": '{"exp":null,"EXP":4102444800}',
    "wrong-type-before-live": '{"exp":"wrong","exp":4102444800}',
    "wrong-type-after-live": '{"exp":4102444800,"exp":false}',
    "negative": '{"exp":-4102444800}', "fractional-future": '{"exp":4102444800.9}',
    "unknown-overflow-number": '{"exp":4102444800,"ignored":1e1000}',
    "unknown-surrogate": '{"exp":4102444800,"ignored":"\\ud800"}',
    "invalid-utf8-ignored": b'{"exp":4102444800,"ignored":"\xed\xa0\x80"}',
    "null": 'null', "empty": '{}', "array": '[]', "truncated": '{"exp":4',
    "exp-overflow": '{"exp":1e1000}', "string-exp": '{"exp":"4102444800"}',
}.items(): jwt(name, payload)
payload = base64.urlsafe_b64encode(b'{"exp":4102444800,"z":0}').decode().rstrip('=')
for name, encoded in {"crlf": payload[:4] + '\r\n' + payload[4:], "space-invalid": payload[:4] + ' ' + payload[4:], "padded-invalid": payload + '=', "one-tail-invalid": 'A', "noncanonical-tail": payload[:-1] + "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"["ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_".index(payload[-1]) ^ 1]}.items():
    add("base64-" + name, json.dumps({"providers": [{"id": "nous", "invoke_jwt": "header." + encoded + ".signature"}]}))
add("file-deep-ignored-metadata", '{"providers":[{"id":"nous","access_token":"retained","unknown":' + '[' * 200 + '0' + ']' * 200 + '}]}')
# Retain Go backing slots across nonempty shrinkage and later growth.
for name, value in {'shrink-regrow-null-slot': '{"providers":[{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{"id":"other"}],"providers":[{},null]}',
 'shrink-regrow-object-slot': '{"providers":[{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{"id":"other"}],"providers":[{},{}]}',
 'shrink-regrow-scalar-null': '{"providers":[{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{}],"providers":[{},{"access_token":null}]}',
 'shrink-hides-old-nous': '{"providers":[{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{}]}',
 'shrink-regrow-null-reset': '{"providers":[{"id":"other"},{"id":"nous","access_token":"discarded"}],"providers":[{}],"providers":null,"providers":[{},null]}',
 'shrink-regrow-empty-reset': '{"providers":[{"id":"other"},{"id":"nous","access_token":"discarded"}],"providers":[{}],"providers":[],"providers":[{},null]}',
 'shrink-regrow-clear-token': '{"providers":[{"id":"other"},{"id":"nous","access_token":"discarded"}],"providers":[{}],"providers":[{},{"access_token":""}]}',
 'shrink-regrow-replace-id': '{"providers":[{"id":"other"},{"id":"nous","access_token":"hidden"}],"providers":[{}],"providers":[{},{"id":"other"}]}',
 'shrink-regrow-prefer-invoke': '{"providers":[{"id":"other"},{"id":"nous","invoke_jwt":"invoke-token","access_token":"access-token"}],"providers":[{}],"providers":[{},null]}',
 'shrink-regrow-multiple-cycles': '{"providers":[{"id":"other"},{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{}],"providers":[{},null],"providers":[null],"providers":[null,{},null]}',
 'shrink-regrow-grow-past-history': '{"providers":[{"id":"other"},{"id":"nous","access_token":"retained"}],"providers":[{}],"providers":[{},null,null,null,null]}',
 'shrink-regrow-fresh-tail': '{"providers":[{"id":"other"},{"id":"other","access_token":"unselected"}],"providers":[{}],"providers":[{},null,{"id":"nous","access_token":"new-token"}]}'}.items():
    add("slice-" + name, value)
# Both typed entry points enforce Go's total 10000-container limit.
# Append boundary cases; retain every original 92 input and ID unchanged.
for arrays in (9999, 10000):
    add("file-total-depth-" + str(arrays + 1), '{"providers":[{"id":"nous","access_token":"retained"}],"ignored":' + '[' * arrays + '0' + ']' * arrays + '}')
    payload = '{"exp":4102444800,"ignored":' + '[' * arrays + '0' + ']' * arrays + '}'
    add("jwt-total-depth-" + str(arrays + 1), json.dumps({"providers": [{"id": "nous", "invoke_jwt": token(payload)}]}, separators=(',', ':')))
assert len({case['id'] for case in cases}) == len(cases)
pathlib.Path(sys.argv[1]).write_text(json.dumps(cases, indent=2) + '\n', encoding='utf-8')
print(f"Hermes source-bound cases: {len(cases)}")
