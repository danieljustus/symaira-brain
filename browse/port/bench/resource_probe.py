#!/usr/bin/env python3
"""Run one child and return its OS-reported per-process peak resident memory."""
from __future__ import annotations

import json
import os
import platform
import subprocess
import sys
import time
from typing import Any

MAX_OUTPUT = 1 << 20


def peak_resident_measurement(process: subprocess.Popen[bytes]) -> tuple[int | None, str | None]:
    if os.name == "nt":
        import ctypes
        from ctypes import wintypes

        class MemoryCounters(ctypes.Structure):
            _fields_ = [
                ("cb", wintypes.DWORD),
                ("PageFaultCount", wintypes.DWORD),
                ("PeakWorkingSetSize", ctypes.c_size_t),
                ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPagedPoolUsage", ctypes.c_size_t),
                ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t),
                ("PagefileUsage", ctypes.c_size_t),
                ("PeakPagefileUsage", ctypes.c_size_t),
            ]

        counters = MemoryCounters()
        counters.cb = ctypes.sizeof(counters)
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        query = psapi.GetProcessMemoryInfo
        query.argtypes = [wintypes.HANDLE, ctypes.POINTER(MemoryCounters), wintypes.DWORD]
        query.restype = wintypes.BOOL
        if not query(wintypes.HANDLE(process._handle), ctypes.byref(counters), counters.cb):
            return None, None
        value = int(counters.PeakWorkingSetSize)
        method = "GetProcessMemoryInfo.PeakWorkingSetSize"
    else:
        try:
            import resource

            value = int(resource.getrusage(resource.RUSAGE_CHILDREN).ru_maxrss)
            if platform.system() == "Linux":
                value *= 1024
                method = "getrusage.RUSAGE_CHILDREN.ru_maxrss_kib"
            else:
                method = "getrusage.RUSAGE_CHILDREN.ru_maxrss_bytes"
        except (ImportError, OSError, ValueError):
            return None, None
    return (value, method) if value > 0 else (None, None)


def main() -> int:
    request: dict[str, Any] = json.loads(sys.stdin.readline())
    command = request["command"]
    started = time.perf_counter_ns()
    process = subprocess.Popen(
        command,
        cwd=request["cwd"],
        env=request["env"],
        stdin=subprocess.PIPE if request.get("stdin") else subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        input_data = request.get("stdin", "").encode() if request.get("stdin") else None
        stdout, stderr = process.communicate(
            input=input_data,
            timeout=float(request.get("timeout", 15)),
        )
        timed_out = False
    except subprocess.TimeoutExpired:
        process.kill()
        stdout, stderr = process.communicate()
        timed_out = True
    duration_ns = time.perf_counter_ns() - started
    peak, method = peak_resident_measurement(process)
    report = {
        "returncode": process.returncode,
        "timed_out": timed_out,
        "duration_ns": duration_ns,
        "peak_rss_bytes": peak,
        "peak_rss_method": method,
        "stdout": stdout[:MAX_OUTPUT + 1].decode("utf-8", errors="replace"),
        "stderr": stderr[:MAX_OUTPUT + 1].decode("utf-8", errors="replace"),
        "stdout_bytes": len(stdout),
        "stderr_bytes": len(stderr),
    }
    print(json.dumps(report, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
