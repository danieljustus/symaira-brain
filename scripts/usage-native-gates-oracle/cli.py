#!/usr/bin/env python3
"""Real Go/native CLI comparison of former `symbrain usage` fallback gates (#768).

Every scenario is network-free by construction: either nothing is configured,
or the one configured credential carries a byte that Go's net/http refuses
before connecting. Both binaries run with a cleared environment in a disposable
HOME, the Rust binary gets a SYMBRAIN_GO_BINARY that fails loudly, and the
macOS Keychain, `security`, `ps` and every live endpoint stay untouched
(ANTHROPIC_OAUTH_TOKEN is always set when Claude is selected, so the Keychain
read is preempted exactly as in Go).

    cli.py GO_BINARY RUST_BINARY [--record FIXTURE]

`--record` stores the Go bytes so `cargo test -p symbrain-cli --test
usage_native_gates_cli` can re-prove the native side without a Go binary.
"""
import base64
import json
import os
import pathlib
import subprocess
import sys
import tempfile

CTL = "\x01"
KIMI_TOKEN = '{"access_token":"cli-token-a"}'


def file(path, contents):
    return {"path": path, "contents": contents}


def scenarios():
    out = []

    def add(id, ids, env=None, files=None, home="set", output="json"):
        out.append({"id": id, "ids": ids, "env": env or {}, "files": files or [], "home": home, "output": output})

    # Unusual base overrides with nothing configured: previously routed to Go.
    for i, base in enumerate(["http://unsupported.invalid", "https://API.example.com/v1?x=1", "https://api.example.com:8443/a/../b", "https://user@example.com", "https://exämple.com/%41", "https://api.example.com//"]):
        add(f"base-kimi-{i}", ["kimi"], {"KIMI_CODE_BASE_URL": base})
        add(f"base-nous-{i}", ["nous"], {"HERMES_PORTAL_BASE_URL": base})
        add(f"base-openrouter-{i}", ["openrouter"], {"OPENROUTER_API_URL": base})
    add("base-all-table", ["kimi", "nous", "openrouter"], {"KIMI_CODE_BASE_URL": "https://A.example.com/x?y", "HERMES_PORTAL_BASE_URL": "https://b.example.com/%zz", "OPENROUTER_API_URL": "https://c.example.com#f"}, output="table")
    # OpenCode workspace spellings without a cookie never start a fetch.
    for i, ws in enumerate(["wrk_é", "wrk_a/b", "wrk_a-b", "wrk_", "plain", "https://opencode.ai/workspace/wrk_x/billing"]):
        add(f"workspace-only-{i}", ["opencode"], {"OPENCODE_WORKSPACE_ID": ws})
    # Claude: the OAuth environment source preempts the Keychain in Go and natively.
    add("claude-oauth-unresolvable-reference", ["claude"], {"ANTHROPIC_OAUTH_TOKEN": "env://CLAUDE_ABSENT_768"})
    add("claude-oauth-empty-reference", ["claude"], {"ANTHROPIC_OAUTH_TOKEN": "env://"})
    # Credentials Go refuses to send: deterministic before any network.
    refused = [
        ("claude-env", ["claude"], {"ANTHROPIC_OAUTH_TOKEN": "tok" + CTL}, []),
        ("claude-admin-env", ["claude"], {"ANTHROPIC_OAUTH_TOKEN": "env://CLAUDE_ABSENT_768", "ANTHROPIC_ADMIN_KEY": "key" + CTL}, []),
        ("codex-env", ["codex"], {"CODEX_ACCESS_TOKEN": "tok" + CTL}, []),
        ("copilot-env", ["copilot"], {"COPILOT_ACCESS_TOKEN": "tok" + CTL}, []),
        ("copilot-file", ["copilot"], {}, [file(".config/github-copilot/apps.json", json.dumps({"github.com:1": {"oauth_token": "tok" + CTL}}))]),
        ("copilot-file-newline", ["copilot"], {}, [file(".config/github-copilot/apps.json", json.dumps({"github.com:1": {"oauth_token": "tok\nx"}}))]),
        ("cursor-env", ["cursor"], {"CURSOR_COOKIE": "a=" + CTL}, []),
        ("kimi-api-env", ["kimi"], {"KIMI_CODE_API_KEY": "key" + CTL}, []),
        ("kimi-web-env", ["kimi"], {"KIMI_AUTH_TOKEN": "key" + CTL}, []),
        ("kimi-cli-file", ["kimi"], {}, [file(".kimi-code/credentials/kimi-code.json", json.dumps({"access_token": "tok" + CTL}))]),
        ("kimi-device-control", ["kimi"], {}, [file(".kimi-code/credentials/kimi-code.json", KIMI_TOKEN), file(".kimi-code/device_id", "dev" + CTL + "ice")]),
        ("kimi-device-newline", ["kimi"], {}, [file(".kimi-code/credentials/kimi-code.json", KIMI_TOKEN), file(".kimi-code/device_id", "dev\nice")]),
        ("nous-env", ["nous"], {"NOUS_PORTAL_ACCESS_TOKEN": "tok" + CTL}, []),
        ("nous-file", ["nous"], {}, [file(".hermes/auth.json", json.dumps({"providers": {"nous": {"access_token": "tok" + CTL}}}))]),
        ("openrouter-env", ["openrouter"], {"OPENROUTER_API_KEY": "key" + CTL}, []),
        ("opencode-cookie", ["opencode"], {"OPENCODE_COOKIE": "a=" + CTL, "OPENCODE_WORKSPACE_ID": "wrk_abc"}, []),
        ("opencode-workspace-control", ["opencode"], {"OPENCODE_COOKIE": "auth=1", "OPENCODE_WORKSPACE_ID": "wrk_ab" + CTL + "c"}, []),
    ]
    for id, ids, env, files in refused:
        add("refused-" + id, ids, env, files)
        add("refused-" + id + "-table", ids, env, files, output="table")
    # Home resolution: HOME alone, relative when unset or empty.
    for mode in ["unset", "empty"]:
        add(f"home-{mode}-kimi-control", ["kimi"], {}, [file(".kimi-code/credentials/kimi-code.json", json.dumps({"access_token": "tok" + CTL}))], home=mode)
    add("home-userprofile-ignored", ["kimi", "nous"], {"USERPROFILE": "/nonexistent-768"})
    return out


def run(binary, scenario, scratch, fallback):
    home = pathlib.Path(scratch) / "home"
    home.mkdir()
    for entry in scenario["files"]:
        target = home / entry["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(entry["contents"])
    env = {"PATH": "/nonexistent-768"}
    if scenario["home"] == "set":
        env["HOME"] = str(home)
    elif scenario["home"] == "empty":
        env["HOME"] = ""
    for key, value in scenario["env"].items():
        env[key] = value.replace("$HOME", str(home))
    if fallback:
        env["SYMBRAIN_GO_BINARY"] = fallback
    flag = "--json" if scenario["output"] == "json" else "--output=table"
    result = subprocess.run([str(binary), "usage", flag, "--providers", ",".join(scenario["ids"])], env=env, cwd=home, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=60, check=False)
    return {"exit": result.returncode, "stdout_b64": base64.b64encode(result.stdout).decode(), "stderr_b64": base64.b64encode(result.stderr).decode()}


def main():
    args = sys.argv[1:]
    record = None
    if "--record" in args:
        index = args.index("--record")
        record = pathlib.Path(args[index + 1])
        del args[index:index + 2]
    go, rust = (pathlib.Path(a).resolve() for a in args)
    cases, failures = [], []
    sentinel = tempfile.NamedTemporaryFile("w", suffix="-go-fallback.sh", delete=False)
    sentinel.write("#!/bin/sh\necho 'unexpected Go fallback' >&2\nexit 97\n")
    sentinel.close()
    os.chmod(sentinel.name, 0o700)
    try:
        for scenario in scenarios():
            with tempfile.TemporaryDirectory(prefix="usage768-") as a, tempfile.TemporaryDirectory(prefix="usage768-") as b:
                expected = run(go, scenario, a, None)
                actual = run(rust, scenario, b, sentinel.name)
            match = expected == actual
            if not match:
                failures.append(scenario["id"])
            cases.append({**scenario, "go": expected, "matches": match})
    finally:
        os.unlink(sentinel.name)
    print(f"usage native gates CLI oracle: {len(cases) - len(failures)}/{len(cases)} byte/exit matches")
    for id in failures:
        print("  MISMATCH", id)
    if failures:
        sys.exit(1)
    if record:
        record.write_text(json.dumps({"schema_version": 1, "cases": cases}, indent=1, ensure_ascii=False) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
