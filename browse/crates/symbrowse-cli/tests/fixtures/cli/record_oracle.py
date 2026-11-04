#!/usr/bin/env python3
"""Records the Go `symbrowse` CLI against a recording fake daemon.

Every scenario runs the Go binary in an isolated HOME/cwd with a fake daemon
listening on the real session socket. The daemon answers every frame with the
scenario's canned response (per command, falling back to a default) and logs
the frames it received. The recorded observation (frames, stdout, stderr, exit
code, written files) is frozen in `cli_oracle.json`; the Rust test replays the
same scenarios without Go.

Usage (explicit generation, never part of a test run):
    record_oracle.py --go /path/to/symbrowse-go --out cli_oracle.json \
        --commit <worktree commit> --scratch <short isolated dir>
"""
import argparse
import json
import os
import shutil
import socket
import stat
import subprocess
import sys
import threading

OK = lambda data=None: {"success": True, **({"data": data} if data is not None else {})}
ERR = lambda code, message, hint="": {
    "success": False,
    "error": {"code": code, "message": message, **({"hint": hint} if hint else {})},
}

NAV = {"action": "open", "url": "https://example.com/", "title": "Example", "http_status": 200}
SNAP = {"snapshot_id": "s1", "tree": "- button \"Go\" [ref=e1]\n- link \"More\" [ref=e2]\n", "refs": {"e1": {"role": "button"}}, "hint": "use @e1"}
TABS = {"active": "t1", "tabs": [{"id": "t1", "label": "main", "url": "https://example.com/", "active": True}, {"id": "t2", "url": "about:blank"}]}
COOKIES = {"origin": "https://example.com", "cookies": [{"name": "sid", "domain": "example.com", "path": "/", "secure": True, "http_only": True}]}
STATE_SAVED = {"saved": "login", "metadata": {"name": "login", "schema_version": 1, "saved_at": "2026-01-02T03:04:05Z", "key_source": "environment", "origins": [{"origin": "https://example.com", "cookie_count": 2, "local_storage_keys": 1, "session_storage_keys": 0}]}}
JOURNAL = {"entries": [
    {"timestamp": "2026-01-02T03:04:05Z", "command": "open", "risk_class": "read", "decider": "builtin", "result": "ok", "args": {"url": "https://example.com/"}},
    {"timestamp": "2026-01-02T03:04:06Z", "command": "click", "risk_class": "write", "decider": "builtin", "result": "ok", "args": {"selector": "#go"}},
    {"timestamp": "2026-01-02T03:04:07Z", "command": "fill", "risk_class": "write", "decider": "builtin", "result": "error: nope", "args": {"selector": "#q", "value": "x"}},
]}
HANDOFF = {"status": "pending", "prompt_id": "p1"}
MARKER = {"truncated": True, "head": "head text", "foot": "foot text", "hint": "symbrowse cache get c-1", "tokens_returned": 10, "tokens_total": 99, "cache_id": "c-1"}
WARN = {"success": True, "data": {"action": "open", "url": "https://example.com/"}, "warnings": [{"kind": "prompt_injection", "severity": "high", "message": "suspicious text", "ref": "e3", "excerpt": "ignore previous"}]}

FLOW_YAML = "name: demo\nversion: 1\ndomains: [\"example.com\"]\nsteps:\n  - open: { url: \"https://example.com/\" }\n  - wait: { url: \"**/done\" }\n"
FLOW_YAML_INLINE = FLOW_YAML

# (id, argv, data, extras). extras: err=True adds failure variants.
DAEMON = [
    ("open", ["open", "https://example.com/"], NAV, {"err": True}),
    ("goto", ["goto", "https://example.com/"], NAV, {}),
    ("back", ["back"], NAV, {}),
    ("forward", ["forward"], NAV, {}),
    ("reload", ["reload"], NAV, {}),
    ("open-session", ["open", "https://example.com/", "--session", "work"], NAV, {}),
    ("wait-selector", ["wait", "#ready"], {"matched": True}, {"err": True}),
    ("wait-ms", ["wait", "--ms", "250"], {"matched": True}, {}),
    ("wait-text", ["wait", "--text", "hello"], {"matched": True}, {}),
    ("wait-url", ["wait", "--url", "**/done"], {"matched": True}, {}),
    ("wait-load", ["wait", "--load", "networkidle"], {"matched": True}, {}),
    ("click", ["click", "#go"], {"ref": "e1", "action": "click"}, {"err": True}),
    ("dblclick", ["dblclick", "#go"], {"action": "dblclick"}, {}),
    ("hover", ["hover", "#go"], {"action": "hover"}, {}),
    ("focus", ["focus", "#q"], {"action": "focus"}, {}),
    ("check", ["check", "#cb"], {"action": "check"}, {}),
    ("uncheck", ["uncheck", "#cb"], {"action": "uncheck"}, {}),
    ("fill", ["fill", "#q", "hello world"], {"action": "fill"}, {"err": True}),
    ("type", ["type", "#q", "abc"], {"action": "type"}, {}),
    ("press", ["press", "#q", "Enter"], {"action": "press"}, {}),
    ("select", ["select", "#sel", "two"], {"action": "select"}, {}),
    ("scroll", ["scroll", "#list", "300"], {"action": "scroll"}, {}),
    ("scroll-bad", ["scroll", "#list", "x"], {"action": "scroll"}, {}),
    ("scrollintoview", ["scrollintoview", "#footer"], {"action": "scrollintoview"}, {}),
    ("find-role", ["find", "role", "button", "click", "--name", "Go", "--exact"], {"ref": "e1", "action": "click"}, {"err": True}),
    ("find-text", ["find", "text", "Hello", "text"], {"action": "text", "value": "Hello"}, {}),
    ("find-fill", ["find", "label", "Email", "fill", "a@b.c"], {"ref": "e4", "action": "fill"}, {}),
    ("find-nth", ["find", "role", "link", "nth", "2"], {"ref": "e2", "action": "nth"}, {}),
    ("find-nth-bad", ["find", "role", "link", "nth", "zz"], {"ref": "e2"}, {}),
    ("snapshot", ["snapshot"], SNAP, {"err": True}),
    ("snapshot-flags", ["snapshot", "-i", "--compact", "--depth", "3", "--selector", "main", "--urls", "--diff", "--since", "s0", "--max-tokens", "200"], SNAP, {}),
    ("snapshot-marker", ["snapshot"], MARKER, {}),
    ("snapshot-warn", ["snapshot"], {"snapshot_id": "s2", "tree": "x", "refs": {}}, {"warnings": WARN["warnings"]}),
    ("screenshot", ["screenshot", "shot.png"], {"path": "shot.png", "bytes": 1234}, {"err": True}),
    ("screenshot-flags", ["screenshot", "--full", "--format", "jpeg", "--quality", "80", "--selector", "#a"], {"path": "x.jpg", "bytes": 12}, {}),
    ("eval", ["eval", "1+1"], {"result": 2}, {"err": True}),
    ("eval-base64", ["eval", "--base64", "MSsx"], {"result": 2}, {}),
    ("console-list", ["console", "list"], {"messages": [{"type": "log", "text": "hi"}]}, {}),
    ("console-list-max", ["console", "list", "--max-tokens", "50"], {"messages": []}, {}),
    ("console-clear", ["console", "clear"], {"cleared": True}, {}),
    ("errors-list", ["errors", "list"], {"errors": []}, {}),
    ("errors-clear", ["errors", "clear"], {"cleared": True}, {}),
    ("upload", ["upload", "#file", "a.txt", "b.txt"], {"uploaded": 2}, {}),
    ("downloads", ["downloads", "--dir", "dl"], {"downloads": []}, {}),
    ("network-requests", ["network", "requests", "--method", "GET", "--status", "200", "--type", "xhr", "--filter", "api", "--max-tokens", "100"], {"requests": []}, {}),
    ("network-request", ["network", "request", "r1"], {"id": "r1"}, {}),
    ("network-route", ["network", "route", "**/api/*", "--status", "201", "--body", "{}"], {"routed": True}, {}),
    ("network-route-abort", ["network", "route", "**/ads/*", "--abort"], {"routed": True}, {}),
    ("network-unroute", ["network", "unroute", "**/api/*"], {"removed": 1}, {}),
    ("network-unroute-all", ["network", "unroute"], {"removed": 2}, {}),
    ("network-har-start", ["network", "har", "start"], {"recording": True}, {}),
    ("network-har-stop", ["network", "har", "stop", "--output", "out.har", "--content", "embed"], {"har": {"log": {}}}, {}),
    ("session-list", ["session", "list"], {"sessions": ["default"]}, {}),
    ("session-info", ["session", "info"], {"session": "default", "pid": 1}, {}),
    ("daemon-status", ["daemon", "status"], {"running": True}, {}),
    ("daemon-stop", ["daemon", "stop"], {"stopping": True}, {}),
    ("tab-list", ["tab", "list"], TABS, {}),
    ("tab-new", ["tab", "new", "https://example.com/", "--label", "x"], {"id": "t3"}, {}),
    ("tab-switch", ["tab", "switch", "t2"], {"active": "t2"}, {}),
    ("tab-close", ["tab", "close", "t2"], {"closed": "t2"}, {}),
    ("tab-close-current", ["tab", "close"], {"closed": "t1"}, {}),
    ("tab-window", ["tab", "window", "window"], {"id": "w2"}, {}),
    ("frame-tree", ["frame", "tree"], {"frames": []}, {}),
    ("frame-main", ["frame", "main"], {"frame": "main"}, {}),
    ("frame-select", ["frame", "select", "f1"], {"frame": "f1"}, {}),
    ("dialog-status", ["dialog", "status"], {"open": False}, {}),
    ("dialog-accept", ["dialog", "accept", "yes"], {"accepted": True}, {}),
    ("dialog-dismiss", ["dialog", "dismiss"], {"dismissed": True}, {}),
    ("dialog-auto", ["dialog", "auto", "accept"], {"mode": "accept"}, {}),
    ("cookies-list", ["cookies", "list"], COOKIES, {}),
    ("cookies-set", ["cookies", "set", "sid", "abc", "--domain", "example.com", "--path", "/", "--secure", "--http-only"], {"set": True}, {}),
    ("cookies-set-curl", ["cookies", "set", "--curl", "curl -H 'Cookie: a=b' https://example.com/"], {"set": 1}, {}),
    ("cookies-clear", ["cookies", "clear", "sid", "--url", "https://example.com"], {"cleared": 1}, {}),
    ("storage-get", ["storage", "get", "local", "k"], {"value": "v"}, {}),
    ("storage-get-all", ["storage", "get", "session"], {"items": {}}, {}),
    ("storage-set", ["storage", "set", "local", "k", "v"], {"set": True}, {}),
    ("storage-clear", ["storage", "clear", "local"], {"cleared": True}, {}),
    ("set-viewport", ["set", "viewport", "800", "600", "2"], {"ok": True}, {}),
    ("set-geo", ["set", "geo", "52.5", "13.4"], {"ok": True}, {}),
    ("set-offline", ["set", "offline", "on"], {"ok": True}, {}),
    ("set-offline-bare", ["set", "offline"], {"ok": True}, {}),
    ("set-headers", ["set", "headers", "{\"X-A\":\"b\"}"], {"ok": True}, {}),
    ("set-media", ["set", "media", "dark"], {"ok": True}, {}),
    ("set-user-agent", ["set", "user-agent", "Test/1.0"], {"ok": True}, {}),
    ("set-device", ["set", "device", "iPhone 13"], {"ok": True}, {}),
    ("state-save", ["state", "save", "login"], STATE_SAVED, {"err": True}),
    ("state-load", ["state", "load", "login"], {"loaded": "login"}, {}),
    ("state-clear", ["state", "clear", "login"], {"cleared": "login"}, {}),
    ("state-show", ["state", "show", "login"], STATE_SAVED["metadata"], {}),
    ("state-list", ["state", "list"], {"states": ["login", "other"]}, {}),
    ("state-clean", ["state", "clean", "--older-than", "24h"], {"removed": ["old"]}, {}),
    ("a11y", ["a11y", "--tags", "wcag2a", "--selector", "main"], {"violations": []}, {}),
    ("diff-snapshot", ["diff", "snapshot"], {"added": [{"ref": "e9"}], "removed": [], "changed": [{"role": "button", "name": "Go"}]}, {}),
    ("diff-url", ["diff", "url", "https://a.example/", "https://b.example/"], {"added": [], "removed": [], "changed": []}, {}),
    ("diff-screenshot", ["diff", "screenshot", "--threshold", "0.05"], {"diff_ratio": 0.01}, {}),
    ("read", ["read", "https://example.com/"], {"html": "<html><head><title>T</title></head><body><h1>Hi</h1><p>Body text</p></body></html>", "title": "T", "url": "https://example.com/"}, {"err": True}),
    ("read-outline", ["read", "--outline"], {"html": "<html><head><title>T</title></head><body><h1>Hi</h1><h2>Sub</h2><p>x</p></body></html>", "title": "T", "url": "https://example.com/"}, {}),
    ("read-raw", ["read", "--raw", "--selector", "p"], {"html": "<html><body><p>Body text</p></body></html>", "title": "T", "url": "https://example.com/"}, {}),
    ("read-hint", ["read", "--engine-hint"], {"html": "<html><body><p>Body text</p></body></html>", "title": "T", "url": "https://example.com/", "js_required": False, "js_required_reason": "static"}, {}),
    ("get-text", ["get", "text", "#t"], {"text": "hello"}, {"err": True}),
    ("get-title", ["get", "title"], {"title": "T"}, {}),
    ("get-url", ["get", "url"], {"url": "https://example.com/"}, {}),
    ("get-html", ["get", "html", "#t"], {"html": "<b>x</b>"}, {}),
    ("get-value", ["get", "value", "#q"], {"value": "x"}, {}),
    ("get-attr", ["get", "attr", "#a", "href"], {"value": "/x"}, {}),
    ("get-count", ["get", "count", "li"], {"count": 3}, {}),
    ("get-box", ["get", "box", "#a"], {"x": 1, "y": 2, "width": 3, "height": 4}, {}),
    ("get-styles", ["get", "styles", "#a", "color", "display"], {"styles": {"color": "red"}}, {}),
    ("is-visible", ["is", "visible", "#a"], {"visible": True}, {}),
    ("is-enabled", ["is", "enabled", "#a"], {"enabled": True}, {}),
    ("is-checked", ["is", "checked", "#a"], {"checked": False}, {}),
    ("handoff", ["handoff", "--reason", "captcha", "--timeout", "2m"], HANDOFF, {"err": True}),
    ("journal-tail", ["journal", "tail", "--lines", "2"], JOURNAL, {}),
    ("journal-show", ["journal", "show"], JOURNAL, {}),
    ("oob-status", ["oob", "status"], {"active": True, "prompt": {"id": "p1", "kind": "captcha", "reason": "solve"}}, {}),
    ("oob-status-none", ["oob", "status"], {"active": False}, {}),
    ("policy-explain", ["policy", "explain", "click", "--url", "https://example.com/", "--mode", "balanced"], None, {}),
    ("auth-login", ["auth", "login", "example/login", "--url", "https://example.com/login"], {"logged_in": True}, {}),
    ("watch-take-over", ["watch", "--take-over", "--reason", "fix"], HANDOFF, {}),
    ("trace-export", ["trace", "export", "--out", "t.json"], JOURNAL, {"files_after": ["t.json"]}),
    ("trace-replay", ["trace", "replay", "in-trace.json"], {"total": 1, "matched": 1, "deviated": 0, "failed": 0}, {"files": {"in-trace.json": json.dumps({"schema_version": 1, "created_at": "2026-01-01T00:00:00Z", "session": "default", "steps": [{"command": "open", "url": "https://example.com/", "expected_url": "https://example.com/"}]})}}),
    ("flow-run", ["flow", "run", "demo", "--input", "q=x"], {"ok": True}, {"files": {"cwd/.symbrowse/flows/demo.yaml": FLOW_YAML}}),
    ("flow-run-dry", ["flow", "run", "demo", "--dry-run"], {"ok": True}, {"files": {"cwd/.symbrowse/flows/demo.yaml": FLOW_YAML}}),
    ("flow-record-start", ["flow", "record", "start"], {"recording": True}, {}),
    ("flow-record-stop", ["flow", "record", "stop"], {"draft": "name: x"}, {}),
    ("flow-record-status", ["flow", "record", "status"], {"recording": False}, {}),
]

FLOW_YAML = "name: demo\nversion: 1\ndomains: [\"example.com\"]\nsteps:\n  - open: { url: \"https://example.com/\" }\n  - wait: { url: \"**/done\" }\n"


def local_scenarios():
    s = []
    def add(id, argv, **kw):
        s.append({"id": id, "argv": argv, **kw})
    for f in ([], ["--json"], ["--output", "yaml"], ["--output", "json"]):
        add("version" + "".join(f), ["version"] + f)
    add("root-version-flag", ["--version"])
    add("root-help", ["--help"])
    add("root-noargs", [])
    add("help-cmd", ["help"])
    add("help-open", ["help", "open"])
    add("help-unknown", ["help", "nope"])
    add("unknown-command", ["frobnicate"])
    add("unknown-command-json", ["frobnicate", "--json"])
    add("unknown-flag", ["open", "--nope"])
    add("bad-output", ["version", "--output", "xml"])
    add("bad-output-json", ["open", "x", "--output", "xml", "--json"])
    add("open-noargs", ["open"], daemon="down")
    add("open-twoargs", ["open", "a", "b"])
    add("click-noargs", ["click"])
    add("fill-toomany", ["fill", "a", "b", "c"])
    add("find-few", ["find", "role", "x"])
    add("wait-none", ["wait"])
    add("wait-two", ["wait", "#a", "--ms", "1"])
    add("wait-neg", ["wait", "--ms", "-3"])
    add("wait-badload", ["wait", "--load", "weird"])
    add("get-noargs", ["get", "text"])
    add("get-attr-one", ["get", "attr", "#a"])
    add("tab-switch-noargs", ["tab", "switch"])
    add("state-save-noargs", ["state", "save"])
    add("set-viewport-few", ["set", "viewport", "800"])
    add("bad-session", ["open", "https://x", "--session", "bad/name"])
    add("handoff-noreason", ["handoff"])
    add("watch-takeover-noreason", ["watch", "--take-over"])
    add("storage-get-bad", ["storage", "get", "weird", "k"])
    add("set-headers-bad", ["set", "headers", "nope"])
    add("set-geo-bad", ["set", "geo", "x", "y"])
    add("daemon-down-open", ["open", "https://example.com/"], daemon="down")
    add("daemon-down-open-json", ["open", "https://example.com/", "--json"], daemon="down")
    add("daemon-down-status", ["daemon", "status"], daemon="down")
    add("daemon-down-list", ["session", "list"], daemon="down")
    add("daemon-down-session", ["click", "#a", "--session", "work"], daemon="down")
    add("mcp-list-profiles", ["mcp", "--list-profiles"])
    add("mcp-bad-tools", ["mcp", "--tools", "nope"])
    add("completion-bash", ["completion", "bash"])
    add("completion-zsh-nodesc", ["completion", "zsh", "--no-descriptions"])
    add("completion-fish", ["completion", "fish"])
    add("completion-powershell", ["completion", "powershell"])
    add("complete-root", ["__complete", ""])
    add("complete-sub", ["__complete", "get", ""])
    add("complete-flag", ["__complete", "open", "--"])
    for f in ([], ["--json"], ["--output", "yaml"]):
        add("config-show" + "".join(f), ["config", "show"] + f)
    add("config-show-flags", ["config", "show", "--log-level", "debug", "--config-dir", "cfg"])
    add("config-show-env", ["config", "show"], env={"SYMBROWSE_LOG_LEVEL": "warn", "SYMBROWSE_ALLOW_PRIVATE": "true"})
    add("config-show-file", ["config", "show"], files={"home/.config/symbrowse/config.toml": "log_level = \"error\"\ncache_ttl_hours = 5\n"})
    add("transport-compat-flag", ["daemon", "--transport", "compat"])
    for f in ([], ["--json"], ["--output", "yaml"]):
        add("cache-list-empty" + "".join(f), ["cache", "list"] + f)
        add("cache-clear-empty" + "".join(f), ["cache", "clear"] + f)
    add("cache-get-missing", ["cache", "get", "nope"])
    add("cache-get-missing-json", ["cache", "get", "nope", "--json"])
    add("cache-get-badrange", ["cache", "get", "x", "--range", "z-1"])
    add("cache-get-fetch-badkey", ["cache", "get", "fetch:zz"])
    add("cache-noargs", ["cache"])
    for f in ([], ["--json"], ["--output", "yaml"]):
        add("session-id-cwd" + "".join(f), ["session", "id", "--scope", "cwd", "--prefix", "agent"] + f)
        add("session-id-worktree" + "".join(f), ["session", "id"] + f)
    add("session-id-bad", ["session", "id", "--scope", "galaxy"])
    add("session-id-repo", ["session", "id", "--scope", "repo"])
    add("policy-explain-local", ["policy", "explain", "click"], daemon="down")
    add("policy-explain-bad-mode", ["policy", "explain", "click", "--mode", "weird"], daemon="down")
    add("policy-explain-unknown", ["policy", "explain", "frob"], daemon="down")
    add("policy-explain-json", ["policy", "explain", "open", "--url", "https://example.com", "--json"], daemon="down")
    add("flow-validate-ok", ["flow", "validate", "f.yaml"], files={"cwd/f.yaml": FLOW_YAML})
    add("flow-validate-ok-json", ["flow", "validate", "f.yaml", "--json"], files={"cwd/f.yaml": FLOW_YAML})
    add("flow-validate-ok-yaml", ["flow", "validate", "f.yaml", "--output", "yaml"], files={"cwd/f.yaml": FLOW_YAML})
    add("flow-validate-bad", ["flow", "validate", "f.yaml"], files={"cwd/f.yaml": "name: demo\nversion: 9\nsteps: []\n"})
    add("flow-validate-bad-json", ["flow", "validate", "f.yaml", "--json"], files={"cwd/f.yaml": "name: demo\nversion: 9\nsteps: []\n"})
    add("flow-validate-missing", ["flow", "validate", "nope.yaml"])
    add("flow-list-empty", ["flow", "list"])
    add("flow-list-empty-json", ["flow", "list", "--json"])
    add("flow-list-found", ["flow", "list"], files={"cwd/.symbrowse/flows/demo.yaml": FLOW_YAML, "home/.config/symbrowse/flows/glob.yaml": FLOW_YAML.replace("demo", "glob")})
    add("flow-list-found-json", ["flow", "list", "--json"], files={"cwd/.symbrowse/flows/demo.yaml": FLOW_YAML})
    add("flow-list-found-yaml", ["flow", "list", "--output", "yaml"], files={"cwd/.symbrowse/flows/demo.yaml": FLOW_YAML})
    add("flow-run-missing", ["flow", "run", "nope"])
    add("flow-run-badinput", ["flow", "run", "demo", "--input", "novalue"], files={"cwd/.symbrowse/flows/demo.yaml": FLOW_YAML})
    add("flow-record-bad", ["flow", "record", "weird"])
    add("trace-export-empty", ["trace", "export", "--out", "t.json"], responses={"journal.show": OK({"entries": []})})
    add("trace-replay-missing", ["trace", "replay", "nope.json"])
    add("trace-replay-badjson", ["trace", "replay", "bad.json"], files={"cwd/bad.json": "{"})
    add("trace-replay-badschema", ["trace", "replay", "old.json"], files={"cwd/old.json": "{\"schema_version\": 7, \"steps\": []}"})
    add("state-key-init-env", ["state", "key", "init"], env={"SYMBROWSE_ENCRYPTION_KEY": "0" * 64}, path="/var/empty")
    add("profiles", ["profiles"])
    add("profiles-json", ["profiles", "--json"])
    add("profiles-yaml", ["profiles", "--output", "yaml"])
    add("upgrade-help", ["upgrade", "--help"])
    add("batch-dry", ["batch", "--dry-run", "open https://example.com/", "snapshot"])
    add("batch-dry-json", ["batch", "--dry-run", "--json", "open https://example.com/", "snapshot"])
    add("batch-dry-yaml", ["batch", "--dry-run", "--output", "yaml", "open https://example.com/"])
    add("batch-run", ["batch", "open https://example.com/", "snapshot"])
    add("batch-run-json", ["batch", "--json", "open https://example.com/", "bogus cmd"])
    add("batch-bail", ["batch", "--bail", "bogus", "snapshot"])
    add("batch-stdin", ["batch"], stdin="[\"open https://example.com/\"]")
    add("batch-stdin-bad", ["batch"], stdin="{")
    add("batch-stdin-empty", ["batch"], stdin="")
    add("batch-stdin-array-empty", ["batch"], stdin="[]")
    add("batch-stdin-wrongtype", ["batch"], stdin="[1]")
    add("eval-stdin", ["eval", "--stdin"], stdin="1+2\n", responses={"eval": OK({"result": 3})})
    add("eval-noexpr", ["eval"])
    add("set-device-list", ["set", "device", "--list"], daemon="down")
    add("doctor-fake-chrome", ["doctor"], files={"home/fakechrome": "#!/bin/sh\necho 'Fake Chrome 1.2.3'\n"}, exec_files=["home/fakechrome"], env={"SYMBROWSE_EXECUTABLE_PATH": "$HOME/fakechrome"}, path="/var/empty", platform="darwin")
    add("doctor-fake-chrome-json", ["doctor", "--json"], files={"home/fakechrome": "#!/bin/sh\necho 'Fake Chrome 1.2.3'\n"}, exec_files=["home/fakechrome"], env={"SYMBROWSE_EXECUTABLE_PATH": "$HOME/fakechrome"}, path="/var/empty", platform="darwin")
    add("doctor-fix", ["doctor", "--fix"], files={"home/fakechrome": "#!/bin/sh\necho 'Fake Chrome 1.2.3'\n"}, exec_files=["home/fakechrome"], env={"SYMBROWSE_EXECUTABLE_PATH": "$HOME/fakechrome"}, path="/var/empty", platform="darwin")
    add("doctor-no-chrome", ["doctor"], env={"SYMBROWSE_EXECUTABLE_PATH": "$HOME/missing"}, path="/var/empty", platform="darwin")
    add("doctor-no-chrome-json", ["doctor", "--json"], env={"SYMBROWSE_EXECUTABLE_PATH": "$HOME/missing"}, path="/var/empty", platform="darwin")
    return s


def build_scenarios():
    scenarios = []
    for sid, argv, data, extra in DAEMON:
        base = {"argv": argv}
        default = OK(data) if data is not None else OK()
        if extra.get("warnings"):
            default = {"success": True, "data": data, "warnings": extra["warnings"]}
        common = {k: v for k, v in extra.items() if k in ("files", "files_after")}
        common["responses"] = {"*": default}
        scenarios.append({"id": sid + ".text", "argv": argv, **common})
        scenarios.append({"id": sid + ".json", "argv": argv + ["--json"], **common})
        scenarios.append({"id": sid + ".yaml", "argv": argv + ["--output", "yaml"], **common})
        if extra.get("err"):
            fail = {"*": ERR("not_found", "element not found: #go", "run snapshot first")}
            c2 = {k: v for k, v in common.items() if k != "responses"}
            scenarios.append({"id": sid + ".err", "argv": argv, "responses": fail, **c2})
            scenarios.append({"id": sid + ".errjson", "argv": argv + ["--json"], "responses": fail, **c2})
            scenarios.append({"id": sid + ".erryaml", "argv": argv + ["--output", "yaml"], "responses": fail, **c2})
    scenarios.extend(local_scenarios())
    # Default responses for local scenarios: success with empty data.
    for sc in scenarios:
        sc.setdefault("responses", {"*": OK({"ok": True})})
        sc.setdefault("daemon", "up")
    for sc in scenarios:
        i = sc["id"]
        if i.startswith(("read", "trace-export", "batch", "flow-list-found")):
            sc["mask"] = ["time", "duration"]
        if i.startswith("session-id"):
            sc["mask"] = ["hex16"]
        if i.startswith("flow-run"):
            sc["mask"] = ["hex16", "duration"]
    ids = [s["id"] for s in scenarios]
    assert len(ids) == len(set(ids)), "duplicate scenario ids"
    return scenarios


class FakeDaemon(threading.Thread):
    def __init__(self, path, responses):
        super().__init__(daemon=True)
        self.path, self.responses, self.frames = path, responses, []
        os.makedirs(os.path.dirname(path), mode=0o700, exist_ok=True)
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.bind(path)
        self.sock.listen(8)
        self.sock.settimeout(0.2)
        self.stop = False

    def run(self):
        while not self.stop:
            try:
                conn, _ = self.sock.accept()
            except socket.timeout:
                continue
            except OSError:
                return
            with conn:
                data = b""
                while not data.endswith(b"\n"):
                    chunk = conn.recv(65536)
                    if not chunk:
                        break
                    data += chunk
                try:
                    frame = json.loads(data)
                except ValueError:
                    frame = {"_raw": data.decode("utf-8", "replace")}
                self.frames.append(frame)
                resp = self.responses.get(frame.get("cmd")) or self.responses.get("*") or OK()
                conn.sendall(json.dumps(resp).encode() + b"\n")

    def close(self):
        self.stop = True
        self.sock.close()
        try:
            os.unlink(self.path)
        except OSError:
            pass


def mask_text(text, kinds):
    """Masks wall-clock dependent values; mirrored by the Rust replay test."""
    import re
    if "time" in kinds:
        text = re.sub(r"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?(Z|[+-]\d{2}:\d{2})", "<TIMESTAMP>", text)
    if "hex16" in kinds:
        text = re.sub(r"(?<![0-9a-f])[0-9a-f]{16}(?![0-9a-f])", "<HEX16>", text)
    if "duration" in kinds:
        text = re.sub(r"(duration_ms\"?:\s?)\d+", r"\g<1>0", text)
        text = re.sub(r"tokens_est: \d+", "tokens_est: 0", text)
    return text


def session_of(argv):
    for i, a in enumerate(argv):
        if a == "--session" and i + 1 < len(argv):
            return argv[i + 1]
    return "default"


def run_scenario(go, scratch, index, sc):
    root = os.path.join(scratch, "s%d" % index)
    shutil.rmtree(root, ignore_errors=True)
    home, cwd = os.path.join(root, "h"), os.path.join(root, "h", "cwd")
    os.makedirs(cwd)
    for rel, content in (sc.get("files") or {}).items():
        target = os.path.join(root, "h", rel.replace("home/", "", 1)) if rel.startswith("home/") else os.path.join(root, "h", rel)
        os.makedirs(os.path.dirname(target), exist_ok=True)
        with open(target, "w") as handle:
            handle.write(content)
        if rel in (sc.get("exec_files") or []):
            os.chmod(target, 0o755)
    env = {
        "HOME": home,
        "PATH": sc.get("path", "/usr/bin:/bin"),
        "TMPDIR": os.path.join(home, "tmp"),
        "LANG": "C",
        "SYMBROWSE_NO_AUTOSTART": "1",
    }
    os.makedirs(env["TMPDIR"], exist_ok=True)
    for key, value in (sc.get("env") or {}).items():
        env[key] = value.replace("$HOME", home)
    daemon = None
    if sc.get("daemon") == "up":
        sock = os.path.join(home, "Library", "Caches", "symbrowse", "run", session_of(sc["argv"]) + ".sock")
        if len(sock) < 100:
            daemon = FakeDaemon(sock, sc["responses"])
            daemon.start()
    try:
        proc = subprocess.run(
            [go] + sc["argv"], cwd=cwd, env=env, input=(sc.get("stdin") or "").encode(),
            capture_output=True, timeout=30,
        )
    finally:
        if daemon:
            daemon.close()
    def norm(text):
        text = text.decode("utf-8", "replace")
        real = os.path.realpath(root)
        return text.replace(real, "$ROOT").replace(root, "$ROOT")
    kinds = sc.get("mask") or []
    obs = {
        "stdout": mask_text(norm(proc.stdout), kinds),
        "stderr": mask_text(norm(proc.stderr), kinds),
        "exit": proc.returncode,
        "frames": json.loads(norm(json.dumps(daemon.frames).encode())) if daemon else [],
    }
    after = {}
    for rel in sc.get("files_after") or []:
        path = os.path.join(cwd, rel)
        if os.path.exists(path):
            after[rel] = mask_text(norm(open(path, "rb").read()), kinds)
    if after:
        obs["files_after"] = after
    shutil.rmtree(root, ignore_errors=True)
    return obs


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--go", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--commit", required=True)
    parser.add_argument("--scratch", required=True)
    parser.add_argument("--only")
    args = parser.parse_args()
    go_version = subprocess.run(["/opt/homebrew/bin/go", "version"], capture_output=True, text=True).stdout.strip()
    scenarios = build_scenarios()
    out = []
    for index, sc in enumerate(scenarios):
        if args.only and args.only not in sc["id"]:
            continue
        sc = dict(sc)
        sc["expected"] = run_scenario(args.go, args.scratch, index, sc)
        out.append(sc)
    json.dump({"scenarios": out}, open(args.out, "w"), indent=1, sort_keys=False)
    prov = {
        "tool": "record_oracle.py",
        "oracle": "Go symbrowse CLI (browse/cmd/symbrowse) built from the worktree commit",
        "worktree_commit": args.commit,
        "go_version": go_version,
        "platform": sys.platform,
        "go_binary": os.path.basename(args.go),
        "argv": sys.argv[1:],
        "scenario_count": len(out),
        "method": "recording fake daemon on the session socket; isolated HOME/cwd/TMPDIR; env -i style environment",
    }
    json.dump(prov, open(args.out.replace(".json", ".provenance.json"), "w"), indent=1)
    print("recorded", len(out), "scenarios")


if __name__ == "__main__":
    main()
