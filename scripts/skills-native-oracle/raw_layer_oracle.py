#!/usr/bin/env python3
"""Record or verify the MCP raw-Skills (exact JSON error / EOF) Go oracle (#764).

`record` drives the real Go `symbrain mcp` binary (built from the pinned ref's
unmodified Go sources) over every case below, in an isolated HOME/XDG tree, and
writes the observed JSON-RPC replies to the fixture the Rust integration test
`skills_raw_oracle` replays against the native binary. `check` records into a
temporary file and fails unless it equals the committed fixture, so the fixture
can never drift from what Go actually does. Nothing is hand-written: every
expected value in the fixture is a Go reply.

    python3 scripts/skills-native-oracle/raw_layer_oracle.py record|check \
        [--go-binary PATH]      # default: build ./cmd/symbrain with `go build`
"""
import argparse
import hashlib
import json
import os
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
FIXTURE = ROOT / "rust/symbrain-cli/tests/fixtures/skills_raw_go_oracle.json"
GO_SOURCES = ("cmd", "internal", "go.mod", "go.sum")
TOOLS = ("skills_list", "skills_inspect", "skills_validate", "skills_render_plan",
         "skills_install", "skills_targets_status")  # the six main implements

START = b'{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{'
BOOT = [b'{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"raw-oracle","version":"0"}}}',
        b'{"jsonrpc":"2.0","method":"notifications/initialized"}',
        b'{"jsonrpc":"2.0","id":2,"method":"tools/list"}']


def request(params, outer=b""):
    return b'{"jsonrpc":"2.0","id":3,"method":"tools/call",' + outer + b'"params":' + params + b"}"


def wire(body, framed):
    lines = [*BOOT, body]
    if framed:
        return b"".join(b"Content-Length: %d\r\n\r\n%s" % (len(line), line) for line in lines)
    return b"\n".join(lines) + b"\n"


def review_files(directory, prefix):
    rows = []
    for path in sorted((HERE / directory).iterdir()):
        framed = path.suffix == ".framed"
        raw = path.read_bytes()
        body = raw.split(b"\r\n\r\n", 1)[1] if framed else raw[:-1]
        rows.append((f"{prefix}-{path.stem}-{'framed' if framed else 'line'}", body, framed))
    return rows


def eof_cases():
    partial = [("minus", b"-")]
    partial += [(n, t) for n, t in [("zero-dot", b"0."), ("dot", b"1.")]]
    partial += [(n, t) for n, t in [("exponent", b"1e"), ("upper-exponent", b"1E"), ("exp-plus", b"1e+"),
                                    ("exp-minus", b"1e-"), ("signed-exp", b"-0E+")]]
    partial += [(f"{w}-{k}", w[:k].encode()) for w in ("true", "false", "null") for k in range(1, len(w))]
    partial += [("escape", b'"abc\\')]
    partial += [(f"unicode-{k}", b'"\\u' + b"0" * k) for k in range(4)]
    generic = [("object", b"{"), ("array", b"["), ("string", b'"abc'),
               ("unicode-complete-unclosed-string", b'"\\u1234'), ("complete-number", b"1"),
               ("complete-literal", b"true"), ("complete-string", b'"abc"')]
    valid = [("minus", b"-1"), ("fraction", b"1.0"), ("exponent", b"1e+2"), ("true", b"true"),
             ("false", b"false"), ("null", b"null"), ("string", b'"abc"'), ("unicode", b'"\\u1234"')]
    prefixes = {
        "meta": START + b'"name":"skills_list","_meta":{"n":',
        "arguments": START + b'"name":"skills_list","arguments":{"ignored":',
        "syntax-before-type": START + b'"name":4,"name":"skills_list","_meta":[],"arguments":{"ignored":',
    }
    rows = [(f"eof-{pos}-{n}", prefix + token) for pos, prefix in prefixes.items() for n, token in partial]
    rows += [("eof-generic-" + n, prefixes["meta"] + t) for n, t in generic]
    rows += [("eof-valid-" + n, request(b'{"name":"skills_list","arguments":{},"_meta":{"n":' + t + b"}}"))
             for n, t in valid]
    return rows


def json_cases():
    rows = []
    for depth in (100, 128, 9996, 9997, 9998):
        nested = b"[" * depth + b"0" + b"]" * depth
        rows.append((f"depth-{depth}-arguments",
                     request(b'{"name":"skills_list","arguments":{"ignored":' + nested + b"}}")))
        rows.append((f"depth-{depth}-meta",
                     request(b'{"name":"skills_list","arguments":{},"_meta":{"n":' + nested + b"}}")))
        rows.append((f"depth-{depth}-envelope", request(b'{"name":"skills_list","arguments":{}}', b'"ignored":' + nested + b",")))
    long = b"0." + b"0" * 10000 + b"1e100000"
    for n, t in [("negative-long", b"-" + long), ("negative-zero", b"-0"), ("negative-underflow", b"-1e-9999"),
                 ("positive-underflow", b"1e-9999"), ("negative-overflow", b"-1e9999"),
                 ("finite-max", b"1.7976931348623157e308"), ("boundary-overflow", b"1.7976931348623159e308"),
                 ("least-subnormal", b"5e-324"), ("tie-underflow", b"2.4703282292062327e-324")]:
        rows.append(("float-" + n, request(b'{"name":"skills_list","arguments":{},"_meta":{"n":' + t + b"}}")))
    for n, content in [("quoted-containers", rb'{"n":"[{\\\"}]","x":0}'),
                       ("duplicate-first-negative-overflow", rb'{"n":-1e9999,"n":0,"x":[1e9999]}'),
                       ("duplicate-first-positive-overflow", rb'{"n":1e9999,"n":-1e9999}'),
                       ("numeric-key-is-not-number", rb'{"1e9999":"1e9999","0x1p2":false}'),
                       ("nested-first-error", rb'{"n":[{"first":-1e9999}],"second":1e9999}')]:
        rows.append((n, request(b'{"name":"skills_list","arguments":{},"_meta":' + content + b"}")))
    for n, content in [("plus", b"+1"), ("leading-zero", b"01"), ("hex", b"0x1p2"), ("underscore", b"1_0"),
                       ("nan", b"NaN"), ("inf", b"Inf"), ("trailing-comma", b"[1,]"), ("missing-colon", b'{"n" 1}')]:
        rows.append(("syntax-before-type-" + n,
                     request(b'{"name":4,"name":"skills_list","_meta":[],"arguments":' + content + b"}")))
    return rows


def envelope_cases():
    """Typed outer-envelope failures and valid calls for each implemented tool."""
    rows = [("envelope-name-number", request(b'{"name":4,"arguments":{}}')),
            ("envelope-name-array-then-valid", request(b'{"name":[],"name":"skills_list","arguments":{}}')),
            ("envelope-meta-string", request(b'{"name":"skills_list","_meta":"x"}')),
            ("envelope-meta-array", request(b'{"name":"skills_list","_meta":[1]}')),
            ("envelope-meta-overflow", request(b'{"name":"skills_list","_meta":{"n":1e9999}}')),
            ("envelope-meta-case-folded", request(b'{"NAME":"skills_list","_META":{"n":1e9999}}')),
            ("envelope-jsonrpc-number", b'{"jsonrpc":2,"id":3,"method":"tools/call","params":{"name":"skills_list"}}'),
            ("envelope-method-object", b'{"jsonrpc":"2.0","id":3,"method":{},"params":{"name":"skills_list"}}'),
            ("envelope-ignored-arg-overflow", request(b'{"name":"skills_list","arguments":{"ignored":1e9999}}')),
            ("envelope-lone-surrogate-name", request(b'{"name":"\\ud800","arguments":{}}'))]
    for tool in TOOLS:
        rows.append((f"ok-{tool}", request(b'{"name":"%s","arguments":{}}' % tool.encode())))
        rows.append((f"meta-overflow-{tool}", request(b'{"name":"%s","arguments":{},"_meta":{"n":1e9999}}' % tool.encode())))
    return rows


# Calls whose `name` never resolves to one of the eleven Skills owners belong to
# the inherited generic decoder (not this layer; see the #764 JSON ADR). Go's
# replies are still recorded; the Rust test asserts they DIVERGE today so the
# residual cannot silently disappear or be mistaken for parity.
OTHER_OWNER = ("envelope-name-number", "envelope-lone-surrogate-name")


def all_cases():
    rows = []
    for ident, body, framed in review_files("eof-review-inputs", "wire-eof") + review_files("json-review-inputs", "wire-json"):
        rows.append({"id": ident, "mode": "framed" if framed else "line", "body": body})
    for ident, body in eof_cases() + json_cases() + envelope_cases():
        for framed in (False, True):
            rows.append({"id": f"{ident}-{'framed' if framed else 'line'}", "mode": "framed" if framed else "line", "body": body})
    ids = [row["id"] for row in rows]
    assert len(ids) == len(set(ids)), "duplicate case ids"
    return rows


def rle(body):
    """Compact body: [hex, count] pairs; a count > 1 repeats a single byte. Lossless."""
    out, i = [], 0
    while i < len(body):
        j = i
        while j < len(body) and body[j] == body[i]:
            j += 1
        if j - i >= 16:
            out.append([body[i:i + 1].hex(), j - i])
            i = j
        else:
            k = i
            while k < len(body):
                m = k
                while m < len(body) and body[m] == body[k]:
                    m += 1
                if m - k >= 16:
                    break
                k = m
            out.append([body[i:k].hex(), 1])
            i = k
    assert b"".join(bytes.fromhex(h) * n for h, n in out) == body
    return out


def profile(path):
    path.write_text('[profile]\nname = "raw-skills"\n[servers.vault]\nenabled = false\n[servers.memory]\n'
                    'enabled = false\n[servers.usage]\nenabled = false\n[servers.skills]\nenabled = true\n'
                    '[audit]\nenabled = false\n')


def replies(output, framed):
    if not framed:
        return [json.loads(line) for line in output.split(b"\n") if line]
    result = []
    while output:
        head, _, rest = output.partition(b"\r\n\r\n")
        size = int(head.split(b":")[1])
        result.append(json.loads(rest[:size]))
        output = rest[size:]
    return result


def run(binary, case):
    framed = case["mode"] == "framed"
    with tempfile.TemporaryDirectory(prefix="raworacle-") as tmp:
        tmp = Path(tmp)
        for name in ("home", "config", "data", "cache", "project", "tmp"):
            (tmp / name).mkdir()
        profile(tmp / "profile.toml")
        env = {"PATH": "", "HOME": tmp / "home", "USERPROFILE": tmp / "home", "XDG_CONFIG_HOME": tmp / "config",
               "XDG_DATA_HOME": tmp / "data", "XDG_CACHE_HOME": tmp / "cache", "TMPDIR": tmp / "tmp"}
        done = subprocess.run([str(binary), "mcp", "--profile-file", str(tmp / "profile.toml")],
                              input=wire(case["body"], framed), capture_output=True, cwd=tmp / "project",
                              env={k: str(v) for k, v in env.items()}, timeout=30, check=False)
    assert done.returncode == 0, (case["id"], done)
    return replies(done.stdout, framed)


def view(replies_):
    """Compared view of the third reply: parse/params errors in full; calls by outcome."""
    assert len(replies_) == 3, replies_
    reply = replies_[2]
    if "error" in reply:
        return {"id": reply["id"], "error": reply["error"]}
    return {"id": reply["id"], "isError": reply["result"].get("isError")}


def git(*args):
    return subprocess.run(["git", "-C", str(ROOT), *args], capture_output=True, text=True, check=True).stdout.strip()


def build(ref):
    out = Path(tempfile.mkdtemp(prefix="raworacle-go-")) / "symbrain"
    subprocess.run(["go", "build", "-o", str(out), "./cmd/symbrain"], cwd=ROOT, check=True)
    return out


def produce(binary, ref):
    for source in GO_SOURCES:
        if subprocess.run(["git", "-C", str(ROOT), "diff", "--quiet", ref, "--", source]).returncode != 0:
            sys.exit(f"Go sources differ from pinned ref {ref}: {source}")
    go_version = subprocess.run(["go", "version"], capture_output=True, text=True, check=True).stdout.strip()
    cases = []
    for case in all_cases():
        extra = {"known_divergence": "non-skills name owner"} if case["id"].rsplit("-", 1)[0] in OTHER_OWNER else {}
        cases.append({**extra, "id": case["id"], "mode": case["mode"], "body_rle": rle(case["body"]),
                      "body_sha256": hashlib.sha256(case["body"]).hexdigest(), "go": view(run(binary, case))})
    return {"source": "real Go `symbrain mcp` replies, never hand-written", "pinned_ref": ref,
            "go_trees": {s: git("rev-parse", f"{ref}:{s}") for s in GO_SOURCES}, "go_version": go_version,
            "bootstrap": [line.decode() for line in BOOT], "cases": cases}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("mode", choices=("record", "check"))
    parser.add_argument("--go-binary")
    parser.add_argument("--ref", help="pinned commit whose Go sources are the oracle (required to record)")
    args = parser.parse_args()
    ref = args.ref
    if ref is None and args.mode == "check":
        ref = json.loads(FIXTURE.read_text())["pinned_ref"]
    if not ref:
        sys.exit("--ref <pinned commit> is required to record")
    ref = git("rev-parse", ref)
    binary = Path(args.go_binary) if args.go_binary else build(ref)
    data = produce(binary, ref)
    text = json.dumps(data, indent=1, sort_keys=True) + "\n"
    if args.mode == "record":
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(text)
        print(f"recorded {len(data['cases'])} cases from {ref} ({data['go_version']})")
    else:
        if FIXTURE.read_text() != text:
            sys.exit("fixture differs from the real Go replies")
        print(f"fixture matches real Go: {len(data['cases'])} cases")


if __name__ == "__main__":
    main()
