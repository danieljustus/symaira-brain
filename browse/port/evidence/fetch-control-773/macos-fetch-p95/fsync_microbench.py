#!/usr/bin/env python3
"""Append-then-flush latency on the benchmark volume: fsync(2) vs F_FULLFSYNC.

Rust's File::sync_all on macOS is F_FULLFSYNC; the journal used it per fetch.url.
Usage: fsync_microbench.py DIR OUTPUT_JSON [N]
"""
import fcntl, json, os, sys, time

directory, output = sys.argv[1], sys.argv[2]
count = int(sys.argv[3]) if len(sys.argv) > 3 else 300
result = {"directory": directory, "samples_per_mode": count, "uname": os.uname().machine, "modes": {}}
for name, flush in (("fsync", os.fsync), ("F_FULLFSYNC", lambda fd: fcntl.fcntl(fd, fcntl.F_FULLFSYNC))):
    path = os.path.join(directory, f"fsync-bench-{os.getpid()}.tmp")
    durations = []
    with open(path, "ab") as stream:
        for _ in range(count):
            stream.write(b"x" * 300 + b"\n")
            stream.flush()
            started = time.perf_counter_ns()
            flush(stream.fileno())
            durations.append(time.perf_counter_ns() - started)
            time.sleep(0.002)
    os.unlink(path)
    ordered = sorted(durations)
    pick = lambda q: ordered[min(len(ordered) - 1, int(len(ordered) * q))]
    result["modes"][name] = {"raw_ns": durations, "p50_ms": pick(.5) / 1e6, "p90_ms": pick(.9) / 1e6,
                             "p95_ms": pick(.95) / 1e6, "max_ms": ordered[-1] / 1e6}
with open(output, "w") as handle:
    json.dump(result, handle, indent=1)
for name, mode in result["modes"].items():
    print(f"{name:12s} p50 {mode['p50_ms']:.3f} p90 {mode['p90_ms']:.3f} p95 {mode['p95_ms']:.3f} max {mode['max_ms']:.3f} ms")
