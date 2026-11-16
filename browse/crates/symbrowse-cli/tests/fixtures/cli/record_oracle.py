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



HUMAN_PAYLOADS = [
    ("nav-status", {"action": "open", "url": "https://example.com/", "http_status": 200}),
    ("nav-nostatus", {"action": "open", "url": "https://example.com/"}),
    ("nav-status0", {"action": "back", "url": "https://example.com/", "http_status": 0}),
    ("nav-status-frac", {"action": "open", "url": "https://example.com/", "http_status": 200.5}),
    ("nav-status-string", {"action": "open", "url": "https://example.com/", "http_status": "200"}),
    ("nav-no-url", {"action": "open", "url": ""}),
    ("nav-no-action", {"url": "https://example.com/", "http_status": 200}),
    ("nav-extra", {"action": "reload", "url": "https://example.com/", "title": "T", "http_status": 304}),
    ("snap-id", {"snapshot_id": "s1", "tree": "- button \"Go\" [ref=e1]\n- link [ref=e2]\n", "refs": {"e1": {"role": "button"}}}),
    ("snap-noid", {"tree": "- button\n", "refs": {}}),
    ("snap-id-norefs", {"snapshot_id": "s2", "tree": "- x"}),
    ("snap-tree-only", {"tree": "- x"}),
    ("snap-empty-tree", {"snapshot_id": "s3", "tree": "\n\n", "refs": {}}),
    ("snap-hint", {"snapshot_id": "s4", "tree": "- a", "refs": {}, "hint": "use @e1"}),
    ("snap-empty-id", {"snapshot_id": "", "tree": "- a", "refs": {}}),
    ("snap-tree-nonstring", {"snapshot_id": "s5", "tree": 3, "refs": {}}),
    ("snap-html", {"snapshot_id": "s6", "tree": "- <b>&</b>", "refs": {}}),
    ("diff-all", {"snapshot_id": "s7", "added": [{"ref": "e9"}], "removed": [{"refkey": "k1"}], "changed": [{"role": "button", "name": "Go"}], "hint": "h"}),
    ("diff-items", {"added": [{"role": "link"}, {"name": "only"}, {}, "str", 3, None, {"ref": ""}, {"role": "b", "name": "q\"x"}]}),
    ("diff-empty", {"added": [], "removed": [], "changed": []}),
    ("diff-removed-only", {"removed": [{"ref": "e1"}, {"ref": "e2"}]}),
    ("state-saved", {"saved": "login"}),
    ("state-saved-meta", {"saved": "login", "metadata": {"name": "login", "schema_version": 3, "saved_at": "2026-01-02T03:04:05Z", "expires_at": "2026-02-01T00:00:00Z", "key_source": "environment", "origins": [{"origin": "https://example.com", "cookie_count": 2, "local_storage_keys": 1, "session_storage_keys": 0}, "weird", {"origin": "x"}]}}),
    ("state-loaded", {"loaded": "login"}),
    ("state-loaded-meta", {"loaded": "login", "metadata": {"name": "login", "schema_version": 1.5}}),
    ("state-cleared", {"cleared": "login"}),
    ("state-cleared-bool", {"cleared": True}),
    ("state-removed", {"removed": ["a", "b"]}),
    ("state-removed-empty", {"removed": []}),
    ("state-removed-null", {"removed": None}),
    ("state-removed-mixed", {"removed": ["a", 1]}),
    ("state-states", {"states": ["login", "other"]}),
    ("state-states-empty", {"states": []}),
    ("state-metadata", {"metadata": {"name": "n", "schema_version": 2, "origins": []}}),
    ("state-metadata-empty", {"metadata": {}}),
    ("state-toplevel", {"name": "n", "schema_version": 3, "saved_at": "t", "key_source": "keychain", "origins": [{"origin": "https://a", "cookie_count": 1}]}),
    ("state-toplevel-noorigins", {"schema_version": 3, "key_source": "k"}),
    ("state-toplevel-bool", {"schema_version": True, "origins": []}),
    ("cookies-full", {"origin": "https://example.com", "cookies": [{"name": "sid", "value": "s3cret", "domain": ".example.com", "path": "/", "same_site": "Lax", "secure": True, "http_only": True, "session": True}]}),
    ("cookies-noorigin", {"cookies": [{"name": "a"}, {"domain": "d"}, "str", 4, {"secure": False, "http_only": False}]}),
    ("cookies-empty", {"origin": "https://example.com", "cookies": []}),
    ("cookies-null", {"origin": "", "cookies": None}),
    ("cookies-notlist", {"cookies": "x"}),
    ("tabs-basic", {"active": "t1", "tabs": [{"id": "t1", "label": "main", "url": "https://example.com/", "active": True}, {"id": "t2", "url": "about:blank"}]}),
    ("tabs-label-eq-id", {"tabs": [{"id": "t1", "label": "t1", "url": "u"}]}),
    ("tabs-empty-tab", {"tabs": [{}, "str", 3, {"active": False}, {"label": "L"}]}),
    ("tabs-none", {"active": "", "tabs": []}),
    ("tabs-quote-label", {"tabs": [{"id": "t1", "label": "a \"b\" \u00e9\n"}]}),
    ("tabs-notlist", {"tabs": 3}),
    ("generic-ordering", {"z": "last", "a": 1, "m": {"y": 1, "b": [1, 2, {"k": None}]}}),
    ("generic-html", {"html": "<a href=\"x\">&</a>", "nl": "line1\nline2", "u": "\u2028\u00e9\u2603"}),
    ("generic-empty-map", {}),
    ("generic-empty-list", []),
    ("generic-list", [1, "two", {"three": 3}, None, True, 1.5, 1e21, 12345678901234567890]),
    ("generic-string", "hello"),
    ("generic-string-multiline", "a\nb\n"),
    ("generic-number", 42),
    ("generic-float", 1.25),
    ("generic-bool", True),
    ("generic-nested-empty", {"a": {}, "b": [], "c": [[]], "d": [{}]}),
    ("generic-null-field", {"a": None}),
    ("generic-nodata", None),
    ("generic-trunc-marker", {"truncated": True, "tokens_returned": 90, "tokens_total": 18400, "cache_id": "out_abc", "hint": "symbrowse cache get out_abc --range 40-120", "head": "HEAD TEXT", "foot": "FOOT TEXT"}),
    ("generic-trunc-string", {"truncated": "yes"}),
    ("generic-trunc-missing", {"truncated": True, "head": "h"}),
    ("yaml-specials", {"colon": "a: b", "yes": "yes", "num": "123", "float": "1.5", "empty": "", "lead": " x", "trail": "x ", "hash": "# c", "dash": "- x", "quote": "it's", "dq": "say \"hi\"", "uni": "caf\u00e9 \u2603", "null": "null", "tilde": "~", "bool": "true", "octal": "0o17", "time": "2026-01-02", "multi": "a\nb", "multi-trail": "a\nb\n\n", "multi-lead": "  a\nb", "key with space": 1, "k:colon": 2, "123": 3, "true": 4}),
    ("yaml-numbers", {"i": 1, "neg": -5, "f": 0.1, "big": 12345678901234567890, "exp": 1e21, "small": 1e-7, "zero": 0, "fz": 0.0}),
    ("yaml-sequences", {"list": [[1, 2], [3, [4, 5]], {"a": 1, "b": [2]}, [], {}, None, "s"]}),
    ("yaml-emoji", {"a": "x\U0001F600y", "b": "\U0001F600", "c": "a\u00a0b", "d": "caf\u00e9"}),
    ("yaml-bom", {"a": "\ufeffa", "b": "x\ufeffy", "c": "\ufeff"}),
    ("yaml-docmarker", {"a": "---", "b": "...", "c": "--- x", "d": "-\nx", "e": "-", "f": "- ", "g": "?", "h": ": x", "i": "a #b", "j": "a#b"}),
    ("yaml-seq-multiline", {"list": ["a\nb", {"k": "x\ny"}, ["p\nq"]]}),
    ("yaml-long-line", {"a": "word " * 40, "b": "x" * 200, "c": "a b " * 30 + "\t"}),
    ("yaml-keys-special", {"a\nb": 1, "": 2, " lead": 3, "x#": 4, "- k": 5, "\U0001F600": 6, "k\tt": 7, "yes": 8, "null": 9, "1.5": 10}),
    ("yaml-nested-seq-map", {"l": [[{"a": 1, "b": [1, {"c": [2]}]}]], "m": [{"x": [[1], []]}, {"y": {"z": [{}]}}]}),
    ("yaml-top-seq", [{"a": 1}, {"b": []}, [1, [2]], "x"]),
    ("yaml-literal-variants", {"a": "x\n", "b": "x\n\n", "c": "\n", "d": "\nx", "e": " x\ny", "f": "x \ny", "g": "x\n y", "h": "x\n\ny", "i": "x\ty\nz", "j": "\n\n", "k": "x\n\n\n", "l": "x\r\ny", "m": "a\u2028b\nc", "n": "  \n", "o": "x\n  "}),
    ("yaml-key-order", {"e10": 1, "e2": 2, "a_b": 3, "aB": 4, "Z": 5, "e02": 6, "e1": 7, "10": 8, "9": 9, "x-1": 10, "x_1": 11, "x1": 12}),
    ("yaml-contract-mix", {"tab": "a\tb", "emoji": "x\U0001F600", "yes": "yes", "num": "123", "empty": "", "colon": "a: b", "bom": "\ufeffa", "d": "\nx", "c": "x\n\n"}),
    ("yaml-empty-and-null", {"e": "", "n": None, "s": " ", "t": "\t", "z": "0", "o": "0x1F", "f": "1e3", "b": "On"}),
]

# (field, global, project, env, flag) -- flag is the `config show --<flag>`
# option of the Go CLI; list values are TOML arrays / comma-separated env text.
CONFIG_MATRIX = [
    ("log_level", "info", "debug", "error", ("--log-level", "trace")),
    ("log_format", "json", "text", "json", ("--log-format", "flagfmt")),
    ("config_dir", "g-config", "p-config", "e-config", ("--config-dir", "f-config")),
    ("cache_dir", "g-cache", "p-cache", "e-cache", ("--cache-dir", "f-cache")),
    ("state_dir", "g-state", "p-state", "e-state", ("--state-dir", "f-state")),
    ("executable_path", "g-exe", "p-exe", "e-exe", ("--executable-path", "f-exe")),
    ("cdp_endpoint", "http://g:9222", "http://p:9222", "http://e:9222", None),
    ("engine", "static", "safari-attach", "safari-bidi", None),
    ("allowed_domains", ["g.example"], ["p.example", "*.p.example"], "e.example, *.e.example,,", None),
    ("ssrf_enabled", True, False, True, None),
    ("allow_private", True, False, True, None),
    ("headless", True, False, True, None),
    ("cache_ttl_hours", 11, 22, 33, None),
    ("fetch_robots", False, True, False, None),
    ("fetch_user_agent", "g-agent", "p-agent", "e-agent", None),
    ("fetch_no_cache", True, False, True, None),
    ("idle_timeout", 11, 22, 33, None),
    ("operation_timeout", 11, 22, 33, None),
    ("read_timeout", 11, 22, 33, None),
    ("state_expire_days", 11, 22, 33, None),
    ("autosave", "always", "never", "always", None),
    ("autosave_interval", 11, 22, 33, None),
    ("autosave_key", "g-key", "p-key", "e-key", None),
    ("upload_dirs", ["g-up"], ["p-up", "p-up2"], "e-up, e-up2", None),
    ("daemon_log", "g.log", "p.log", "e.log", None),
    ("approval_timeout", 11, 22, 33, None),
]
ENV_NAMES = {"ssrf_enabled": "SYMBROWSE_SSRF"}


def _toml(field, value):
    if isinstance(value, bool):
        return "%s = %s\n" % (field, "true" if value else "false")
    if isinstance(value, int):
        return "%s = %d\n" % (field, value)
    if isinstance(value, list):
        return "%s = [%s]\n" % (field, ", ".join(json.dumps(v) for v in value))
    return "%s = %s\n" % (field, json.dumps(value))


def _env_text(value):
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, list):
        return ",".join(value)
    return str(value)


def config_scenarios(add):
    """CFG-001/CFG-002/CFG-005: every setting through every layer of
    defaults < global TOML < project TOML < env < flags, XDG/HOME handling and
    engine validation. All run `config show --json` (or a daemon-down command
    whose error names the derived paths)."""
    glob_file = "home/.config/symbrowse/config.toml"
    proj_file = "cwd/.symbrowse.toml"
    for field, g, p, e, flag in CONFIG_MATRIX:
        env_name = ENV_NAMES.get(field, "SYMBROWSE_" + field.upper())
        layers = {"g": g, "p": p, "e": e}
        for combo in ("g", "p", "e", "gp", "ge", "pe", "gpe"):
            files, env = {}, {}
            if "g" in combo:
                files[glob_file] = _toml(field, g)
            if "p" in combo:
                files[proj_file] = _toml(field, p)
            if "e" in combo:
                env[env_name] = _env_text(e)
            kw = {}
            if files:
                kw["files"] = files
            if env:
                kw["env"] = env
            add("cfgm-%s-%s" % (field, combo), ["config", "show", "--json"], **kw)
        if flag:
            for combo in ("f", "ef", "pef", "gpef"):
                files, env = {}, {}
                if "g" in combo:
                    files[glob_file] = _toml(field, g)
                if "p" in combo:
                    files[proj_file] = _toml(field, p)
                if "e" in combo:
                    env[env_name] = _env_text(e)
                kw = {}
                if files:
                    kw["files"] = files
                if env:
                    kw["env"] = env
                add("cfgm-%s-%s" % (field, combo), ["config", "show", "--json", flag[0], flag[1]], **kw)
    # Empty environment values are ignored; invalid values fail with the source.
    add("cfgm-env-empty-ignored", ["config", "show", "--json"], files={glob_file: "log_level = \"info\"\n"}, env={"SYMBROWSE_LOG_LEVEL": "", "SYMBROWSE_READ_TIMEOUT": ""})
    for name, env in (
        ("bool", {"SYMBROWSE_HEADLESS": "maybe"}),
        ("int", {"SYMBROWSE_READ_TIMEOUT": "abc"}),
        ("int-zero", {"SYMBROWSE_READ_TIMEOUT": "0"}),
        ("int-neg-idle", {"SYMBROWSE_IDLE_TIMEOUT": "-1"}),
        ("int-zero-idle", {"SYMBROWSE_IDLE_TIMEOUT": "0"}),
        ("int-zero-op", {"SYMBROWSE_OPERATION_TIMEOUT": "0"}),
        ("int-zero-approval", {"SYMBROWSE_APPROVAL_TIMEOUT": "0"}),
        ("int-neg-expire", {"SYMBROWSE_STATE_EXPIRE_DAYS": "-1"}),
        ("int-neg-ttl", {"SYMBROWSE_CACHE_TTL_HOURS": "-5"}),
        ("autosave", {"SYMBROWSE_AUTOSAVE": "sometimes"}),
        ("engine", {"SYMBROWSE_ENGINE": "wat"}),
        ("bool-numeric", {"SYMBROWSE_HEADLESS": "1"}),
        ("bool-upper", {"SYMBROWSE_HEADLESS": "TRUE"}),
        ("bool-T", {"SYMBROWSE_FETCH_ROBOTS": "F"}),
        ("int-plus", {"SYMBROWSE_READ_TIMEOUT": "+5"}),
        ("int-space", {"SYMBROWSE_READ_TIMEOUT": " 5"}),
    ):
        add("cfgm-env-bad-" + name, ["config", "show", "--json"], env=env)
        add("cfgm-env-bad-%s-text" % name, ["config", "show"], env=env)
    for name, content in (
        ("toml-syntax", "log_level = \n"),
        ("toml-type", "read_timeout = \"x\"\n"),
        ("toml-bool-type", "headless = \"yes\"\n"),
        ("toml-unknown-key", "bogus = 1\nlog_level = \"info\"\n"),
        ("toml-engine", "engine = \"wat\"\n"),
        ("toml-zero-timeout", "operation_timeout = 0\n"),
        ("toml-neg", "idle_timeout = -1\n"),
        ("toml-autosave", "autosave = \"x\"\n"),
        ("toml-list-type", "allowed_domains = \"a\"\n"),
        ("toml-float", "read_timeout = 1.5\n"),
        ("toml-empty-file", ""),
        ("toml-comment-only", "# nothing\n"),
        ("toml-empty-list", "allowed_domains = []\nupload_dirs = []\n"),
        ("toml-empty-string", "log_level = \"\"\nautosave_key = \"\"\n"),
        ("toml-table", "[log_level]\nx = 1\n"),
    ):
        add("cfgm-global-" + name, ["config", "show", "--json"], files={glob_file: content})
        add("cfgm-project-" + name, ["config", "show", "--json"], files={proj_file: content})
    # Global file removed by an unreadable-path trick is not portable; the
    # project file is read relative to the working directory only.
    add("cfgm-project-in-parent-ignored", ["config", "show", "--json"], files={"project-parent.toml": ".", ".symbrowse.toml": "log_level = \"debug\"\n"})
    # CFG-002: HOME and every XDG variable present/absent/empty/relative/unclean.
    xdg_values = (
        ("abs", "$HOME/x"),
        ("slash", "$HOME/x/"),
        ("double", "$HOME//x"),
        ("dots", "$HOME/a/../x/./y"),
        ("rel", "rel"),
        ("rel-dot", "./rel"),
        ("rel-dotdot", "../rel"),
        ("empty", ""),
        ("root", "/"),
        ("space", "$HOME/with space"),
        ("unicode", "$HOME/caf\u00e9"),
    )
    for var, field in (("XDG_CONFIG_HOME", "config_dir"), ("XDG_CACHE_HOME", "cache_dir"), ("XDG_STATE_HOME", "state_dir")):
        for name, value in xdg_values:
            add("xdg-%s-%s" % (var.lower(), name), ["config", "show", "--json"], env={var: value})
    add("xdg-all-abs", ["config", "show", "--json"], env={"XDG_CONFIG_HOME": "$HOME/c", "XDG_CACHE_HOME": "$HOME/ca", "XDG_STATE_HOME": "$HOME/s"})
    add("xdg-all-empty", ["config", "show", "--json"], env={"XDG_CONFIG_HOME": "", "XDG_CACHE_HOME": "", "XDG_STATE_HOME": ""})
    add("xdg-all-rel", ["config", "show", "--json"], env={"XDG_CONFIG_HOME": "rc", "XDG_CACHE_HOME": "rca", "XDG_STATE_HOME": "rs"})
    add("xdg-config-global-file", ["config", "show", "--json"], env={"XDG_CONFIG_HOME": "$HOME/c"}, files={"home/c/symbrowse/config.toml": "log_level = \"info\"\n", glob_file: "log_level = \"warn\"\n"})
    add("xdg-config-rel-global-file", ["config", "show", "--json"], env={"XDG_CONFIG_HOME": "rc"}, files={"cwd/rc/symbrowse/config.toml": "log_level = \"info\"\n"})
    add("xdg-config-empty-falls-back-to-home", ["config", "show", "--json"], env={"XDG_CONFIG_HOME": ""}, files={glob_file: "log_level = \"info\"\n"})
    add("xdg-state-daemon-log", ["config", "show", "--json"], env={"XDG_STATE_HOME": "$HOME/s", "SYMBROWSE_STATE_DIR": "$HOME/override"})
    add("xdg-state-daemon-log-explicit", ["config", "show", "--json"], env={"XDG_STATE_HOME": "$HOME/s", "SYMBROWSE_DAEMON_LOG": "$HOME/d.log"})
    add("xdg-state-flag-daemon-log", ["config", "show", "--json", "--state-dir", "fs"], env={"XDG_STATE_HOME": "$HOME/s"})
    add("home-empty", ["config", "show", "--json"], env={"HOME": ""})
    add("home-empty-text", ["config", "show"], env={"HOME": ""})
    add("home-empty-xdg-set", ["config", "show", "--json"], env={"HOME": "", "XDG_CONFIG_HOME": "$HOME/c", "XDG_CACHE_HOME": "$HOME/ca", "XDG_STATE_HOME": "$HOME/s"})
    add("home-empty-version", ["version", "--json"], env={"HOME": ""})
    add("home-empty-open", ["open", "https://example.com/"], env={"HOME": ""}, daemon="down")
    add("home-trailing-slash", ["config", "show", "--json"], env={"HOME": "$HOME/"})
    add("home-dots", ["config", "show", "--json"], env={"HOME": "$HOME/./x/.."})
    add("home-relative", ["config", "show", "--json"], env={"HOME": "rel-home"})
    # Socket and daemon-log paths derived from the same roots (darwin: HOME based).
    for name, env in (
        ("default", {}),
        ("xdg-state", {"XDG_STATE_HOME": "$HOME/s"}),
        ("xdg-cache", {"XDG_CACHE_HOME": "$HOME/ca"}),
        ("xdg-runtime", {"XDG_RUNTIME_DIR": "$HOME/run"}),
        ("state-env", {"SYMBROWSE_STATE_DIR": "$HOME/se"}),
        ("log-env", {"SYMBROWSE_DAEMON_LOG": "$HOME/l.log"}),
        ("tmpdir", {"TMPDIR": "$HOME/tmpx"}),
    ):
        add("socket-" + name, ["daemon", "status", "--json"], env=env, daemon="down", platform="darwin")
        add("socket-%s-session" % name, ["open", "https://example.com/", "--session", "alt.1_x-y", "--json"], env=env, daemon="down", platform="darwin")
    add("socket-session-invalid-dot", ["open", "x", "--session", ".hidden"], daemon="down")
    add("socket-session-invalid-long", ["open", "x", "--session", "a" * 65], daemon="down")
    add("socket-session-64", ["open", "x", "--session", "a" * 64, "--json"], daemon="down", platform="darwin")
    add("socket-session-invalid-empty", ["open", "x", "--session", ""], daemon="down")
    add("socket-session-invalid-dash", ["open", "x", "--session", "-a"], daemon="down")
    add("socket-session-invalid-space", ["open", "x", "--session", "a b"], daemon="down")
    # CFG-005: engine selection (daemon/mcp flags validate; config engine from env/TOML/flag).
    for engine in ("chrome", "static", "safari-attach", "safari-bidi", "wat", "", "Chrome"):
        add("engine-mcp-flag-%s" % (engine or "empty"), ["mcp", "--engine", engine, "--list-profiles"])
        add("engine-mcp-flag-bad-tools-%s" % (engine or "empty"), ["mcp", "--engine", engine, "--tools", "nope"])
        add("engine-env-%s" % (engine or "empty"), ["config", "show", "--json"], env={"SYMBROWSE_ENGINE": engine})
        if engine:
            add("engine-toml-%s" % engine, ["config", "show", "--json"], files={proj_file: "engine = %s\n" % json.dumps(engine)})
    for engine in ("wat", "Chrome"):
        add("engine-daemon-flag-%s" % engine, ["daemon", "--engine", engine], daemon="down")
        add("engine-daemon-flag-%s-json" % engine, ["daemon", "--engine", engine, "--json"], daemon="down")
        add("engine-daemon-env-%s" % engine, ["daemon"], env={"SYMBROWSE_ENGINE": engine}, daemon="down")
        add("engine-daemon-toml-%s" % engine, ["daemon"], files={proj_file: "engine = %s\n" % json.dumps(engine)}, daemon="down")
        add("engine-mcp-env-%s" % engine, ["mcp"], env={"SYMBROWSE_ENGINE": engine})
        add("engine-mcp-toml-%s" % engine, ["mcp"], files={proj_file: "engine = %s\n" % json.dumps(engine)})
    add("engine-flag-beats-env-bad", ["mcp", "--engine", "static", "--list-profiles"], env={"SYMBROWSE_ENGINE": "wat"})
    add("engine-env-beats-toml-bad", ["config", "show", "--json"], env={"SYMBROWSE_ENGINE": "static"}, files={proj_file: "engine = \"wat\"\n"})
    add("engine-project-beats-global", ["config", "show", "--json"], files={glob_file: "engine = \"static\"\n", proj_file: "engine = \"safari-attach\"\n"})
    add("engine-default-source", ["config", "show"])


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
    contract_scenarios(add)
    return s


def contract_scenarios(add):
    """Contract rows CLI-002..CLI-006 (browse contract matrix): aliases, argument
    validation, inherited output-flag placement, eval input resolution, batch."""
    # CLI-002: goto is the compatibility alias of open (separate cobra command,
    # same argv shape and same frame except the command name).
    add("alias-goto-session", ["goto", "https://example.com/", "--session", "work"])
    add("alias-goto-noargs", ["goto"], daemon="down")
    add("alias-goto-twoargs", ["goto", "a", "b"])
    add("alias-goto-json-before", ["--json", "goto", "https://example.com/"])
    add("alias-goto-help", ["goto", "--help"])
    add("alias-help-goto", ["help", "goto"])
    add("batch-alias-goto", ["batch", "--json", "goto https://example.com/", "open https://example.com/"])
    # CLI-003: missing, extra, repeated and -- terminated arguments.
    add("args-version-extra", ["version", "extra"])
    add("args-version-extra-json", ["version", "extra", "--json"])
    add("args-state-save-noargs", ["state", "save"])
    add("args-state-save-two", ["state", "save", "a", "b"])
    add("args-state-save-dd", ["state", "save", "--", "login"])
    add("args-state-load-noargs", ["state", "load"])
    add("args-open-dd", ["open", "--", "https://example.com/"])
    add("args-open-dd-dash", ["open", "--", "-weird"])
    add("args-open-dd-two", ["open", "--", "a", "b"])
    add("args-open-dd-flag-literal", ["open", "--", "--json"])
    add("args-open-dd-after-url", ["open", "https://example.com/", "--"])
    add("args-open-dd-then-flag", ["open", "https://example.com/", "--", "--json"])
    add("args-open-session-dd", ["open", "--session", "work", "--", "https://example.com/"])
    add("args-click-dd-dash", ["click", "--", "-x"])
    add("args-fill-dd", ["fill", "--", "#q", "-v"])
    add("args-fill-dd-short", ["fill", "#q", "--", "-v"])
    add("args-get-text-dd", ["get", "text", "--", "#a"])
    add("args-tab-switch-two", ["tab", "switch", "a", "b"])
    add("args-daemon-status-extra", ["daemon", "status", "extra"])
    add("args-session-list-extra", ["session", "list", "extra"])
    add("args-cookies-list-extra", ["cookies", "list", "extra"])
    add("args-snapshot-extra", ["snapshot", "extra"])
    add("args-snapshot-depth-repeat", ["snapshot", "--depth", "1", "--depth", "2"])
    add("args-snapshot-depth-bad", ["snapshot", "--depth", "x"])
    add("args-snapshot-depth-missing", ["snapshot", "--depth"])
    add("args-wait-ms-repeat", ["wait", "--ms", "1", "--ms", "2"])
    add("args-session-repeat", ["open", "https://example.com/", "--session", "a", "--session", "work"])
    add("args-session-eq", ["open", "https://example.com/", "--session=work"])
    add("args-session-missing", ["open", "https://example.com/", "--session"])
    add("args-flag-eq-bool", ["snapshot", "--compact=true"])
    add("args-flag-eq-bool-false", ["snapshot", "--compact=false"])
    add("args-flag-eq-bool-bad", ["snapshot", "--compact=maybe"])
    add("args-help-flag-extra", ["open", "--help", "extra"])
    add("args-short-help", ["open", "-h"])
    add("args-unknown-short", ["open", "-z"])
    add("args-unknown-nested", ["state", "frobnicate"])
    add("args-unknown-nested-json", ["state", "frobnicate", "--json"])
    add("args-group-noargs", ["state"])
    add("args-group-noargs-json", ["state", "--json"])
    # CLI-004: inherited --json/--output placement, = syntax and repeats.
    for place, argv in (
        ("root-first", ["--json", "config", "show"]),
        ("between", ["config", "--json", "show"]),
        ("last", ["config", "show", "--json"]),
        ("out-root-first", ["--output", "yaml", "config", "show"]),
        ("out-between", ["config", "--output", "yaml", "show"]),
        ("out-last", ["config", "show", "--output", "yaml"]),
        ("out-eq-root", ["--output=yaml", "config", "show"]),
        ("out-eq-between", ["config", "--output=yaml", "show"]),
        ("out-eq-last", ["config", "show", "--output=yaml"]),
        ("out-eq-json", ["config", "show", "--output=json"]),
        ("out-eq-text", ["config", "show", "--output=text"]),
        ("out-eq-bad", ["config", "show", "--output=xml"]),
        ("out-bad-root", ["--output", "xml", "config", "show"]),
        ("out-missing-last", ["config", "show", "--output"]),
        ("out-eq-empty", ["config", "show", "--output="]),
        ("json-eq-true", ["config", "show", "--json=true"]),
        ("json-eq-false", ["config", "show", "--json=false"]),
        ("json-eq-bad", ["config", "show", "--json=maybe"]),
        ("json-repeat", ["config", "show", "--json", "--json"]),
        ("out-repeat", ["config", "show", "--output", "yaml", "--output", "json"]),
        ("out-repeat-rev", ["config", "show", "--output", "json", "--output", "yaml"]),
        ("json-then-out", ["config", "show", "--json", "--output", "yaml"]),
        ("out-then-json", ["config", "show", "--output", "yaml", "--json"]),
        ("json-false-then-out", ["config", "show", "--json=false", "--output", "yaml"]),
        ("json-both-ends", ["--json", "config", "show", "--output", "yaml"]),
        ("version-json-first", ["--json", "version"]),
        ("version-out-eq-first", ["--output=yaml", "version"]),
        ("version-json-out-yaml", ["version", "--json", "--output", "yaml"]),
        ("version-out-yaml-json", ["version", "--output", "yaml", "--json"]),
        ("version-out-text-json", ["version", "--output", "text", "--json"]),
        ("version-json-false-out-json", ["version", "--json=false", "--output", "json"]),
        ("version-dd-json", ["version", "--", "--json"]),
        ("unknown-json-first", ["--json", "frobnicate"]),
        ("nested-json-first", ["--json", "state", "frobnicate"]),
        ("nested-unknown-flag-json", ["config", "--json", "show", "--nope"]),
        ("help-json", ["--json", "--help"]),
        ("help-sub-out", ["config", "--output", "yaml", "--help"]),
    ):
        add("flags-" + place, argv)
    for place, argv in (
        ("open-json-first", ["--json", "open", "https://example.com/"]),
        ("open-json-between", ["open", "--json", "https://example.com/"]),
        ("open-out-first", ["--output", "yaml", "open", "https://example.com/"]),
        ("open-out-between", ["open", "--output=yaml", "https://example.com/"]),
        ("open-json-session-first", ["--json", "--session", "work", "open", "https://example.com/"]),
        ("session-list-json-between", ["session", "--json", "list"]),
        ("tab-list-out-between", ["tab", "--output", "yaml", "list"]),
        ("cookies-json-between", ["cookies", "--json", "list"]),
        ("get-json-between", ["get", "--json", "title"]),
        ("get-out-eq-between", ["get", "--output=yaml", "title"]),
        ("snapshot-json-first-flags", ["--json", "snapshot", "--compact"]),
    ):
        add("flags-daemon-" + place, argv)
    add("flags-daemon-open-err-json-first", ["--json", "open", "https://example.com/"], responses={"*": ERR("not_found", "element not found: #go", "run snapshot first")})
    add("flags-daemon-open-err-out-eq", ["--output=yaml", "open", "https://example.com/"], responses={"*": ERR("not_found", "element not found: #go", "run snapshot first")})
    add("flags-daemon-down-json-first", ["--json", "open", "https://example.com/"], daemon="down")
    # CLI-005: eval input resolution.
    add("eval-stdin-json", ["eval", "--stdin", "--json"], stdin="1+1", responses={"eval": OK({"result": 2})})
    add("eval-stdin-yaml", ["eval", "--stdin", "--output", "yaml"], stdin="1+1", responses={"eval": OK({"result": 2})})
    add("eval-stdin-crlf", ["eval", "--stdin"], stdin="1+2\r\n", responses={"eval": OK({"result": 3})})
    add("eval-stdin-empty", ["eval", "--stdin"], stdin="")
    add("eval-stdin-blank", ["eval", "--stdin"], stdin="  \n")
    add("eval-stdin-multiline", ["eval", "--stdin"], stdin="const a = 1;\na + 1\n")
    add("eval-stdin-with-arg", ["eval", "--stdin", "1+1"], stdin="2+2")
    add("eval-stdin-with-arg-before", ["eval", "1+1", "--stdin"], stdin="2+2")
    add("eval-stdin-with-base64", ["eval", "--stdin", "--base64", "MSsx"], stdin="2+2")
    add("eval-base64-with-arg", ["eval", "--base64", "MSsx", "3+3"])
    add("eval-base64-bad", ["eval", "--base64", "!!!"])
    add("eval-base64-bad-json", ["eval", "--base64", "!!!", "--json"])
    add("eval-base64-empty", ["eval", "--base64", ""])
    add("eval-base64-padded-newline", ["eval", "--base64", "MSsx\n"])
    add("eval-base64-url-alphabet", ["eval", "--base64", "Pz8_"])
    add("eval-base64-unpadded", ["eval", "--base64", "MQ"])
    add("eval-base64-multiline", ["eval", "--base64", "YSA9IDE7CmEgKyAx"])
    add("eval-base64-invalid-utf8", ["eval", "--base64", "/w=="])
    add("eval-base64-whitespace-only", ["eval", "--base64", "IA=="])
    add("eval-two-args", ["eval", "1+1", "2+2"])
    add("eval-empty-arg", ["eval", ""])
    add("eval-dd-expr", ["eval", "--", "-1"])
    add("eval-expr-flaglike", ["eval", "--", "--stdin"])
    add("eval-json-noexpr", ["eval", "--json"])
    add("eval-session", ["eval", "1+1", "--session", "work"])
    add("eval-noexpr-down", ["eval"], daemon="down")
    # CLI-006: batch (JSON-producing version, mcp --list-profiles, --bail, dry-run).
    add("batch-version-json", ["batch", "--json", "version --json", "version"])
    add("batch-version-text", ["batch", "--json", "version"])
    add("batch-mcp-profiles", ["batch", "--json", "mcp --list-profiles"])
    add("batch-mcp-profiles-text", ["batch", "mcp --list-profiles"])
    add("batch-mixed-json", ["batch", "--json", "version --json", "bogus cmd", "open https://example.com/", "config show --json"])
    add("batch-mixed-yaml", ["batch", "--output", "yaml", "version --json", "bogus cmd", "open https://example.com/"])
    add("batch-bail-json", ["batch", "--bail", "--json", "version --json", "bogus cmd", "open https://example.com/"])
    add("batch-bail-first-ok", ["batch", "--bail", "--json", "version --json", "open https://example.com/"])
    add("batch-bail-yaml", ["batch", "--bail", "--output", "yaml", "bogus", "version"])
    add("batch-bail-text", ["batch", "--bail", "version", "bogus cmd", "version"])
    add("batch-dry-bail-json", ["batch", "--dry-run", "--bail", "--json", "bogus cmd", "version"])
    add("batch-dry-version", ["batch", "--dry-run", "--json", "version --json"])
    add("batch-dry-mcp-profiles", ["batch", "--dry-run", "--json", "mcp --list-profiles", "open https://example.com/"])
    add("batch-dry-stdin", ["batch", "--dry-run", "--json"], stdin="[\"open https://example.com/\", \"snapshot\"]")
    add("batch-dry-nothing-sent", ["batch", "--dry-run", "click #a", "fill #q x"], daemon="down")
    add("batch-down-json", ["batch", "--json", "open https://example.com/", "version --json"], daemon="down")
    add("batch-down-bail-json", ["batch", "--bail", "--json", "open https://example.com/", "version --json"], daemon="down")
    add("batch-stdin-json-mixed", ["batch", "--json"], stdin="[\"version --json\", \"bogus cmd\", \"snapshot\"]")
    add("batch-stdin-bail", ["batch", "--bail", "--json"], stdin="[\"bogus\", \"snapshot\"]")
    add("batch-args-over-stdin", ["batch", "--json", "version --json"], stdin="[\"snapshot\"]")
    add("batch-quoted-args", ["batch", "--json", "fill #q 'hello world'", "eval \"1 + 1\""])
    add("batch-empty-item", ["batch", "--json", "", "version --json"])
    add("batch-blank-item", ["batch", "--json", "   ", "version --json"])
    add("batch-unterminated-quote", ["batch", "--json", "fill #q 'oops", "version --json"])
    add("batch-session-flag", ["batch", "--json", "open https://example.com/", "--session", "work"])
    add("batch-item-output-flag", ["batch", "--json", "version --output yaml", "version --output json"])
    add("batch-item-session", ["batch", "--json", "open https://example.com/ --session work"])
    add("batch-help-item", ["batch", "--json", "help open"])
    add("batch-noargs-tty", ["batch"], stdin="")
    # OUT-003: every dedicated human renderer branch (text) and the same payloads
    # as --output yaml, driven through a daemon response so both formats see
    # identical data.
    for name, data in HUMAN_PAYLOADS:
        responses = {"*": OK(data) if data is not None else OK()}
        add("human-" + name, ["open", "https://example.com/"], responses=responses)
        add("human-yaml-" + name, ["open", "https://example.com/", "--output", "yaml"], responses=responses)
        add("human-json-" + name, ["open", "https://example.com/", "--json"], responses=responses)
    config_scenarios(add)
    # CLI-005: printEvalResult variants and the -b shorthand.
    for name, data in (("exception", {"exception_text": "ReferenceError: x"}), ("null", {"value": None}), ("string", {"value": "hi \"there\" <b>"}), ("number", {"value": 3.5}), ("int", {"value": 42}), ("bool", {"value": False}), ("object", {"value": {"b": 1, "a": [1, 2]}}), ("array", {"value": [1, "x", None]}), ("missing", {}), ("empty-exception", {"exception_text": "", "value": 7})):
        add("eval-print-" + name, ["eval", "x"], responses={"eval": OK(data)})
    add("eval-short-b", ["eval", "-b", "MSsx"])
    add("eval-short-b-stdin", ["eval", "-b", "--stdin"], stdin="MSsx")
    add("eval-b-stdin-trailing-newline", ["eval", "--base64", "--stdin"], stdin="MSsx\n")
    # OUT-003: yaml.v3 style choice for control characters (tab) in strings.
    for name, value in (("tab-single", "a\tb"), ("tab-lead", "\tab"), ("tab-trail", "ab\t"), ("tab-multi", "a\tb\nc"), ("tab-multi-trail", "a\tb\nc\n"), ("tab-only", "\t"), ("cr-multi", "a\r\nb"), ("esc-single", "a\u001bb"), ("esc-multi", "a\u001bb\nc"), ("nel-single", "a\u0085b"), ("bom-single", "\ufeffa")):
        add("yaml-" + name, ["eval", "1", "--output", "yaml"], responses={"eval": OK({"result": value})})


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
        text = re.sub(r"(durationms\"?:\s?)\d+", r"\g<1>0", text)
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
    # The toolchain that built the oracle binary (embedded build info), not the
    # one installed on the recording host.
    go_version = subprocess.run(["go", "version", args.go], capture_output=True, text=True).stdout.strip()
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
