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

## Re-run at HEAD e2110dc89 under shared-host load (2026-11-10, later session)

Rebuilt `target/release/symbrowse` (sha256 `869e4c19...b58170`) at `e2110dc89` and repeated the
unmodified `port/bench/run.py --workload fetch --runs 30` nine times (`head-rerun/bench-{A,B,C}{1,2,3}.json`,
recomputed in `head-rerun/gate-all-9.json`). Series A ran at 1-minute load ~86-89 (other agents'
builds), series B and C at ~20. All 270 samples passed the response-semantics check.

| Series | p95 (ms) per run | Gate |
|---|---|---|
| A (load ~88) | 3.189 / 81.343 / 5.470 | 2 of 3 under 6.4834 |
| B (load ~20) | 37.213 / 6.209 / 1.115 | 2 of 3 under 6.4834 |
| C (load ~20) | 42.201 / 3.763 / 58.651 | 1 of 3 under 6.4834 |

Medians stay 0.8-2.0 ms in every run: the systematic `F_FULLFSYNC` cost is gone. The p95 gate,
however, is **not stable on a busy host**: 5 of 9 runs pass, 4 fail with a few slow samples
(max 28-163 ms), the same shape as the fsync tail in `fsync-microbench.json`. The three quiet-host
runs recorded above (p95 0.568-0.689 ms, load ~15) remain the only runs where the gate held 3/3.
So the committed claim is limited to a quiet benchmark host; a loaded/contended volume can still
fail it. Making the journal write non-blocking for the request (group commit / flush off the
request path) is the open design decision if the gate must hold under contention; it changes the
audit-line durability contract and was not done here.

## Quiet-host gate on main 712e3e31b (2026-11-10)

Decision for #773: the p95 gate is measured on a quiet host only; the journal write stays
synchronous (no group commit), so the audit-line durability contract is unchanged.

Release binary built at `712e3e31b` (sha256 `5899cca7...c5c18d`), then
`port/bench/run.py --workload fetch --runs 30` three times at 1-minute load ~15
(`quiet-main/bench-quiet-{1,2,3}.json`, recomputed in `quiet-main/gate-quiet.json`):

| Run | p95 (ms) | median (ms) | max (ms) | Gate (<= 6.4834 ms) |
|---|---|---|---|---|
| 1 | 1.290 | 0.761 | 2.950 | pass |
| 2 | 1.549 | 0.718 | 2.477 | pass |
| 3 | 3.011 | 1.804 | 3.945 | pass |

All 90 samples passed the response-semantics check and the wrong-but-fast negative control was
rejected. Under contention (load 20-90) the gate is not stable, see the section above.
On macOS the runner needs `SYMAIRA_EXTERNAL_RUNTIME_ROOT` set to a very short directory on the
NVMe volume, otherwise the daemon socket path exceeds `SUN_LEN` and the report records
`status: error` with zero samples.
