#!/usr/bin/env python3
"""Owned proxy peers enforce byte-preserving Basic auth on both HTTP routes."""
import argparse
import base64
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlencode, unquote_to_bytes, urlsplit
import fetch_control_process as common
import fetch_control_proxy_fixture as fixture
import fetch_control_raw_proxy as raw_proxy
import fetch_control_tls_fixture as tls


USERINFO = ["", ":", "user", "user:", ":password", "%ff:password", "user:%ff", "%C3%A9:%C3%A9",
            "%00:%80", "user:p%3Aa:ss", "us%3Aer:password", "%2540:%25ff"]


def basic(info):
    user, _, password = info.partition(":")
    return "Basic " + base64.b64encode(unquote_to_bytes(user) + b":" + unquote_to_bytes(password)).decode()


def echo_cases(proxy, label):
    return [dict(id=f"{label}-{port}-{index}-{caller}",
                 url=f"http://93.184.216.34:{port}/echo",
                 proxy=proxy.replace("://", "://" + info + "@", 1),
                 headers={"Proxy-Authorization": "synthetic-caller"} if caller else {})
            for port in ["080", "81"] for index, info in enumerate(USERINFO) for caller in [False, True]]


def assert_echo(observed, infos):
    for row, info in zip(observed, infos):
        assert row["matched"], row
        body = json.loads(bytes.fromhex(row["rust"]["body_hex"]))
        expected = (["synthetic-caller"] if row["request"].get("headers") else []) + [basic(info)]
        assert body["headers"]["proxy-authorization"] == expected, row


def pairs(args, selected, root, environment=None):
    root.mkdir()
    return raw_proxy.pairs(args, selected, root, environment)


class EnforcingPeer(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        info = self.server.credentials[urlsplit(self.path).path[1:]]
        headers = self.headers.get_all("Proxy-Authorization") or []
        accepted = headers == [basic(info)]
        body = json.dumps(dict(headers=headers, accepted=accepted), sort_keys=True, separators=(",", ":")).encode()
        self.send_response_only(200 if accepted else 407)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        self.wfile.flush()

    def log_message(self, *_):
        pass


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["go", "rust", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    observed, enforced, limits = [], [], []
    with tempfile.TemporaryDirectory(prefix="fetch773-proxy-auth-") as temporary:
        root = Path(temporary)
        server, thread = fixture.start()
        try:
            proxy = "http://127.0.0.1:" + str(server.server_port)
            selected = echo_cases(proxy, "http-auth")
            results = pairs(args, selected, root / "plain")
            assert_echo(results, [info for _ in ["080", "81"] for info in USERINFO for _ in [False, True]])
            observed += results
            # Each group starts a fresh client: uppercase/lowercase and schemeless env metadata.
            for index, (key, scheme) in enumerate([("HTTP_PROXY", True), ("http_proxy", True),
                                                   ("HTTP_PROXY", False), ("http_proxy", False)]):
                environment = {key: proxy.replace("://", "://user:%ff@", 1)}
                if not scheme:
                    environment[key] = environment[key].split("://", 1)[1]
                selected = [dict(id=f"env-auth-{index}-{port}-{caller}",
                                 url=f"http://93.184.216.34:{port}/echo",
                                 headers={"Proxy-Authorization": "synthetic-caller"} if caller else {})
                            for port in ["080", "81"] for caller in [False, True]]
                results = pairs(args, selected, root / f"env-{index}", environment)
                assert_echo(results, ["user:%ff"] * len(results))
                for row in results:
                    row["environment"] = environment
                observed += results
            for index, port in enumerate(["080", "81"]):
                target = f"http://93.184.216.34:{port}/echo"
                selected = [dict(id=f"redirect-auth-{port}", url="http://93.184.216.34:080/redirect?" + urlencode(dict(target=target)),
                                 proxy=proxy.replace("://", "://user:%ff@", 1), headers={"Proxy-Authorization": "synthetic-caller"})]
                results = pairs(args, selected, root / f"redirect-{index}")
                assert_echo(results, ["user:%ff"])
                observed += results
        finally:
            fixture.stop(server, thread)
        contexts, ca = tls.contexts(root / "tls")
        trust = tls.linux_trust_env(ca)
        if trust is None:
            limits.append("Owned trusted HTTPS proxy auth requires Linux SSL_CERT_FILE; other native trust backends pending")
        else:
            server, thread = fixture.start(contexts["valid"])
            try:
                proxy = "https://127.0.0.1:" + str(server.server_port)
                selected = echo_cases(proxy, "https-auth")
                results = pairs(args, selected, root / "trusted-tls", trust)
                assert_echo(results, [info for _ in ["080", "81"] for info in USERINFO for _ in [False, True]])
                observed += results
            finally:
                fixture.stop(server, thread)
        server = ThreadingHTTPServer(("127.0.0.1", 0), EnforcingPeer)
        server.credentials = dict(password="user:%ff", username="%ff:password", ascii="user:password", unicode="%C3%A9:%C3%A9")
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            selected = [dict(id=f"enforced-{kind}-{port}", url=f"http://93.184.216.34:{port}/{kind}",
                             proxy=f"http://{info}@127.0.0.1:{server.server_port}")
                        for kind, info in server.credentials.items() for port in ["080", "81"]]
            enforced = pairs(args, selected, root / "enforced")
            for row in enforced:
                assert row["matched"] and row["go"]["status"] == row["rust"]["status"] == 200, row
        finally:
            fixture.stop(server, thread)
    repo = Path(__file__).resolve().parents[3]
    report = dict(candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
                  candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)),
                  echo_matched=len(observed), enforced_matched=len(enforced), echo_pairs=observed, enforced_pairs=enforced,
                  explicit_limits=limits, inherited_limits="Non-UTF8 origin URL userinfo, HTTPS-target CONNECT/SOCKS auth, H2/trailer parity remain outside this correction",
                  binaries_sha256={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [("go", args.go), ("rust", args.rust)]},
                  harness_sha256={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in Path(__file__).parent.glob("fetch_control*.py")})
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Proxy auth: {len(observed)} exact ordered-header pairs and {len(enforced)} actual accepted-auth pairs")


if __name__ == "__main__":
    main()
