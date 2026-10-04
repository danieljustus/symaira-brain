"""Owned HTTP proxy request-target fixture; no connection to an external origin."""
import gzip
import json
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlsplit


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        parsed = urlsplit(self.path)
        params = parse_qs(parsed.query)
        body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
        record = dict(method=self.command, target=self.path, body=body.decode(),
                      headers={name.lower(): self.headers.get_all(name) for name in
                               ["Host", "Authorization", "Proxy-Authorization", "Cookie", "Referer", "Content-Type"]
                               if self.headers.get_all(name)})
        self.server.observations.append(record)
        status, headers = 200, {}
        content = json.dumps(record, sort_keys=True, separators=(",", ":")).encode()
        if parsed.path == "/redirect":
            status = int(params.get("status", ["302"])[0])
            headers["Location"] = params["target"][0]
            content = b"redirect"
        elif parsed.path == "/gzip":
            content = gzip.compress(b"bounded raw-proxy body", mtime=0)
            headers["Content-Encoding"] = "gzip"
        elif parsed.path == "/cookie-set":
            headers["Set-Cookie"] = "raw_owned=first; Path=/"
            content = b"set"
        elif parsed.path == "/cookie":
            content = self.headers.get("Cookie", "").encode()
        elif parsed.path in ("/body", "/slow-headers", "/slow-body"):
            content = b"12345678"
        if parsed.path == "/slow-headers":
            time.sleep(0.08)
        self.send_response_only(status)
        for name, value in headers.items():
            self.send_header(name, value)
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        try:
            if self.command == "HEAD":
                return
            if parsed.path == "/slow-body":
                self.wfile.write(content[:1])
                self.wfile.flush()
                time.sleep(0.08)
                content = content[1:]
            self.wfile.write(content)
            self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass

    do_POST = do_PUT = do_HEAD = do_GET
    # A real custom method lets the client perform and prove its 302→GET rewrite.
    # Avoid stdlib's unrelated volatile Date-bearing unsupported-method response.
    do_head = do_GET

    def log_message(self, *_):
        pass


def start(context=None):
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    server.observations = []
    if context:
        server.socket = context.wrap_socket(server.socket, server_side=True)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, thread


def stop(server, thread):
    server.shutdown()
    server.server_close()
    thread.join(timeout=3)
