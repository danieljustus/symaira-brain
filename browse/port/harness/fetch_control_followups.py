"""Permanent actual-process regressions from the immutable a073 review."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import tempfile
import threading


def route_groups(proxy):
    pairs = [
        ("93.184.216.0/+24", "http://93.184.216.34"),
        ("93.184.216.0/024", "http://93.184.216.34"),
        ("93.184.216.0/24", "http://93.184.216.34"),
        ("2001:4860::/+32", "http://[2001:4860::1]"),
        ("::ffff:93.184.216.0/+120", "http://93.184.216.34"),
        ("example.com:080", "http://example.com:080"),
        ("example.com:80", "http://example.com:080"),
        ("example.com:80", "http://example.com:80"),
        ("93.184.216.34:080", "http://93.184.216.34:080"),
        ("[2001:4860::1]:080", "http://[2001:4860::1]:080"),
    ]
    return [(f"review-a073-{i}", {"HTTP_PROXY": proxy, "NO_PROXY": entry}, [url])
            for i, (entry, url) in enumerate(pairs)]


class ProxyHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.server.observations.append(dict(path=self.path, host=self.headers.get("Host"),
                                            cookie=self.headers.get("Cookie", "")))
        content = self.server.label.encode()
        self.send_response_only(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def log_message(self, *_):
        pass


def cache_collision(go, rust, execute, comparable):
    servers = [ThreadingHTTPServer(("127.0.0.1", 0), ProxyHandler) for _ in range(2)]
    threads = []
    for server, label in zip(servers, ["proxy-first", "proxy-second"]):
        server.label, server.observations = label, []
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        threads.append(thread)
    try:
        p, q = [f"http://127.0.0.1:{server.server_port}" for server in servers]
        selected = [dict(id="cache-first", url="http://93.184.216.34/fixture", session="A",
                         proxy=p + "/x;proxy=" + q + "/y"),
                    dict(id="cache-second", url="http://93.184.216.34/fixture",
                         session="A;proxy=" + p + "/x", proxy=q + "/y")]
        with tempfile.TemporaryDirectory(prefix="fetch773-cache-") as raw:
            left = execute(go, selected, Path(raw) / "go")
            go_requests = [list(server.observations) for server in servers]
            for server in servers:
                server.observations.clear()
            right = execute(rust, selected, Path(raw) / "rust")
            rust_requests = [list(server.observations) for server in servers]
        assert [record["body_hex"] for record in left] == [value.encode().hex() for value in ("proxy-first", "proxy-second")]
        assert all(len(requests) == 1 for requests in go_requests), "actual Go did not reach each distinct peer"
        return [dict(id=case["id"], kind="actual-http-peer", request=case, go=a, rust=b,
                     matched=comparable(a) == comparable(b) and go_requests[i] == rust_requests[i],
                     go_peer_requests=go_requests[i], rust_peer_requests=rust_requests[i])
                for i, (case, a, b) in enumerate(zip(selected, left, right))]
    finally:
        for server in servers:
            server.shutdown()
            server.server_close()
        for thread in threads:
            thread.join(timeout=3)
