#!/usr/bin/env python3
"""Supplemental actual frozen-Go/native honest transport comparisons for #773."""
from __future__ import annotations
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.handle_request()

    do_POST = do_PUT = do_PATCH = do_DELETE = do_HEAD = do_GET

    def handle_request(self):
        parsed = urlsplit(self.path)
        params = parse_qs(parsed.query)
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        status = 200
        headers = {}
        content = b"12345678"
        if parsed.path == "/echo":
            content = json.dumps(dict(method=self.command, body=body.decode(),
                headers={name.lower(): self.headers.get_all(name) for name in
                    ["User-Agent", "Accept", "Accept-Language", "Accept-Encoding", "X-Probe"]
                    if self.headers.get_all(name)}), sort_keys=True, separators=(",", ":")).encode()
        elif parsed.path == "/redirect":
            status = int(params["status"][0])
            headers["Location"] = "/echo"
        elif parsed.path == "/hop":
            left = int(params["left"][0])
            if left:
                status = 302
                headers["Location"] = "/hop?left=" + str(left - 1)
        elif parsed.path == "/status":
            status = int(params["code"][0])
        elif parsed.path == "/set-cookie":
            headers["Set-Cookie"] = "private_probe=synthetic; Path=/"
        elif parsed.path == "/cookie":
            content = self.headers.get("Cookie", "").encode()
        elif parsed.path in ("/gzip-multi", "/gzip-corrupt-tail", "/gzip-garbage-tail"):
            content = gzip.compress(b"first", mtime=0) + gzip.compress(b"second", mtime=0)
            if parsed.path == "/gzip-corrupt-tail":
                content = content[:-8] + bytes([content[-8] ^ 1]) + content[-7:]
            if parsed.path == "/gzip-garbage-tail": content += b"invalid"
            headers["Content-Encoding"] = "gzip"
        elif parsed.path == "/gzip":
            content = gzip.compress(content, mtime=0)
            headers["Content-Encoding"] = "gzip"
        elif parsed.path == "/slow-headers":
            time.sleep(0.08)
        self.send_response_only(status)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        for key, value in headers.items():
            self.send_header(key, value)
        self.send_header("Content-Length", str(0 if status == 204 else len(content)))
        self.end_headers()
        try:
            if self.command == "HEAD" or status == 204:
                return
            if parsed.path == "/slow-body":
                self.wfile.write(content[:1])
                self.wfile.flush()
                time.sleep(0.08)
                content = content[1:]
            self.wfile.write(content)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, *_):
        pass


def cases(base):
    result = []
    def add(name, path="/echo", **options):
        result.append(dict(id=name, url=base + path, method="GET", **options))
    add("default-headers")
    for name, value in [("User-Agent", "custom-header-agent"), ("Accept", "application/custom"),
                        ("Accept-Language", "de"), ("Accept-Encoding", "identity"), ("X-Probe", "custom")]:
        add("override-" + name.lower(), headers={name: value})
    add("range-disables-implicit-gzip", headers={"Range": "bytes=0-7"})
    add("empty-accept-encoding", headers={"Accept-Encoding": ""})
    add("explicit-gzip-response", "/gzip", headers={"Accept-Encoding": "gzip"})
    add("range-gzip-response", "/gzip", headers={"Range": "bytes=0-7"})
    add("explicit-user-agent-wins", headers={"User-Agent": "header-agent"}, user_agent="explicit-agent")
    add("empty-user-agent-default", user_agent="")
    add("empty-user-agent-header", headers={"User-Agent": ""})
    add("empty-method-default")
    result[-1]["method"] = ""
    add("zero-timeout-default", timeout_ms=0)
    add("zero-size-default", "/body", max_body=0)
    for method in ["POST", "PUT", "PATCH", "DELETE", "HEAD"]:
        add("method-" + method.lower(), body="synthetic-body")
        result[-1]["method"] = method
    for code in [301, 302, 303, 307, 308]:
        add("redirect-post-" + str(code), "/redirect?status=" + str(code), body="post-body")
        result[-1]["method"] = "POST"
    for count in [8, 9, 10, 11]:
        add("redirect-hops-" + str(count), "/hop?left=" + str(count))
    add("head-gzip", "/gzip")
    result[-1]["method"] = "HEAD"
    for limit in [7, 8, 9]:
        add("body-size-" + str(limit), "/body", max_body=limit)
        add("gzip-size-" + str(limit), "/gzip", max_body=limit)
    for path in ["slow-headers", "slow-body"]:
        add("timeout-" + path, "/" + path, timeout_ms=20)
    for code in [200, 204, 400, 408, 429, 500, 503]:
        add("status-" + str(code), "/status?code=" + str(code))
    add("ephemeral-cookie-set", "/set-cookie")
    add("ephemeral-cookie-not-carried", "/cookie")
    add("gzip-multi-success", "/gzip-multi")
    for limit in [7, 10, 11, 12]:
        add("gzip-multi-size-" + str(limit), "/gzip-multi", max_body=limit)
    add("gzip-multi-corrupt-tail", "/gzip-corrupt-tail")
    add("gzip-multi-garbage-tail", "/gzip-garbage-tail")
    add("http-proxy-explicit-private", proxy=base)
    result[-1]["url"] = "http://93.184.216.34/echo"
    add("http-proxy-private-blocked", proxy=base, allow_private=False)
    result[-1]["url"] = "http://93.184.216.34/echo"
    return result


def comparable(record):
    value = {key: value for key, value in record.items() if key != "detail"}
    if value.get("route"):
        value["route"] = value["route"].rstrip("/")
    return value


def execute(binary, selected, root, proxy_env=None):
    root.mkdir()
    env = {key: os.environ[key] for key in ("SystemRoot", "WINDIR", "TMP", "TEMP") if key in os.environ}
    env.update(HOME=str(root), USERPROFILE=str(root), XDG_CONFIG_HOME=str(root / "config"),
        XDG_CACHE_HOME=str(root / "cache"), XDG_DATA_HOME=str(root / "data"), PATH="",
        SYMBROWSE_GO_BINARY=str(root / "absent-go"))
    env.update(proxy_env or {})
    command = [str(binary.resolve())]
    if binary.suffix == ".py": command.insert(0, sys.executable)
    process = subprocess.run(command, input=json.dumps(selected).encode(),
        capture_output=True, cwd=root, env=env, timeout=15)
    if process.returncode or process.stderr:
        raise AssertionError((process.returncode, process.stderr.decode(errors="replace")))
    records = [json.loads(line) for line in process.stdout.splitlines()]
    expected = [case["id"] for case in selected]
    assert [record["id"] for record in records] == expected, "missing/duplicate/reordered process case"
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--go-source", type=Path, required=True,
                        help="owned complete frozen Browse module used to build the Go probe")
    args = parser.parse_args()
    repo = Path(__file__).resolve().parents[3]
    ref = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
    names = subprocess.check_output(["git", "ls-tree", "-r", "--name-only", ref, "browse"], cwd=repo, text=True).splitlines()
    original_sources = {}
    for name in names:
        if not (name.endswith(".go") or name in ("browse/go.mod", "browse/go.sum")):
            continue
        expected = subprocess.check_output(["git", "show", ref + ":" + name], cwd=repo)
        actual = (args.go_source / Path(name).relative_to("browse")).read_bytes()
        assert actual == expected, "frozen Go source changed: " + name
        original_sources[name] = hashlib.sha256(actual).hexdigest()
    assert original_sources, "no verified frozen Go input"
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        selected = cases(f"http://127.0.0.1:{server.server_port}")
        assert len(selected) == 57 and len({case["id"] for case in selected}) == 57
        with tempfile.TemporaryDirectory(prefix="fetch773-process-") as raw:
            go = execute(args.go, selected, Path(raw) / "go")
            rust = execute(args.rust, selected, Path(raw) / "rust")
        results = [dict(id=case["id"], request=case, go=left, rust=right,
            matched=comparable(left) == comparable(right)) for case, left, right in zip(selected, go, rust)]
        proxy_base = f"http://127.0.0.1:{server.server_port}"
        route_groups = [
            ("http-upper", {"HTTP_PROXY": proxy_base}, ["http://example.com", "http://localhost", "http://127.0.0.2", "http://[::1]", "http://[::ffff:127.0.0.1]"]),
            ("http-lower", {"http_proxy": proxy_base}, ["http://example.com"]),
            ("https-upper", {"HTTPS_PROXY": proxy_base}, ["https://example.com", "http://example.com"]),
            ("https-lower", {"https_proxy": proxy_base}, ["https://example.com"]),
            ("all-ignored", {"ALL_PROXY": proxy_base}, ["http://example.com", "https://example.com"]),
            ("bare-proxy", {"HTTP_PROXY": "127.0.0.1:12345"}, ["http://example.com"]),
            ("invalid-proxy", {"HTTP_PROXY": "http://%zz"}, ["http://example.com"]),
            ("cgi", {"HTTP_PROXY": proxy_base, "REQUEST_METHOD": "GET", "NO_PROXY": "*"}, ["http://example.com", "http://localhost"]),
        ]
        bypasses = ["*", "example.com", ".example.com", "*.example.com", "example.com:80", "example.com:81", " example.com , other.test ", "EXAMPLE.COM", "10.0.0.0/8", "93.184.216.34", "93.184.216.34:80", "93.184.216.34:81", "93.184.216.0/24", "[2001:4860::1]:80", "2001:4860::/32"]
        urls = ["http://example.com", "http://sub.example.com", "http://notexample.com", "http://93.184.216.34", "http://[2001:4860::1]"]
        route_groups += [("no-proxy-" + str(index), {"HTTP_PROXY": proxy_base, "NO_PROXY": entry}, urls) for index, entry in enumerate(bypasses)]
        route_groups += [("no-proxy-lower", {"HTTP_PROXY": proxy_base, "no_proxy": "example.com"}, ["http://example.com", "http://sub.example.com"])]
        route_groups += [("unicode-domain", {"HTTP_PROXY": proxy_base, "NO_PROXY": "bücher.test"}, ["http://bücher.test", "http://sub.bücher.test", "http://xn--bcher-kva.test"])]
        review_entries = ["example.com:080", "example.com:00080", "example.com:", "example.com:80", "[2001:4860::1]", "[2001:4860::1]:080", "2001:4860::/32", "::ffff:93.184.216.0/120", ".com", "*.example.com", "127.1", "example.com.", ".example.com.", "93.184.216.34:080"]
        review_urls = ["http://example.com", "http://example.com.", "http://sub.example.com", "http://93.184.216.34", "http://[2001:4860::1]"]
        route_groups += [("review-no-proxy-" + str(index), {"HTTP_PROXY": proxy_base, "NO_PROXY": entry}, review_urls) for index, entry in enumerate(review_entries)]
        if os.name != "nt":
            route_groups += [("upper-precedence", {"HTTP_PROXY": proxy_base, "http_proxy": "http://127.0.0.1:1", "NO_PROXY": "example.com", "no_proxy": "*"}, ["http://other.test", "http://example.com"])]
        with tempfile.TemporaryDirectory(prefix="fetch773-proxy-") as raw:
            for index, (name, env, urls) in enumerate(route_groups):
                group = [dict(id=f"env-route-{name}-{n}", url=url, route_only=True) for n, url in enumerate(urls)]
                left = execute(args.go, group, Path(raw) / f"go-{index}", env)
                right = execute(args.rust, group, Path(raw) / f"rust-{index}", env)
                results += [dict(id=case["id"], kind="route-only-no-network", request=case, environment=env, go=a, rust=b, matched=comparable(a) == comparable(b)) for case, a, b in zip(group, left, right)]
            for index, (name, env, url, allow_private) in enumerate([
                ("implicit-private-denied", {"HTTP_PROXY": proxy_base}, "http://93.184.216.34/echo", False),
                ("implicit-private-allowed", {"HTTP_PROXY": proxy_base}, "http://93.184.216.34/echo", True),
                ("loopback-bypass", {"HTTP_PROXY": "http://127.0.0.1:1"}, proxy_base + "/echo", True),
                ("all-proxy-ignored", {"ALL_PROXY": "http://127.0.0.1:1"}, proxy_base + "/echo", True),
                ("explicit-overrides-env", {"HTTP_PROXY": "http://127.0.0.1:1"}, "http://93.184.216.34/echo", True),
                ("cgi-denied", {"HTTP_PROXY": proxy_base, "REQUEST_METHOD": "GET"}, proxy_base + "/echo", True),
                ("https-proxy-private-denied", {"HTTPS_PROXY": proxy_base}, "https://93.184.216.34/echo", False),
            ]):
                case = dict(id="env-http-" + name, url=url, method="GET", allow_private=allow_private)
                if name == "explicit-overrides-env": case["proxy"] = proxy_base
                left = execute(args.go, [case], Path(raw) / f"go-http-{index}", env)[0]
                right = execute(args.rust, [case], Path(raw) / f"rust-http-{index}", env)[0]
                results.append(dict(id=case["id"], kind="actual-http-peer", request=case, environment=env, go=left, rust=right, matched=comparable(left) == comparable(right)))
        report = dict(candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
            candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)),
            oracle_ref="dcddcef0df5789123c7c9a7ebe6e01f10e941f2c", total=len(results),
            actual_http_cases=sum(record.get("kind") != "route-only-no-network" for record in results),
            route_only_cases=sum(record.get("kind") == "route-only-no-network" for record in results),
            platform=os.name,
            comparison_policy="success: status/url/body bytes/response headers/protocol; errors: typed class, literal details retained; route-only cases open no socket",
            sdk_proxy_source="Go 1.26.7 vendor/golang.org/x/net/http/httpproxy/proxy.go; actual net/http.ProxyFromEnvironment process calls",
            matched=sum(record["matched"] for record in results), results=results,
            go_source_sha256=original_sources,
            go_sdk=subprocess.check_output(["go", "version"], text=True).strip(),
            go_sdk_proxy_sha256=hashlib.sha256((Path(subprocess.check_output(["go", "env", "GOROOT"], text=True).strip()) / "src/vendor/golang.org/x/net/http/httpproxy/proxy.go").read_bytes()).hexdigest(),
            rust_sdk=subprocess.check_output(["rustc", "--version"], text=True).strip(),
            candidate_source_sha256={str(path.relative_to(repo)): hashlib.sha256(path.read_bytes()).hexdigest()
                for path in sorted((repo / "browse/crates/symbrowse-fetch").rglob("*")) if path.is_file() and path.suffix in (".rs", ".toml")},
            supplemental_go_sha256=hashlib.sha256((repo / "browse/port/harness/fetch_control_773.go.txt").read_bytes()).hexdigest(),
            binaries_sha256={label: hashlib.sha256(binary.read_bytes()).hexdigest() for label, binary in
                [("go", args.go), ("rust", args.rust)]})
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_bytes((json.dumps(report, indent=2) + "\n").encode())
        failures = [record["id"] for record in results if not record["matched"]]
        print(f"honest fetch process comparisons: {report['matched']}/{len(results)}; failures: {failures}")
        return bool(failures)
    finally:
        server.shutdown()
        thread.join(timeout=3)
        server.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
