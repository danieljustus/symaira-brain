"""Disposable certificate roots; never install a CA in an operator trust store."""
import platform
import ssl
import subprocess


def contexts(root):
    root.mkdir()
    def openssl(*args):
        subprocess.run(["openssl", *args], cwd=root, check=True,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=15)
    openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
            "-subj", "/CN=Owned Fetch773 CA", "-keyout", "ca.key", "-out", "ca.pem")
    result = {}
    for label, san in [("valid", "IP:127.0.0.1,DNS:localhost"), ("wrong-host", "DNS:owned.invalid")]:
        openssl("req", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=Owned Fetch773 peer",
                "-keyout", label + ".key", "-out", label + ".csr")
        (root / (label + ".ext")).write_text("subjectAltName=" + san + "\nextendedKeyUsage=serverAuth\n")
        openssl("x509", "-req", "-in", label + ".csr", "-CA", "ca.pem", "-CAkey", "ca.key",
                "-CAcreateserial", "-days", "1", "-extfile", label + ".ext", "-out", label + ".pem")
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(root / (label + ".pem"), root / (label + ".key"))
        result[label] = context
    for path in root.iterdir():
        if path.suffix == ".key":
            path.chmod(0o600)
    return result, root / "ca.pem"


def linux_trust_env(ca):
    # Go and OpenSSL native-tls both support this per-process trust input on Linux.
    # macOS/Windows system-root backends need separate native owned-CA evidence.
    return {"SSL_CERT_FILE": str(ca)} if platform.system() == "Linux" else None
