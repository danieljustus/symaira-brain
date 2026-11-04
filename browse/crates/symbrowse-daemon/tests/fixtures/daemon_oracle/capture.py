#!/usr/bin/env python3
"""Capture Go-daemon oracle responses for scenarios.json (BROWSE-DAEMON).

Usage: capture.py <symbrowse-go-binary> <scenarios.json> <out.json> <work-dir>

Runs the Go `symbrowse daemon` with an isolated HOME/XDG/TMPDIR under
<work-dir>, a local fixture HTTP server, and real headless Chrome, then sends
each scenario frame over the unix socket and records the raw response.
Only localhost is used.  The recorded file is the frozen oracle that the Rust
tests replay without Go.
"""
import http.server
import json
import os
import platform
import shutil
import socket
import subprocess
import sys
import threading
import time

with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixture_site.json")) as _fh:
    PAGES = {
        path: (entry["content_type"], entry["body"].encode(), entry["headers"])
        for path, entry in json.load(_fh).items()
    }


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):  # noqa: N802
        path = self.path.split("?")[0]
        entry = PAGES.get(path)
        if entry is None:
            self.send_response(404)
            self.end_headers()
            return
        ctype, body, headers = entry
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        for key, value in headers.items():
            self.send_header(key, value)
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *args):
        pass


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def send(sock_path, frame):
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(60)
        s.connect(sock_path)
        s.sendall((json.dumps(frame) + "\n").encode())
        buf = b""
        while not buf.endswith(b"\n"):
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
    return json.loads(buf)


def send_pipeline(sock_path, frames):
    """Send several frames back-to-back on ONE connection (Go keeps the
    connection usable after an operation timeout) and read every response."""
    with socket.socket(socket.AF_UNIX) as s:
        s.settimeout(60)
        s.connect(sock_path)
        s.sendall(b"".join((json.dumps(f) + "\n").encode() for f in frames))
        buf = b""
        while buf.count(b"\n") < len(frames):
            chunk = s.recv(65536)
            if not chunk:
                break
            buf += chunk
    return [json.loads(line) for line in buf.splitlines() if line.strip()]


def substitute(value, base, workdir):
    if isinstance(value, str):
        return value.replace("{BASE}", base).replace("{WORK}", workdir)
    if isinstance(value, list):
        return [substitute(v, base, workdir) for v in value]
    if isinstance(value, dict):
        return {k: substitute(v, base, workdir) for k, v in value.items()}
    return value


def main():
    binary, scenarios_path, out_path, work = sys.argv[1:5]
    work = os.path.abspath(work)
    shutil.rmtree(work, ignore_errors=True)
    for sub in ("home", "xdg/config", "xdg/data", "xdg/state", "xdg/cache", "tmp", "fixtures"):
        os.makedirs(os.path.join(work, sub))
    port = free_port()
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    base = f"http://127.0.0.1:{port}"
    with open(scenarios_path) as fh:
        spec = json.load(fh)
    env = {
        "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
        "HOME": os.path.join(work, "home"),
        "XDG_CONFIG_HOME": os.path.join(work, "xdg/config"),
        "XDG_DATA_HOME": os.path.join(work, "xdg/data"),
        "XDG_STATE_HOME": os.path.join(work, "xdg/state"),
        "XDG_CACHE_HOME": os.path.join(work, "xdg/cache"),
        "TMPDIR": os.path.join(work, "tmp"),
        "SYMBROWSE_STATE_DIR": os.path.join(work, "xdg/state/symbrowse"),
        "SYMBROWSE_EXECUTABLE_PATH": "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    }
    env.update(spec.get("env", {}))
    session = spec.get("session", "oracle")
    args = [binary, "daemon", "--session", session, "--headless", "--allow-private"] + spec.get("daemon_args", [])
    log = open(os.path.join(work, "daemon.log"), "wb")
    proc = subprocess.Popen(args, env=env, stdout=log, stderr=log, cwd=work)
    results = []
    sock_path = None
    try:
        sock_path = None
        for _ in range(300):
            for root, _dirs, files in os.walk(work):
                for name in files:
                    if name == session + ".sock":
                        sock_path = os.path.join(root, name)
            if sock_path:
                break
            time.sleep(0.1)
        if not sock_path:
            # sockets are usually under TMPDIR or XDG_RUNTIME_DIR; fall back to status lookup
            raise SystemExit("daemon socket not found under work dir; see daemon.log")
        for step in spec["steps"]:
            if "pipeline" in step:
                frames = [dict(substitute(f, base, work), session=session) for f in step["pipeline"]]
                responses = send_pipeline(sock_path, frames)
                results.append({"name": step["name"], "pipeline": step["pipeline"], "responses": responses})
                print(step["name"], "pipeline", [r.get("success") for r in responses])
                continue
            frame = substitute(step["frame"], base, work)
            if step.get("session_override"):
                frame["session"] = step["session_override"]
            elif "session" not in frame:
                frame["session"] = session
            delay = step.get("sleep_before", 0)
            if delay:
                time.sleep(delay)
            if step.get("approve"):
                # TTY-mode policy asks a human over the OOB channel for eval,
                # network-mock, download, upload and credential commands.
                # A second client plays the human: status -> complete.
                box = {}
                worker = threading.Thread(target=lambda: box.update(r=send(sock_path, frame)))
                worker.start()
                time.sleep(0.4)
                status = send(sock_path, {"cmd": "oob.status", "session": session})
                prompt = (status.get("data") or {}).get("prompt") or {}
                if prompt.get("id"):
                    send(sock_path, {"cmd": "oob.complete", "args": {"id": prompt["id"]}, "session": session})
                worker.join(60)
                response = box["r"]
            else:
                response = send(sock_path, frame)
            results.append({"name": step["name"], "frame": step["frame"], "response": json.loads(json.dumps(response).replace(base, "{BASE}").replace(work, "{WORK}"))})
            print(step["name"], "ok" if response.get("success") else "ERR " + json.dumps(response.get("error")))
    finally:
        try:
            send(sock_path, {"cmd": "daemon.stop", "session": session}) if sock_path else None
        except Exception:
            pass
        try:
            proc.wait(timeout=10)
        except Exception:
            proc.kill()
        server.shutdown()
    go_version = subprocess.run(["/opt/homebrew/bin/go", "version"], capture_output=True, text=True).stdout.strip()
    out = {
        "provenance": {
            "generator": "browse/crates/symbrowse-daemon/tests/fixtures/daemon_oracle/capture.py",
            "scenarios": os.path.basename(scenarios_path),
            "go_version": go_version,
            "platform": f"{platform.system().lower()}/{platform.machine()}",
            "argv": args[1:],
            "chrome": "/Applications/Google Chrome.app (headless, temp profile)",
            "binary_source_commit": os.environ.get("ORACLE_COMMIT", ""),
        },
        "results": results,
    }
    with open(out_path, "w") as fh:
        json.dump(out, fh, indent=2, sort_keys=False)
        fh.write("\n")


if __name__ == "__main__":
    main()
