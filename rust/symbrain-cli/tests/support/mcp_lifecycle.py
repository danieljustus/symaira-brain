#!/usr/bin/python3
"""Synthetic MCP process tree: only group SIGTERM can satisfy the test."""

import json
import os
from pathlib import Path
import signal
import sys

root = Path(__file__).parent
descendant = None


def watchdog(_signal, _frame):
    # Failure containment only. This path never writes successful reap evidence.
    if descendant:
        try:
            os.kill(descendant, signal.SIGKILL)
            os.waitpid(descendant, 0)
        except ProcessLookupError:
            pass
    os._exit(124)


def terminate(_signal, _frame):
    # Do not forward SIGTERM: the broker must deliver it to the entire group.
    pid, status = os.waitpid(descendant, 0)
    signaled = os.WIFSIGNALED(status)
    evidence = {
        "pid": pid,
        "signaled": signaled,
        "signal": os.WTERMSIG(status) if signaled else None,
    }
    (root / "reaped.json").write_text(json.dumps(evidence), encoding="ascii")
    os._exit(0 if signaled and os.WTERMSIG(status) == signal.SIGTERM else 1)


read_ready, write_ready = os.pipe()
descendant = os.fork()
if descendant == 0:
    os.close(read_ready)
    signal.signal(signal.SIGTERM, signal.SIG_DFL)
    for fd in (0, 1, 2):
        os.close(fd)
    os.write(write_ready, b"R")
    os.close(write_ready)
    while True:
        signal.pause()

os.close(write_ready)
signal.signal(signal.SIGALRM, watchdog)
signal.alarm(15)
signal.signal(signal.SIGTERM, terminate)
assert os.read(read_ready, 1) == b"R"
os.close(read_ready)
identity = {"pid": os.getpid(), "pgid": os.getpgrp(), "descendant": descendant}
# Atomic identity is also available to failure cleanup before MCP readiness.
pending = root / "identity.pending"
pending.write_text(json.dumps(identity), encoding="ascii")
pending.replace(root / "identity.json")

for line in sys.stdin:
    request = json.loads(line)
    if "id" not in request:
        continue
    method = request["method"]
    if method == "initialize":
        result = {"protocolVersion": "2024-11-05", "capabilities": {"tools": {}}, "serverInfo": {"name": "lifecycle-fake", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "lifecycle_ready", "inputSchema": {"type": "object"}}]}
    elif method == "tools/call":
        assert request["params"]["name"] == "lifecycle_ready"
        result = {"content": [{"type": "text", "text": json.dumps(identity)}], "isError": False}
    else:
        raise AssertionError(method)
    print(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": result}), flush=True)

# EOF alone must not make the lifecycle assertion pass.
while True:
    signal.pause()
