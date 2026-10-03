#!/usr/bin/env python3
"""Actual raw proxy authority and specifically accepted automatic-Referer proof."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import tempfile
from urllib.parse import urlencode
import fetch_control_process as common
import fetch_control_proxy_fixture as fixture
import fetch_control_tls_fixture as tls


def pairs(args, selected, root, env=None):
    left = common.execute(args.go, selected, root / "go", env)
    right = common.execute(args.rust, selected, root / "rust", env)
    return [dict(id=c["id"], request=c, go=g, rust=r,
                 matched=common.comparable(g) == common.comparable(r))
            for c, g, r in zip(selected, left, right)]


def cases(proxy):
    selected = [dict(id="proxy-target-port-" + p, url="http://93.184.216.34:" + p + "/echo",
                     method="GET", proxy=proxy) for p in ["80", "080", "00080", "81", "00081"]]
    def add(name, path="/echo", **kw):
        selected.append(dict(id=name, url="http://93.184.216.34:080" + path, proxy=proxy, **kw))
    for method in ["POST", "PUT", "HEAD"]:
        add("raw-method-" + method.lower(), method=method, body="owned-body")
    for status in [301, 302, 303, 307, 308]:
        for port in ["080", "81"]:
            target = "http://93.184.216.34:" + port + "/echo"
            add(f"raw-redirect-{status}-to-{port}", "/redirect?" + urlencode(dict(status=status, target=target)),
                method="POST", body="owned-body", headers={"Content-Type": "text/plain", "Authorization": "synthetic"})
    for port in ["080", "81"]:
        add("raw-auth-" + port, headers={"Authorization": "caller-origin", "Proxy-Authorization": "caller-proxy"})
        selected[-1]["url"] = "http://target:secret@93.184.216.34:" + port + "/echo"
        selected[-1]["proxy"] = proxy.replace("://", "://proxy%40user:p%3Aa%20ss@")
    add("raw-proxy-custom-auth", headers={"Proxy-Authorization": "caller-proxy"})
    for name, target in [("same-host-canonical", "http://93.184.216.34:81/echo"),
                         ("cross-host", "http://owned-origin.test:080/echo")]:
        add("raw-auth-redirect-" + name, "/redirect?" + urlencode(dict(target=target)),
            headers={"Authorization": "caller-origin", "Proxy-Authorization": "caller-proxy", "Cookie": "owned=yes"})
        selected[-1]["proxy"] = proxy.replace("://", "://proxy%40user:p%3Aa%20ss@")
    for path in ["body", "gzip"]:
        for maximum in [7, 8, 25]:
            add(f"raw-{path}-limit-{maximum}", "/" + path, max_body=maximum)
    for path in ["slow-headers", "slow-body"]:
        add("raw-timeout-" + path, "/" + path, timeout_ms=20)
    add("raw-private-proxy-denied", allow_private=False)
    return selected


def referer(args, root, base):
    def redirect(target="/echo", fragment="", **kw):
        return dict(url=base + "/redirect?" + urlencode(dict(target=target)) + fragment, method="GET", **kw)
    selected = [dict(id="automatic-fragment", **redirect(fragment="#private-fragment")),
                dict(id="explicit-fragment", **redirect(headers={"Referer": "http://example.com/custom#fragment"})),
                dict(id="userinfo-same-host", **redirect()),
                dict(id="userinfo-cross-host", **redirect(base.replace("127.0.0.1", "localhost") + "/echo")),
                dict(id="redirect-host-header", **redirect(headers={"Host": "custom.example"})),
                dict(id="direct-host-header", url=base + "/echo", method="GET", headers={"Host": "custom.example"}),
                dict(id="head302", **redirect()), dict(id="lowercase-method302", **redirect())]
    for i in [2, 3]:
        selected[i]["url"] = selected[i]["url"].replace("http://", "http://user:secret@", 1)
    selected[6]["method"], selected[7]["method"] = "HEAD", "head"
    observed = pairs(args, selected, root)
    assert all(x["matched"] for x in observed[1:]), observed
    first = observed[0]
    go = json.loads(bytes.fromhex(first["go"]["body_hex"]))
    native = json.loads(bytes.fromhex(first["rust"]["body_hex"]))
    expected = selected[0]["url"]
    assert go["headers"]["referer"] == [expected], first
    assert native["headers"]["referer"] == [expected.split("#", 1)[0]], first
    assert first["go"]["status"] == first["rust"]["status"] == 200
    assert first["go"]["url"] == first["rust"]["url"] == base + "/echo"
    first["accepted_divergence"] = "E011: automatic Referer omits fragment; explicit caller header remains byte-equal"
    return observed


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["go", "rust", "output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    server, thread = fixture.start()
    results, native, tls_results, skips = [], [], [], []
    try:
        with tempfile.TemporaryDirectory(prefix="fetch773-raw-proxy-") as raw:
            root = Path(raw)
            proxy = "http://127.0.0.1:" + str(server.server_port)
            (root / "plain").mkdir()
            results = pairs(args, cases(proxy), root / "plain")
            assert all(x["matched"] for x in results), [x for x in results if not x["matched"]]
            (root / "referer").mkdir()
            refs = referer(args, root / "referer", proxy)
            # Go has no named jar: this independently checks the declared native feature.
            selected = [dict(id="jar-set", url="http://127.0.0.1:080/cookie-set", proxy=proxy, session="A"),
                        dict(id="jar-direct", url=proxy + "/cookie", session="A"),
                        dict(id="jar-other", url="http://127.0.0.1:080/cookie", proxy=proxy, session="B"),
                        dict(id="jar-return", url="http://127.0.0.1:080/cookie", proxy=proxy, session="A")]
            native = common.execute(args.rust, selected, root / "named-jar")
            assert [bytes.fromhex(x["body_hex"]) for x in native] == [b"set", b"raw_owned=first", b"", b"raw_owned=first"], native
            contexts, ca = tls.contexts(root / "tls")
            trust = tls.linux_trust_env(ca)
            for label in ["valid", "wrong-host"]:
                peer, worker = fixture.start(contexts[label])
                try:
                    tls_proxy = "https://127.0.0.1:" + str(peer.server_port)
                    for trusted in [False, True]:
                        if trusted and trust is None:
                            skips.append("owned trusted " + label + " TLS proof requires Linux SSL_CERT_FILE; native other root backends pending")
                            continue
                        group = root / ("tls-" + label + "-" + str(trusted)); group.mkdir()
                        selected = [dict(id="tls-" + label + "-" + str(trusted), url="http://93.184.216.34:080/echo", proxy=tls_proxy)]
                        observed = pairs(args, selected, group, trust if trusted else None)
                        assert all(x["matched"] for x in observed), observed
                        if label == "valid" and trusted:
                            assert observed[0]["rust"]["error"] == "", observed
                        else:
                            assert observed[0]["go"]["error"] == observed[0]["rust"]["error"] == "transport", observed
                        tls_results += observed
                finally:
                    fixture.stop(peer, worker)
        repo = Path(__file__).resolve().parents[3]
        report = dict(candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=repo, text=True).strip(),
            candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=repo)),
            proxy_pairs=results, referer_pairs=refs, native_named_jar=native, tls_pairs=tls_results, explicit_tls_limits=skips,
            proxy_matched=len(results), referer_equal=7, referer_authorized_divergence=1,
            tls_matched=len(tls_results), native_named_jar_assertions=4,
            binaries_sha256={name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in [("go", args.go), ("rust", args.rust)]},
            harness_sha256={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in Path(__file__).parent.glob("fetch_control*.py")},
            trust_policy="No operator trust-store writes; TLS certificates/CA exist only in owned disposable roots")
        args.output.write_text(json.dumps(report, indent=2) + "\n")
        print(f"raw proxy: {len(results)} exact pairs; Referer7 exact+1 authorized; TLS{len(tls_results)} exact; native jar4")
    finally:
        fixture.stop(server, thread)


if __name__ == "__main__":
    main()
