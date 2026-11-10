#!/usr/bin/env python3
"""Black-box latency breakdown of the daemon fetch path (profiling aid, not evidence)."""
import json, os, socket, statistics, subprocess, sys, tempfile, threading, time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

BIN = sys.argv[1]
N = int(sys.argv[2]) if len(sys.argv) > 2 else 40
BODY = b"<html><head><title>RUST-016</title></head><body><main><p>fixture</p></main></body></html>\n"


class H(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(BODY)))
        self.end_headers()
        self.wfile.write(BODY)

    def log_message(self, *a):
        pass


srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
threading.Thread(target=srv.serve_forever, daemon=True).start()
root = Path(tempfile.mkdtemp(prefix="p", dir=os.environ.get("PROBE_ROOT","/Volumes/1TB_NVMe_SN850X/t")))
home = root / "h"
for d in ("h", "r", "c", "tmp"):
    (root / d).mkdir(mode=0o700, exist_ok=True)
env = {
    "HOME": str(home), "XDG_CONFIG_HOME": str(home / ".config"), "XDG_DATA_HOME": str(home / ".local/share"),
    "XDG_CACHE_HOME": str(root / "c"), "XDG_STATE_HOME": str(home / ".local/state"), "XDG_RUNTIME_DIR": str(root / "r"),
    "TMPDIR": str(root / "tmp"), "LANG": "C", "LC_ALL": "C", "TZ": "UTC", "TERM": "dumb", "NO_COLOR": "1",
    "SYMBROWSE_CHECK_UPDATES": "0", "SYMBROWSE_SYMGUARD": "off", "SYMBROWSE_ALLOW_PRIVATE": "true",
    "SYMBROWSE_MODE": "browser", "PATH": os.environ["PATH"],
}
session = "probe"
proc = subprocess.Popen([BIN, "daemon", "--session", session, "--mode", "static"], env=env, cwd=root,
                        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, start_new_session=True)
paths = [root / "r/symbrowse" / f"{session}.sock", home / "Library/Caches/symbrowse/run" / f"{session}.sock"]
t0 = time.monotonic()
while time.monotonic() - t0 < 10 and not any(p.exists() for p in paths):
    time.sleep(0.01)
sock = next(p for p in paths if p.exists())


def call(frame):
    t = time.perf_counter_ns()
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as c:
        c.settimeout(15)
        c.connect(str(sock))
        c.sendall((json.dumps(frame) + "\n").encode())
        data = c.recv(1 << 20)
    return (time.perf_counter_ns() - t) / 1e6, data


def stat(name, fn):
    ts = [fn(i)[0] for i in range(N)]
    first = ts[0]
    ts2 = sorted(ts)
    p95 = ts2[max(0, -(-N * 95 // 100) - 1)]
    q=lambda f: ts2[min(N-1,int(N*f))]
    print(f"{name:26s} first {first:6.2f} min {ts2[0]:5.2f} p25 {q(.25):5.2f} p50 {q(.5):5.2f} p75 {q(.75):5.2f} p90 {q(.9):6.2f} p95 {p95:6.2f} max {ts2[-1]:6.2f}")


url = lambda i: f"http://127.0.0.1:{srv.server_port}/fixture.html?run={i}"
stat("daemon.ping", lambda i: call({"cmd": "daemon.ping", "session": session}))
stat("session.info", lambda i: call({"cmd": "session.info", "session": session}))
stat("fetch.url (bench frame)", lambda i: call({"cmd": "fetch.url", "session": session, "args": {"url": url(i), "no_cache": True}}))
stat("fetch.url same url", lambda i: call({"cmd": "fetch.url", "session": session, "args": {"url": url(0), "no_cache": True}}))
stat("fetch.url raw", lambda i: call({"cmd": "fetch.url", "session": session, "args": {"url": url(i), "no_cache": True, "raw": True}}))
stat("fetch.url refused port", lambda i: call({"cmd": "fetch.url", "session": session, "args": {"url": "http://127.0.0.1:9/x", "no_cache": True}}))
# raw python baseline: the fixture alone
import urllib.request
stat("python urllib -> fixture", lambda i: (lambda t: (urllib.request.urlopen(url(i)).read() and (time.perf_counter_ns() - t) / 1e6, b""))(time.perf_counter_ns()))
print(call({"cmd": "fetch.url", "session": session, "args": {"url": url(999), "no_cache": True}})[1][:400])
try:
    call({"cmd": "daemon.stop", "session": session})
    proc.wait(timeout=5)
except Exception:
    pass

try:
    os.killpg(proc.pid, 9)
except ProcessLookupError:
    pass
srv.shutdown()
