# #773 macOS arm64 fetch p95 value gate (fixed Go baseline)

Scope update 2026-11-06: Go is removed, so no paired comparison exists. The gate is the
recorded Go fetch p95 (5.894 ms) x 1.10 = **6.4834 ms** for the Rust release binary on the
unchanged workload `rust016-static-fetch-html-v1` (daemon `fetch.url`, loopback fixture,
`no_cache=true`, 30 samples, nearest-rank p95), measured with the unmodified
`port/bench/run.py --rust BIN --workload fetch --runs 30`.
Previous recorded evidence: Rust p95 30.326 ms.

## Root cause

Every gated `fetch.url` appends a journal line (`symbrowse-core::journal::Store::append`) and
flushed it with `File::sync_all()`. On macOS Rust implements that as `fcntl(F_FULLFSYNC)`, which
drains the drive write cache. On the benchmark volume it costs p50 0.9 ms but p90 5.9 ms
(`daemon_latency_probe.py` shows the same ~+5 ms bimodal tail on `fetch.url`, and on a refused
connection that never reaches HTTP). The HTTP stack itself was not the bottleneck: the pre-fix
floor was ~1.0 ms. Fix: use plain `fsync(2)` on macOS for the journal line (other Unix systems
already do). Fetch controls (timeouts, per-hop redirect policy, size limits, honest headers)
are untouched. Durability trade-off: the line is written and `fsync`ed in order; only an abrupt
power loss may lose the newest audit lines (SQLite default behaviour).

## Results (raw samples inside each `bench-*.json`; `gate-*.json` recomputes p95 from them)

Host: Apple Silicon arm64 macOS, 12 logical CPUs, shared with other agents' builds.
Binary: `target/release/symbrowse` (release profile, thin LTO), source `c46e9e3b4` + this change.

| Run | Binary | Load avg (1 min) at start | p95 (ms) | median (ms) | max (ms) |
|---|---|---:|---:|---:|---:|
| before-1 | pre-fix | ~190 | 355.380 | 147.069 | 703.4 |
| before-2 | pre-fix | 104 | 23.886 | 6.481 | 41.1 |
| before-3 | pre-fix | 108 | 26.264 | 11.613 | 31.3 |
| after-1 | fixed | 15.7 | 0.689 | 0.405 | 1.456 |
| after-2 | fixed | 15.0 | 0.568 | 0.413 | 1.622 |
| after-3 | fixed | 15.0 | 0.628 | 0.373 | 1.340 |

Gate: all three post-fix runs are <= 6.4834 ms (`gate-after.json`, exit 0). The pre-fix runs
(`gate-before.json`, exit 1) were taken while the machine was heavily loaded and are
**not a clean A/B**: they show the failing mode under load, not the idle pre-fix p95. A clean
pre-fix rebuild was blocked by the NVMe free-space guard (< 5 GiB). The idle pre-fix
floor/tail is documented by `daemon_latency_probe.py` runs (load ~45-80, p50 1.1-2.5 ms,
p90 ~6 ms, p95 5.5-15 ms) and by `fsync-microbench.json`.

`fsync-microbench.json` (taken under heavy disk contention, 4.8 GiB free): fsync(2) p50 0.64 ms,
F_FULLFSYNC p50 5.92 ms; both degrade (p90 > 14 ms) when other builds saturate the volume.
So a loaded or nearly full benchmark volume can still break the gate; the fix removes the
systematic F_FULLFSYNC cost, not external disk contention. Moving the journal flush off the
request path (group commit) would be the architectural option if that is required.

Not covered here: Linux/native CI contract rows (CI workflow `Browse honest fetch native contracts`).
