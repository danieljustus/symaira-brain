#!/usr/bin/env python3
"""Differential CLI corpus for the symbrowse command tree.

  gen_cases.py gen   <binary> <source-sha> <out.json>   # record from the Go oracle
  gen_cases.py check <binary> <cases.json> [id-prefix]  # replay against any binary

Every case runs in a fresh sandbox (HOME/XDG/TMPDIR under one root, LANG=C,
TZ=UTC, SYMBROWSE_NO_AUTOSTART=1). A scripted fake daemon listens on the
session socket, records every frame the CLI sends and answers with the canned
responses of the case. Outputs are normalised: the sandbox root becomes
<ROOT> and the socket path <SOCKET>.
"""
import hashlib, json, os, platform, re, shutil, socket, subprocess, sys, tempfile, threading

SCRATCH = os.environ.get("SYMBROWSE_CASE_SCRATCH", "/tmp/sbc")


class FakeDaemon(threading.Thread):
    def __init__(self, path, responses):
        super().__init__(daemon=True)
        self.path, self.responses, self.frames, self.index = path, responses, [], 0
        os.makedirs(os.path.dirname(path), exist_ok=True)
        if os.path.exists(path):
            os.unlink(path)
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.bind(path)
        self.sock.listen(8)
        self.sock.settimeout(0.2)
        self.stop = False

    def run(self):
        while not self.stop:
            try:
                conn, _ = self.sock.accept()
            except (socket.timeout, OSError):
                continue
            with conn:
                conn.settimeout(5)
                data = b""
                try:
                    while b"\n" not in data:
                        chunk = conn.recv(65536)
                        if not chunk:
                            break
                        data += chunk
                except OSError:
                    continue
                if not data.strip():
                    continue
                self.frames.append(json.loads(data.split(b"\n")[0]))
                response = self.responses[min(self.index, len(self.responses) - 1)]
                self.index += 1
                conn.sendall(json.dumps(response, separators=(",", ":")).encode() + b"\n")

    def close(self):
        self.stop = True
        self.sock.close()


def run_case(binary, case, number):
    root = os.path.join(SCRATCH, "c%d" % number)
    shutil.rmtree(root, ignore_errors=True)
    for d in ("home", "work", "tmp", "state", "cache", "config", "data", "run"):
        os.makedirs(os.path.join(root, d), exist_ok=True)
    home = os.path.join(root, "home")
    env = {
        "HOME": home, "USERPROFILE": home, "XDG_CONFIG_HOME": root + "/config",
        "XDG_DATA_HOME": root + "/data", "XDG_CACHE_HOME": root + "/cache",
        "XDG_STATE_HOME": root + "/state", "XDG_RUNTIME_DIR": root + "/run",
        "TMPDIR": root + "/tmp", "LANG": "C", "LC_ALL": "C", "TZ": "UTC", "TERM": "dumb",
        "NO_COLOR": "1", "PATH": "/usr/bin:/bin", "SYMBROWSE_NO_AUTOSTART": "1",
    }
    env.update({k: v.replace("<ROOT>", root) for k, v in case.get("env", {}).items()})
    for rel, text in case.get("files", {}).items():
        path = os.path.join(root, "work", rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w") as f:
            f.write(text)
    daemon = None
    session = case.get("session", "default")
    if platform.system() == "Darwin":
        sock = os.path.join(home, "Library/Caches/symbrowse/run", session + ".sock")
    else:
        sock = os.path.join(root, "run/symbrowse", session + ".sock")
    if case.get("daemon") is not None:
        daemon = FakeDaemon(sock, case["daemon"])
        daemon.start()
    argv = [a.replace("<ROOT>", root) for a in case["argv"]]
    try:
        p = subprocess.run([binary] + argv, cwd=root + "/work", env=env, capture_output=True,
                           input=(case.get("stdin") or "").encode(), timeout=20)
    except subprocess.TimeoutExpired:
        return {"stdout": "<TIMEOUT>", "stderr": "", "exit": -1, "frames": []}
    finally:
        if daemon:
            daemon.close()

    def norm(text):
        text = text.decode("utf-8", "replace").replace(sock, "<SOCKET>").replace(root, "<ROOT>")
        return re.sub(r"time=\d{4}-\d\d-\d\dT[\d:.]+Z ", "time=<TS> ", text)
    frames = json.loads(norm(json.dumps(daemon.frames).encode())) if daemon else []
    return {"stdout": norm(p.stdout), "stderr": norm(p.stderr), "exit": p.returncode, "frames": frames}


OK = lambda data=None, warnings=None: {k: v for k, v in {"success": True, "data": data, "warnings": warnings}.items() if v is not None}
ERR = lambda code, message, hint=None, details=None: {"success": False, "error": {k: v for k, v in {
    "code": code, "message": message, "hint": hint, "details": details}.items() if v is not None}}


def build_cases(tree):
    cases = []
    add = lambda cid, argv, **kw: cases.append(dict(id=cid, argv=argv, **kw))
    nodes = tree["nodes"]
    leaves = [n for n in nodes if not n["children"] and n["path"] and n["path"][0] != "compat-sidecar"
              and n["path"][0] not in ("help", "completion")]
    no_exec = {"upgrade", "mcp", "daemon", "watch", "doctor", "batch"}
    # --- help and bare group invocation
    for n in nodes:
        if n["path"] and n["path"][0] == "compat-sidecar":
            continue
        p = n["path"]
        add("help/" + "_".join(p or ["root"]), p + ["--help"])
        if p:
            add("helpcmd/" + "_".join(p), ["help"] + p)
            add("short-help/" + "_".join(p), p + ["-h"])
        if n["children"]:
            if p != ["daemon"]:  # bare `daemon` runs the daemon itself
                add("bare/" + "_".join(p or ["root"]), p)
            add("unknown-child/" + "_".join(p or ["root"]), p + ["zzz"])
    add("help-unknown", ["help", "zzz"])
    add("help-unknown-two", ["help", "tab", "zzz"])
    # --- validation matrix on every leaf
    variants = {
        "none": [], "one": ["a"], "two": ["a", "b"], "three": ["a", "b", "c"], "five": ["a"] * 5,
        "bogusflag": ["--bogus"], "bogusshort": ["-x"], "novalue": ["--session"],
        "badbool": ["--json=maybe"], "badout": ["--output", "bogus"], "yamljson": ["--output=yaml", "--json"],
        "dashdash": ["--"], "dashdash-arg": ["--", "a"], "dashdash-flag": ["--", "--json"],
        "json-before-bogus": ["--json", "--bogus"], "negnum": ["-5"], "dash": ["-"],
    }
    for n in leaves:
        p = n["path"]
        for vname, extra in variants.items():
            if p[0] in no_exec and vname in ("none", "one", "two", "three", "five", "dashdash", "dashdash-arg", "dashdash-flag", "negnum", "dash", "badbool", "yamljson", "badout") and not (p[0] == "daemon" and len(p) > 1):
                if vname in ("one", "two", "three", "five", "dashdash-arg", "dash", "negnum") and p[0] in ("daemon", "mcp", "upgrade", "doctor", "watch"):
                    pass  # NoArgs rejects before running
                else:
                    continue
            add("val/%s/%s" % ("_".join(p), vname), p + extra)
    # --- flag syntax permutations (CLI-004)
    for argv_ in (["--json", "version"], ["version", "--json"], ["--output", "json", "version"], ["--output=yaml", "version"],
                  ["version", "--output", "yaml"], ["version", "--output=bogus"], ["--output=bogus", "version"],
                  ["--json=false", "version"], ["--json=true", "version"], ["--json", "--output", "yaml", "version"],
                  ["--output", "yaml", "--json", "version"], ["--output=json", "--output=yaml", "version"],
                  ["-v"], ["--version"], ["version", "-v"], ["--json", "config", "show", "--json"],
                  ["config", "--json", "show"], ["config", "show", "--json"], ["--output", "yaml", "config", "show"],
                  ["config", "show", "--log-level", "debug", "--json"], ["--json", "-v"], ["--json=0", "--version"]):
        add("flags/" + "~".join(argv_), argv_)
    for fmt in ([], ["--json"], ["--output", "yaml"]):
        add("flags-open/" + "~".join(fmt or ["text"]), ["open", "https://example.test/"] + fmt)
        add("flags-open-before/" + "~".join(fmt or ["text"]), fmt + ["open", "https://example.test/"])
    return cases


def recipes():
    """Cases that reach the daemon: (id, argv, responses[, extra])."""
    r = []
    nav = OK({"action": "open", "url": "https://example.test/", "http_status": 200})
    for name in ("open", "goto"):
        r.append(("nav/" + name, [name, "https://example.test/"], [nav]))
        r.append(("nav/%s-json" % name, [name, "https://example.test/", "--json"], [nav]))
        r.append(("nav/%s-yaml" % name, [name, "https://example.test/", "--output", "yaml"], [nav]))
    r.append(("nav/open-session", ["open", "https://example.test/", "--session", "s2"], [nav], {"session": "s2"}))
    r.append(("nav/open-badsession", ["open", "https://example.test/", "--session", "../x"], [nav]))
    r.append(("nav/open-nodaemon", ["open", "https://example.test/"], None))
    r.append(("nav/open-nodaemon-json", ["open", "https://example.test/", "--json"], None))
    r.append(("nav/open-nodaemon-yaml", ["--output", "yaml", "open", "https://example.test/"], None))
    r.append(("nav/open-nourl", ["open"], [nav]))
    r.append(("nav/open-nourl-json", ["open", "--json"], [nav]))
    fail = ERR("operation_failed", "navigation blocked", "check the policy")
    r.append(("nav/open-fail", ["open", "https://x.test/"], [fail]))
    r.append(("nav/open-fail-json", ["open", "https://x.test/", "--json"], [fail]))
    r.append(("nav/open-fail-yaml", ["open", "https://x.test/", "--output", "yaml"], [fail]))
    r.append(("nav/open-fail-invalid-code", ["open", "https://x.test/"], [ERR("zzz", "bogus code")]))
    r.append(("nav/open-fail-nocode", ["open", "https://x.test/"], [{"success": False}]))
    r.append(("nav/open-fail-stale", ["open", "https://x.test/", "--json"], [ERR("stale_ref", "gone", None, {"ref": "@e1"})]))
    for name in ("back", "forward", "reload"):
        r.append(("nav/" + name, [name], [OK({"action": name, "url": "https://example.test/x"})]))
        r.append(("nav/%s-arg" % name, [name, "https://example.test/"], [nav]))
    r.append(("nav/open-noaction", ["open", "https://example.test/"], [OK({"url": "https://example.test/", "title": "T"})]))
    # wait
    wr = OK({"awaited": "ms", "observed": {"url": "https://example.test/"}})
    for cid, argv in (("sel", ["wait", "#a"]), ("sel-hidden", ["wait", "#a", "--state", "hidden"]), ("ms", ["wait", "--ms", "250"]),
                      ("ms-neg", ["wait", "--ms", "-5"]), ("text", ["wait", "--text", "hello"]), ("url", ["wait", "--url", "**/done"]),
                      ("load", ["wait", "--load", "networkidle"]), ("load-bad", ["wait", "--load", "bogus"]),
                      ("none", ["wait"]), ("two", ["wait", "#a", "--ms", "5"]), ("ms-bad", ["wait", "--ms", "x"]),
                      ("empty-sel", ["wait", ""])):
        r.append(("wait/" + cid, argv, [wr]))
    # interactions
    ir = OK({"action": "click", "selector": "#a", "ok": True})
    for cid, argv in (("click", ["click", "#a"]), ("click-ref", ["click", "@e1"]), ("dblclick", ["dblclick", "#a"]),
                      ("fill", ["fill", "#a", "hello"]), ("fill-empty", ["fill", "#a", ""]), ("fill-novalue", ["fill", "#a"]),
                      ("type", ["type", "#a", "abc"]), ("press", ["press", "#a", "Enter"]), ("hover", ["hover", "#a"]),
                      ("focus", ["focus", "#a"]), ("select", ["select", "#a", "opt"]), ("check", ["check", "#a"]),
                      ("uncheck", ["uncheck", "#a"]), ("scroll", ["scroll", "#a", "300"]), ("scroll-zero", ["scroll", "#a", "0"]),
                      ("scroll-neg", ["scroll", "#a", "--", "-300"]), ("scroll-neg-bad", ["scroll", "#a", "-300"]),
                      ("scroll-bad", ["scroll", "#a", "abc"]), ("scroll-big", ["scroll", "#a", "99999999999999999999"]),
                      ("scrollintoview", ["scrollintoview", "#a"]), ("click-extra", ["click", "#a", "x"]),
                      ("click-none", ["click"]), ("fill-three", ["fill", "#a", "b", "c"])):
        r.append(("act/" + cid, argv, [ir]))
    r.append(("act/click-json", ["click", "#a", "--json"], [ir]))
    r.append(("act/click-fail", ["click", "#a"], [ERR("stale_ref", "stale ref @e1", "run snapshot")]))
    r.append(("act/click-fail-json", ["click", "#a", "--json"], [ERR("stale_ref", "stale ref @e1", "run snapshot")]))
    # get / is
    for kind, val in (("text", "hello <b>&"), ("html", "<p>x</p>"), ("value", "v"), ("title", "T"), ("url", "https://e.test/"),
                      ("count", 3), ("box", {"x": 1, "y": 2.5, "width": 100, "height": 20}), ("attr", None), ("styles", {"color": "red"})):
        resp = OK({"kind": kind, "value": val})
        argv = ["get", kind] + (["#a"] if kind not in ("title", "url") else [])
        if kind == "attr":
            argv += ["href"]; resp = OK({"kind": "attr", "value": "/x"})
        if kind == "styles":
            argv += ["color", "display"]
        r.append(("get/" + kind, argv, [resp]))
        r.append(("get/%s-json" % kind, argv + ["--json"], [resp]))
        r.append(("get/%s-maxtokens" % kind, argv + ["--max-tokens", "100"], [resp]))
    r.append(("get/title-sel", ["get", "title", "#a"], [OK({"kind": "title", "value": "T"})]))
    r.append(("get/url-two", ["get", "url", "a", "b"], [OK({"kind": "url", "value": "u"})]))
    r.append(("get/attr-one", ["get", "attr", "#a"], [OK({"kind": "attr", "value": "u"})]))
    r.append(("get/styles-none", ["get", "styles"], [OK({"kind": "styles", "value": {}})]))
    r.append(("get/text-none", ["get", "text"], [OK({"kind": "text", "value": "x"})]))
    r.append(("get/text-novalue", ["get", "text", "#a"], [OK({"kind": "text"})]))
    r.append(("get/maxtokens-zero", ["get", "text", "#a", "--max-tokens", "0"], [OK({"kind": "text", "value": "x"})]))
    r.append(("get/maxtokens-bad", ["get", "text", "#a", "--max-tokens", "x"], [OK({"kind": "text", "value": "x"})]))
    r.append(("get/maxtokens-range", ["get", "text", "#a", "--max-tokens", "99999999999999999999"], [OK({})]))
    r.append(("get/maxtokens-hex", ["get", "text", "#a", "--max-tokens", "0x10"], [OK({"kind": "text", "value": 1})]))
    r.append(("get/float-values", ["get", "count", "#a"], [OK({"kind": "count", "value": 1234567.0})]))
    r.append(("get/float-values2", ["get", "box", "#a"], [OK({"kind": "box", "value": [1, 2.5, "a", True, None, {"k": [1e21, 1e-7]}]})]))
    for kind in ("visible", "enabled", "checked"):
        r.append(("is/%s-true" % kind, ["is", kind, "#a"], [OK({"kind": kind, "value": True})]))
        r.append(("is/%s-false" % kind, ["is", kind, "#a"], [OK({"kind": kind, "value": False})]))
        r.append(("is/%s-false-json" % kind, ["is", kind, "#a", "--json"], [OK({"kind": kind, "value": False})]))
    r.append(("is/nonbool", ["is", "visible", "#a"], [OK({"kind": "visible", "value": "yes"})]))
    r.append(("is/none", ["is", "visible"], [OK({})]))
    # snapshot
    tree_payload = OK({"tree": "- document\n  - button \"Go\" [ref=@e1]\n", "refs": {"@e1": {"role": "button"}}, "snapshot_id": "s1", "hint": "use @e1"})
    r.append(("snap/plain", ["snapshot"], [tree_payload]))
    r.append(("snap/json", ["snapshot", "--json"], [tree_payload]))
    r.append(("snap/yaml", ["snapshot", "--output", "yaml"], [tree_payload]))
    r.append(("snap/flags", ["snapshot", "-i", "-c", "-d", "3", "-s", "#main", "-u", "--max-tokens", "500"], [tree_payload]))
    r.append(("snap/diff", ["snapshot", "--diff"], [OK({"added": [{"ref": "@e2"}, {"role": "link", "name": "Home"}, {}], "removed": ["x"], "changed": [], "snapshot_id": "s2"})]))
    r.append(("snap/since", ["snapshot", "--since", "s1"], [OK({"added": [], "removed": [{"refkey": "k"}], "hint": "h"})]))
    r.append(("snap/noscan", ["snapshot", "--no-injection-scan", "--injection-patterns", "p.txt"], [tree_payload]))
    r.append(("snap/depth-bad", ["snapshot", "-d", "x"], [tree_payload]))
    r.append(("snap/combined-short", ["snapshot", "-ic"], [tree_payload]))
    r.append(("snap/short-value-attached", ["snapshot", "-d3", "-s#a"], [tree_payload]))
    r.append(("snap/short-eq", ["snapshot", "-d=3"], [tree_payload]))
    r.append(("snap/boundaries-json", ["snapshot", "--content-boundaries", "--json"], [tree_payload]))
    r.append(("snap/fail", ["snapshot"], [ERR("session_not_found", "no session", "open a page first")]))
    r.append(("snap/truncated", ["snapshot"], [OK({"truncated": True, "head": "H", "foot": "F", "hint": "symbrowse cache get c1", "tokens_returned": 10, "tokens_total": 99})]))
    r.append(("snap/truncated-json", ["snapshot", "--json"], [OK({"truncated": True, "head": "H", "foot": "F", "hint": "x", "tokens_returned": 10, "tokens_total": 99})]))
    # screenshot / find / eval
    shot = OK({"path": "/tmp/x.png", "width": 800, "height": 600.0, "bytes": 1234})
    r.append(("shot/default", ["screenshot"], [shot]))
    r.append(("shot/path-flags", ["screenshot", "out.png", "--full", "--selector", "#a", "--format", "jpeg", "--quality", "80", "--screenshot-dir", "/tmp"], [shot]))
    r.append(("shot/json", ["screenshot", "--json"], [shot]))
    r.append(("shot/empty", ["screenshot"], [OK({})]))
    r.append(("shot/fail", ["screenshot"], [ERR("operation_failed", "capture failed")]))
    for cid, argv, data in (("click", ["find", "role", "button", "click", "--name", "Go", "--exact"], {"kind": "role", "query": "button", "action": "click", "ref": "e3"}),
                            ("text", ["find", "text", "Hello", "text"], {"action": "text", "value": "Hello world"}),
                            ("nth", ["find", "role", "link", "nth", "2"], {"action": "nth", "ref": "e9"}),
                            ("nth-bad", ["find", "role", "link", "nth", "x"], {}),
                            ("fill", ["find", "label", "Email", "fill", "me@x.test"], {"action": "fill", "ref": "e4"}),
                            ("short", ["find", "role", "button"], {}),
                            ("long", ["find", "a", "b", "c", "d", "e"], {})):
        r.append(("find/" + cid, argv, [OK(data)]))
    r.append(("find/json", ["find", "role", "button", "click", "--json"], [OK({"action": "click", "ref": "e3"})]))
    r.append(("find/fail", ["find", "role", "button", "click"], [ERR("unknown_ref", "no match")]))
    ev = OK({"value": {"a": [1, 2], "b": "<x>"}})
    r.append(("eval/arg", ["eval", "1+1"], [OK({"value": 2})]))
    r.append(("eval/arg-json", ["eval", "1+1", "--json"], [OK({"value": 2})]))
    r.append(("eval/object", ["eval", "x"], [ev]))
    r.append(("eval/undefined", ["eval", "x"], [OK({})]))
    r.append(("eval/null", ["eval", "x"], [OK({"value": None})]))
    r.append(("eval/exception", ["eval", "x"], [OK({"exception_text": "ReferenceError: x"})]))
    r.append(("eval/stdin", ["eval", "--stdin", "--json"], [OK({"value": 2})], {"stdin": "1+1"}))
    r.append(("eval/stdin-and-arg", ["eval", "ignored", "--stdin"], [OK({"value": 2})], {"stdin": "2+2"}))
    r.append(("eval/stdin-empty", ["eval", "--stdin"], [OK({"value": 2})], {"stdin": ""}))
    r.append(("eval/none", ["eval"], [OK({})]))
    r.append(("eval/none-json", ["eval", "--json"], [OK({})]))
    r.append(("eval/b64", ["eval", "--base64", "MSsx"], [OK({"value": 2})]))
    r.append(("eval/b64-short", ["eval", "-b", "MSsx"], [OK({"value": 2})]))
    r.append(("eval/b64-stdin", ["eval", "-b", "--stdin"], [OK({"value": 2})], {"stdin": "MSsx\n"}))
    r.append(("eval/b64-newline-inside", ["eval", "-b", "--stdin"], [OK({"value": 2})], {"stdin": "MS\nsx"}))
    r.append(("eval/b64-bad", ["eval", "-b", "!!!"], [OK({})]))
    r.append(("eval/b64-nopad", ["eval", "-b", "MSs"], [OK({})]))
    r.append(("eval/b64-bad2", ["eval", "-b", "MS=x"], [OK({})]))
    r.append(("eval/b64-trailing", ["eval", "-b", "MQ==x"], [OK({})]))
    r.append(("eval/b64-onechar", ["eval", "-b", "M"], [OK({})]))
    r.append(("eval/b64-pad-first", ["eval", "-b", "=MSsx"], [OK({})]))
    r.append(("eval/b64-pad-half", ["eval", "-b", "MQ=="], [OK({"value": 1})]))
    r.append(("eval/b64-pad-missing2nd", ["eval", "-b", "MQ="], [OK({})]))
    r.append(("eval/b64-url-alphabet", ["eval", "-b", "-_-_"], [OK({})]))
    r.append(("eval/b64-empty", ["eval", "-b", ""], [OK({"value": 1})]))
    r.append(("eval/base64-eq", ["eval", "--base64=false", "1+1"], [OK({"value": 2})]))
    r.append(("eval/two-args", ["eval", "a", "b"], [OK({"value": 2})]))
    r.append(("eval/unicode", ["eval", "'é\u2028<>&'"], [OK({"value": "é\u2028<>&\b\f\u0001"})]))
    r.append(("eval/unicode-json", ["eval", "x", "--json"], [OK({"value": "é\u2028<>&\b\f\u0001"})]))
    r.append(("eval/session-bad", ["eval", "1", "--session", "bad name"], [OK({})]))
    # console / errors
    entries = OK({"entries": [{"type": "log", "text": "hi"}, {"type": "error", "text": "boom", "stacktrace": ["a.js:1", {"f": 1}]}]})
    for grp in ("console", "errors"):
        r.append(("%s/list" % grp, [grp, "list"], [entries]))
        r.append(("%s/list-json" % grp, [grp, "list", "--json"], [entries]))
        r.append(("%s/list-empty" % grp, [grp, "list"], [OK({})]))
        r.append(("%s/clear" % grp, [grp, "clear"], [OK({})]))
        r.append(("%s/clear-json" % grp, [grp, "clear", "--json"], [OK({})]))
        r.append(("%s/clear-fail" % grp, [grp, "clear"], [ERR("operation_failed", "no")]))
        r.append(("%s/list-maxtokens" % grp, [grp, "list", "--max-tokens", "10"], [entries]))
        r.append(("%s/list-session" % grp, [grp, "--session", "s3", "list"], [entries], {"session": "s3"}))
    # upload / downloads
    r.append(("upload/ok", ["upload", "#f", "a.txt", "b.txt"], [OK({})]))
    r.append(("upload/json", ["upload", "#f", "a.txt", "--json"], [OK({"uploaded": 1})]))
    r.append(("upload/one", ["upload", "#f"], [OK({})]))
    r.append(("upload/fail", ["upload", "#f", "a"], [ERR("permission", "path denied", "use the upload dir")]))
    dl = OK({"downloads": [{"state": "completed", "filename": "a.zip", "url": "https://e.test/a.zip", "sha256": "abc"}, {"state": "in_progress", "filename": "b", "url": "u"}]})
    r.append(("downloads/list", ["downloads"], [dl]))
    r.append(("downloads/json", ["downloads", "--json"], [dl]))
    r.append(("downloads/dir", ["downloads", "--dir", "/tmp/d"], [OK({}), dl]))
    r.append(("downloads/dir-fail", ["downloads", "--dir", "/tmp/d"], [ERR("permission", "no")]))
    # network
    reqs = OK({"requests": [{"method": "GET", "url": "https://A.test/x", "type": "xhr", "status": 200}, {"method": "POST", "url": "https://b.test/y?<a>", "type": "document", "status": 404}]})
    r.append(("net/requests", ["network", "requests"], [reqs]))
    r.append(("net/requests-json", ["network", "requests", "--json"], [reqs]))
    r.append(("net/requests-yaml", ["network", "requests", "--output", "yaml"], [reqs]))
    r.append(("net/requests-filter", ["network", "requests", "--filter", "a.TEST", "--type", "XHR", "--method", "get", "--status", "200"], [reqs]))
    r.append(("net/requests-none", ["network", "requests", "--filter", "zzz"], [reqs]))
    r.append(("net/requests-none-json", ["network", "requests", "--filter", "zzz", "--json"], [reqs]))
    r.append(("net/requests-empty", ["network", "requests", "--json"], [OK({})]))
    r.append(("net/request", ["network", "request", "r1"], [OK({"id": "r1", "url": "u", "headers": {"a": "b"}})]))
    r.append(("net/request-json", ["network", "request", "r1", "--json"], [OK({"id": "r1"})]))
    r.append(("net/route-mock", ["network", "route", "**/api", "--body", '{"a": 1}', "--status", "201"], [OK({})]))
    r.append(("net/route-badbody", ["network", "route", "**/api", "--body", "{x"], [OK({})]))
    r.append(("net/route-abort", ["network", "route", "**/api", "--abort"], [OK({})]))
    r.append(("net/unroute", ["network", "unroute"], [OK({})]))
    r.append(("net/unroute-pattern", ["network", "unroute", "**/api"], [OK({})]))
    r.append(("net/har-start", ["network", "har", "start", "--content", "all"], [OK({})]))
    r.append(("net/har-stop", ["network", "har", "stop"], [OK({"har": {"log": {"entries": []}}})]))
    r.append(("net/har-stop-file", ["network", "har", "stop", "--output", "out.har"], [OK({"har": {"log": {"entries": [1]}}})]))
    r.append(("net/har-stop-json", ["network", "har", "stop", "--json"], [OK({"har": {"log": {}}})]))
    r.append(("net/har-bad", ["network", "har", "pause"], [OK({})]))
    # sessions / daemon lifecycle
    sess = OK({"sessions": [{"name": "default"}]})
    r.append(("session/list", ["session", "list"], [sess]))
    r.append(("session/list-nodaemon", ["session", "list"], None))
    r.append(("session/info", ["session", "info", "--json"], [OK({"session": "default", "engine": "chrome"})]))
    r.append(("daemon/status", ["daemon", "status"], [OK({"running": True, "pid": 12})]))
    r.append(("daemon/stop", ["daemon", "stop", "--session", "s9"], [OK({"stopping": True})], {"session": "s9"}))
    r.append(("daemon/status-nodaemon", ["daemon", "status", "--json"], None))
    # tabs / frames / dialogs
    tabs = OK({"active": "t1", "tabs": [{"id": "t1", "label": "main", "url": "https://e.test/", "active": True}, {"id": "t2", "label": "t2", "url": "u2"}, {}]})
    r.append(("tab/list", ["tab", "list"], [tabs]))
    r.append(("tab/list-json", ["tab", "list", "--json"], [tabs]))
    r.append(("tab/new", ["tab", "new", "https://e.test/", "--label", "x"], [OK({"id": "t3"})]))
    r.append(("tab/new-bare", ["tab", "new"], [OK({"id": "t3"})]))
    r.append(("tab/switch", ["tab", "switch", "t1"], [OK({})]))
    r.append(("tab/close", ["tab", "close"], [OK({})]))
    r.append(("tab/close-arg", ["tab", "close", "x"], [OK({})]))
    r.append(("tab/window", ["tab", "window", "window"], [OK({"id": "w1"})]))
    r.append(("tab/window-new", ["tab", "window", "window", "new"], [OK({})]))
    r.append(("tab/fail", ["tab", "list"], [ERR("session_inactive", "inactive")]))
    r.append(("frame/tree", ["frame", "tree"], [OK({"frames": []})]))
    r.append(("frame/main", ["frame", "main"], [OK({})]))
    r.append(("frame/select", ["frame", "select", "f1"], [OK({})]))
    r.append(("dialog/status", ["dialog", "status"], [OK({"pending": False})]))
    r.append(("dialog/accept", ["dialog", "accept", "hi"], [OK({})]))
    r.append(("dialog/accept-bare", ["dialog", "accept"], [OK({})]))
    r.append(("dialog/dismiss", ["dialog", "dismiss"], [OK({})]))
    r.append(("dialog/auto", ["dialog", "auto", "accept"], [OK({})]))
    # state-ish
    r.append(("auth/login", ["auth", "login", "entry", "--url", "https://e.test/login"], [OK({})]))
    r.append(("auth/login-bare", ["auth", "login", "entry"], [OK({})]))
    r.append(("auth/login-json", ["auth", "login", "entry", "--json"], [OK({"filled": 2})]))
    r.append(("auth/login-fail", ["auth", "login", "entry"], [ERR("auth", "vault locked")]))
    r.append(("handoff/ok", ["handoff", "--reason", "2FA"], [OK({"status": "completed", "prompt_id": "p1"})]))
    r.append(("handoff/timeout", ["handoff", "--reason", "x", "--timeout", "1m"], [OK({"status": "completed"})]))
    r.append(("handoff/noreason", ["handoff"], [OK({})]))
    r.append(("handoff/json", ["handoff", "--reason", "x", "--json"], [OK({"status": "completed"})]))
    r.append(("handoff/fail", ["handoff", "--reason", "x"], [ERR("handoff_timeout", "timed out")]))
    jr = OK({"entries": [{"timestamp": "2026-01-01T00:00:00Z", "command": "open", "risk_class": "navigate", "decider": "builtin", "result": "allow"}, {}]})
    r.append(("journal/tail", ["journal", "tail", "--lines", "3"], [jr]))
    r.append(("journal/tail-default", ["journal", "tail"], [jr]))
    r.append(("journal/show", ["journal", "show"], [jr]))
    r.append(("journal/show-json", ["journal", "show", "--json"], [jr]))
    r.append(("oob/none", ["oob", "status"], [OK({"active": False})]))
    r.append(("oob/active", ["oob", "status"], [OK({"active": True, "prompt": {"id": "p1", "kind": "approval", "reason": "r"}})]))
    r.append(("oob/json", ["oob", "status", "--json"], [OK({"active": False})]))
    r.append(("policy/explain", ["policy", "explain", "click", "--url", "https://e.test/", "--mode", "mcp"], [OK({"explanation": "allowed by rule 1"})]))
    r.append(("policy/explain-raw", ["policy", "explain", "click"], [OK({"decision": "allow"})]))
    r.append(("policy/explain-json", ["policy", "explain", "click", "--json"], [OK({"explanation": "x"})]))
    for name in ("save", "load", "show", "clear"):
        r.append(("state/" + name, ["state", name, "login"], [OK({name + "d": "login", "metadata": {"name": "login", "schema_version": 2, "saved_at": "t", "origins": [{"origin": "https://e.test", "cookie_count": 2, "local_storage_keys": 1, "session_storage_keys": 0}]}})]))
        r.append(("state/%s-json" % name, ["state", name, "login", "--json"], [OK({"x": 1})]))
        r.append(("state/%s-fail" % name, ["state", name, "login"], [ERR("not_found", "state missing")]))
    r.append(("state/list", ["state", "list"], [OK({"schema_version": 1, "states": ["a", "b"]})]))
    r.append(("state/list-empty", ["state", "list"], [OK({"states": []})]))
    r.append(("state/list-json", ["state", "list", "--json"], [OK({"schema_version": 1, "states": ["a"]})]))
    r.append(("state/clean", ["state", "clean"], [OK({"removed": ["old"]})]))
    r.append(("state/clean-json", ["state", "clean", "--json"], [OK({"removed": ["old"]})]))
    r.append(("state/clean-json-none", ["state", "clean", "--json"], [OK({})]))
    r.append(("state/clean-yaml", ["state", "clean", "--output", "yaml"], [OK({"removed": ["old", "x y"]})]))
    r.append(("state/clean-days", ["state", "clean", "--older-than", "7"], [OK({"removed": []})]))
    r.append(("state/clean-days-bad", ["state", "clean", "--older-than", "x"], [OK({})]))
    r.append(("state/clean-days-neg", ["state", "clean", "--older-than", "-1"], [OK({})]))
    ck = OK({"origin": "https://e.test", "cookies": [
        {"name": "sid", "value": "abcdefghijkl", "domain": ".e.test", "path": "/", "expires": -1, "size": 15, "http_only": True, "secure": True, "session": True, "same_site": "Lax"},
        {"name": "short", "value": "abc", "domain": "e.test", "path": "/x", "expires": 1.8e9, "size": 8}, {"name": "empty", "value": ""}]})
    r.append(("cookies/list", ["cookies", "list"], [ck]))
    r.append(("cookies/list-json", ["cookies", "list", "--json"], [ck]))
    r.append(("cookies/list-yaml", ["cookies", "list", "--output", "yaml"], [ck]))
    r.append(("cookies/list-reveal", ["cookies", "list", "--reveal", "sid, short"], [ck]))
    r.append(("cookies/list-reveal-all", ["cookies", "--reveal", "all", "list", "--json"], [ck]))
    r.append(("cookies/list-empty-json", ["cookies", "list", "--json"], [OK({})]))
    r.append(("cookies/set", ["cookies", "set", "a", "b", "--domain", "e.test", "--path", "/p", "--url", "https://e.test/", "--secure", "--http-only"], [OK({})]))
    r.append(("cookies/set-json", ["cookies", "set", "a", "b", "--json"], [OK({"set": True})]))
    r.append(("cookies/set-one", ["cookies", "set", "a"], [OK({})]))
    r.append(("cookies/set-curl", ["cookies", "set", "--curl", "jar.txt"], [OK({}), ERR("operation_failed", "no"), OK({})],
              {"files": {"jar.txt": "# c\n.e.test\tTRUE\t/\tTRUE\t1800000000\tsid\tval\n\nshort\n.f.test\tFALSE\t/\tFALSE\t0\tk\tv\n"}}))
    r.append(("cookies/set-curl-missing", ["cookies", "set", "--curl", "nope.txt"], [OK({})]))
    r.append(("cookies/set-curl-args", ["cookies", "set", "--curl", "jar.txt", "x"], [OK({})]))
    r.append(("cookies/clear", ["cookies", "clear", "a", "--url", "https://e.test/"], [OK({})]))
    r.append(("cookies/clear-json", ["cookies", "clear", "a", "--json"], [OK({"cleared": 1})]))
    st = OK({"origin": "https://e.test", "kind": "local", "items": {"b": "2", "a": "1"}})
    r.append(("storage/get-all", ["storage", "get", "local"], [st]))
    r.append(("storage/get-key", ["storage", "get", "local", "a"], [st]))
    r.append(("storage/get-missing", ["storage", "get", "local", "zz"], [st]))
    r.append(("storage/get-json", ["storage", "get", "session", "--json"], [st]))
    r.append(("storage/get-empty", ["storage", "get", "local"], [OK({})]))
    r.append(("storage/set", ["storage", "set", "local", "k", "v"], [OK({})]))
    r.append(("storage/clear", ["storage", "clear", "local"], [OK({})]))
    # set
    ok = OK({})
    r.append(("set/viewport", ["set", "viewport", "800", "600"], [ok]))
    r.append(("set/viewport-scale", ["set", "viewport", "800", "600", "2.5"], [ok]))
    r.append(("set/viewport-json", ["set", "viewport", "800", "600", "--json"], [ok]))
    r.append(("set/viewport-badw", ["set", "viewport", "x", "600"], [ok]))
    r.append(("set/viewport-badh", ["set", "viewport", "8", "y"], [ok]))
    r.append(("set/viewport-bads", ["set", "viewport", "8", "9", "z"], [ok]))
    r.append(("set/viewport-hex", ["set", "viewport", "0x10", "9"], [ok]))
    r.append(("set/viewport-range", ["set", "viewport", "99999999999999999999", "9"], [ok]))
    r.append(("set/viewport-one", ["set", "viewport", "8"], [ok]))
    r.append(("set/viewport-fail", ["set", "viewport", "8", "9"], [ERR("validation", "bad size")]))
    r.append(("set/geo", ["set", "geo", "52.5", "13.4"], [ok]))
    r.append(("set/geo-bad", ["set", "geo", "x", "13.4"], [ok]))
    r.append(("set/geo-bad2", ["set", "geo", "1", "y"], [ok]))
    r.append(("set/geo-inf", ["set", "geo", "1", "1e999"], [ok]))
    r.append(("set/offline", ["set", "offline"], [ok]))
    r.append(("set/offline-off", ["set", "offline", "OFF"], [ok]))
    r.append(("set/offline-bad", ["set", "offline", "maybe"], [ok]))
    r.append(("set/headers", ["set", "headers", '{"X-A": "b"}'], [ok]))
    r.append(("set/headers-empty", ["set", "headers", "{}"], [ok]))
    r.append(("set/headers-null", ["set", "headers", "null"], [ok]))
    r.append(("set/headers-array", ["set", "headers", "[1]"], [ok]))
    r.append(("set/headers-badvalue", ["set", "headers", '{"a": 1}'], [ok]))
    r.append(("set/headers-syntax", ["set", "headers", "{x"], [ok]))
    r.append(("set/headers-eof", ["set", "headers", "{"], [ok]))
    r.append(("set/headers-str", ["set", "headers", '"x"'], [ok]))
    r.append(("set/media", ["set", "media", "Dark"], [ok]))
    r.append(("set/media-bad", ["set", "media", "blue"], [ok]))
    r.append(("set/user-agent", ["set", "user-agent", "UA/1.0"], [ok]))
    r.append(("set/device", ["set", "device", "iPhone 13"], [ok]))
    r.append(("set/device-none", ["set", "device"], [ok]))
    r.append(("set/device-list-arg", ["set", "device", "--list", "x"], [ok]))
    # a11y / diff / flow record
    r.append(("a11y/plain", ["a11y", "--tags", "wcag2a, wcag2aa,", "--selector", "main"], [OK({"violations": []})]))
    r.append(("a11y/url", ["a11y", "https://e.test/"], [OK({}), OK({"violations": []})]))
    r.append(("a11y/url-fail", ["a11y", "https://e.test/"], [ERR("operation_failed", "nav failed")]))
    r.append(("a11y/json", ["a11y", "--json"], [OK({"violations": []})]))
    r.append(("diff/snapshot", ["diff", "snapshot"], [OK({"added": [], "removed": []})]))
    r.append(("diff/snapshot-missing-baseline", ["diff", "snapshot", "--baseline", "nope.json"], [OK({})]))
    r.append(("diff/screenshot-nobaseline", ["diff", "screenshot"], [OK({})]))
    r.append(("diff/screenshot-missing", ["diff", "screenshot", "--baseline", "nope.png"], [OK({})]))
    r.append(("flow/record-start", ["flow", "record", "start"], [OK({"recording": True})]))
    r.append(("flow/record-status", ["flow", "record", "status", "--json"], [OK({"recording": False})]))
    r.append(("flow/record-fail", ["flow", "record", "stop"], [ERR("operation_failed", "nothing recorded")]))
    return r


def load_tree(path):
    return json.load(open(path))


def all_cases(tree):
    cases = build_cases(tree)
    for entry in recipes():
        cid, argv, responses = entry[0], entry[1], entry[2]
        extra = entry[3] if len(entry) > 3 else {}
        case = dict(id=cid, argv=argv, daemon=responses)
        case.update(extra)
        cases.append(case)
    for i, c in enumerate(cases):
        c.setdefault("daemon", None)
    ids = [c["id"] for c in cases]
    assert len(ids) == len(set(ids)), "duplicate case ids"
    return cases


def main():
    mode, binary = sys.argv[1], sys.argv[2]
    here = os.path.dirname(os.path.abspath(__file__))
    tree = load_tree(os.path.join(here, "tree.json"))
    os.makedirs(SCRATCH, exist_ok=True)
    if mode == "gen":
        sha, out = sys.argv[3], sys.argv[4]
        cases = all_cases(tree)
        for n, case in enumerate(cases):
            case["expect"] = run_case(binary, case, n)
            if case["expect"]["exit"] == -1:
                print("TIMEOUT", case["id"])
        doc = {
            "schema": 1,
            "provenance": {
                "go_source_sha": sha,
                "go_binary_sha256": hashlib.sha256(open(binary, "rb").read()).hexdigest(),
                "go_version": subprocess.run(["go", "version"], capture_output=True, text=True).stdout.strip(),
                "platform": platform.system().lower() + "/" + platform.machine(),
                "generator": "browse/testdata/port/cli/gen_cases.py",
                "normalisation": "sandbox root -> <ROOT>, session socket -> <SOCKET>; request ids are recorded as sent",
            },
            "cases": cases,
        }
        json.dump(doc, open(out, "w"), indent=0, ensure_ascii=False)
        print(len(cases), "cases")
    else:
        doc = json.load(open(sys.argv[3]))
        prefix = sys.argv[4] if len(sys.argv) > 4 else ""
        bad = 0
        total = 0
        for n, case in enumerate(doc["cases"]):
            if not case["id"].startswith(prefix):
                continue
            total += 1
            got = run_case(binary, case, n)
            want = case["expect"]
            diffs = [k for k in ("stdout", "stderr", "exit", "frames") if got[k] != want[k]]
            if diffs:
                bad += 1
                print("DIFF", case["id"], case["argv"], diffs)
                if os.environ.get("VERBOSE"):
                    for k in diffs:
                        print("  want %s: %r" % (k, want[k]))
                        print("  got  %s: %r" % (k, got[k]))
        print("%d/%d cases differ" % (bad, total))
        sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
