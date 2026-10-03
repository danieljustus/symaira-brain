# #773 honest fetch evidence

Final Linux source: `8973726d531a946a6e4a041f0e590b9a16407bbe`, clean, with
actual main `31de72294521701fc4b1a0bce39f03cc34d72e7e` integrated normally.
The publication successor adds only this evidence directory. Candidate source
hashes in the receipts must remain byte-identical to the tested source commit.

- `linux-control-report.json`: 231/231 actual Go/native observations: 64 private
  HTTP exchanges and 167 route-only selections opening no network connection.
  Windows has two fewer routing cases because environment names are case
  insensitive; the Unix conflicting-case test cannot be represented there.
- `linux-negative-controls.json`: five executable wrappers mutate actual native
  body/header/error/route output or remove a case; all are rejected. No Go
  fallback exists in the private candidate process environment.
- `linux-source-bound-summary.json` and JSON-wrapped literal logs: 117 parent tests pass; the one
  ignored daemon child is explicitly invoked seven times by its passing parent,
  which requires a `1 passed` summary for every invocation. The fetch package
  has 49 passing tests and no ignored tests. Strict Clippy, fmt, actionlint,
  original Go checks and matrix validation pass. Two manual macOS NVMe benchmark
  tests are skipped and excluded from native transport evidence.
- `linux-paired-30.json` and comparison: exactly 30 samples for each of CLI, MCP,
  daemon and fetch in both implementations, plus original wrong-body controls.
  Both executables have digest-bound, clean immutable-source build receipts;
  Go is `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` / Go1.26.7, rather than the
  measurement checkout's commit.

| Workload | Go p95 (ms) | Rust p95 (ms) |
|---|---:|---:|
| CLI | 8.954529 | 4.282024 |
| MCP | 9.336215 | 4.641167 |
| Daemon | 20.945106 | 20.454230 |
| Fetch | 2.334890 | 0.843896 |

The unchanged <=10% p95 gate passes for all four workloads. The optimized Rust
executable is 10,577,392 bytes. The established historical Darwin arm64 size
baseline gives 42.296% reduction; the same-host fresh Linux Go executable is
26,611,127 bytes. These host/baseline distinctions prevent interpreting this
Linux receipt as six-platform release certification. Native CI receipts remain
pending, Windows paired daemon/fetch benchmarking remains unported, and neither
#773 closure nor default cutover is claimed.

All `original-*` files retain literal prior failures, the independent cc8 review,
70 extra routing cases, three independent multi-member gzip cases, cc8's original
154-case success and paired30 receipt, and the literal original50 measurement.
The original50 provenance note explains the old mistaken Go revision label
without rewriting its historical evidence. Original cc8 executable snapshots
were retained with their receipt digests before the focused correction.

The [ADR](../../../docs/rust-port/adr-honest-fetch-control-773.md) describes the
scope and long-term decisions. Native six-target CI uses
`.github/workflows/browse-fetch-native.yml`; its separately visible Linux value
job uploads raw paired evidence including failures and is allowed to fail while
the remaining cutover gates stay open.
