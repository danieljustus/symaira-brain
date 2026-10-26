"""Loopback fake sync relay with byte-level control over every response.

The relay binds 127.0.0.1 on an ephemeral port, records each request (method,
target, lower-cased headers, body) and answers with the bytes a scenario
prescribes. Nothing here ever connects outward.
"""
import socket
import threading
import time


def http(status, body=b"", headers=None, length="auto", reason="X"):
    """A complete HTTP/1.1 response; `length` may be False or an explicit int."""
    lines = [f"HTTP/1.1 {status} {reason}"]
    for name, value in (headers or {}).items():
        lines.append(f"{name}: {value}")
    if length == "auto":
        lines.append(f"Content-Length: {len(body)}")
    elif length is not False:
        lines.append(f"Content-Length: {length}")
    return ("\r\n".join(lines) + "\r\n\r\n").encode() + body


def json_reply(status, text, **kwargs):
    return http(status, text.encode() if isinstance(text, str) else text,
                {"Content-Type": "application/json"}, **kwargs)


class Relay:
    """`responder(request) -> list of ("send", bytes) | ("sleep", seconds) | ("close",)`."""

    def __init__(self, responder):
        self.responder = responder
        self.requests = []
        self._socket = socket.socket()
        self._socket.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self._socket.bind(("127.0.0.1", 0))
        self._socket.listen(8)
        self.port = self._socket.getsockname()[1]
        self._closing = False
        threading.Thread(target=self._accept, daemon=True).start()

    def close(self):
        self._closing = True
        try:
            self._socket.close()
        except OSError:
            pass

    def _accept(self):
        while not self._closing:
            try:
                connection, _ = self._socket.accept()
            except OSError:
                return
            threading.Thread(target=self._serve, args=(connection,), daemon=True).start()

    def _serve(self, connection):
        buffer = b""
        try:
            while True:
                while b"\r\n\r\n" not in buffer:
                    chunk = connection.recv(65536)
                    if not chunk:
                        return
                    buffer += chunk
                head, _, buffer = buffer.partition(b"\r\n\r\n")
                lines = head.decode("latin-1").split("\r\n")
                method, target, _ = lines[0].split(" ", 2)
                headers = [(line.split(":", 1)[0].lower(), line.split(":", 1)[1].strip())
                           for line in lines[1:]]
                length = int(dict(headers).get("content-length", "0"))
                while len(buffer) < length:
                    chunk = connection.recv(65536)
                    if not chunk:
                        return
                    buffer += chunk
                body, buffer = buffer[:length], buffer[length:]
                request = dict(method=method, target=target, headers=sorted(headers),
                               body=body.decode("utf-8", "replace"))
                self.requests.append(request)
                for step in self.responder(request):
                    if step[0] == "send":
                        connection.sendall(step[1])
                    elif step[0] == "sleep":
                        time.sleep(step[1])
                    elif step[0] == "close":
                        return
        except OSError:
            return
        finally:
            try:
                connection.close()
            except OSError:
                pass


def reply(data):
    return [("send", data)]
