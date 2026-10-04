# Second independent-review corrections: source-bound Linux evidence

Clean tested source: `1f82e9cf8812c5b383a19349241787215a857493`, with actual main
`31de72294521701fc4b1a0bce39f03cc34d72e7e` already a normal ancestor. This
directory is added by an evidence-only successor; production/test/harness inputs
must stay byte-identical to this tested source.

The original rejected a073 candidate, three P2 findings, actual ten additional
routing observations, two proxy exchanges and actual prior executable snapshots
remain unchanged in `../independent-review-a073/` and its digest-bound archive.

The correction rejects signed CIDR prefixes, retains literal target authority
ports in initial and redirect proxy selection, and replaces delimiter cache keys
with structural transport identity. Every hop preserves target/proxy validation,
shared DNS pinning, the ten-request redirect limit and common deadline. Tests
cover body/header transformations, credential stripping, Referer behavior and
named-session jar isolation. See the public
[ADR](../../../../docs/rust-port/adr-honest-fetch-control-773.md).

Fresh clean-source gates pass:

- 251/251 actual Go/native observations: 74 HTTP exchanges and 177 routing-only
  selections with no socket. Windows has 249 cases, with 175 routing-only cases.
- Five intended executable mutations of actual native output are rejected.
- 119 parent Rust tests across 25 summaries, zero failures; one deliberately
  ignored daemon child is explicitly executed seven times by its passing parent.
  The fetch package has 51 passing tests and no ignored tests.
- Strict fetch/daemon/CLI Clippy with all targets/features, formatting, actionlint
  and matrix validation. Original immutable Go fixtures retain 28 static vectors,
  11 profiles and 15 robots cases; checks pass both Go1.26.6 and native-CI Go1.26.7.
  Uncached original production fetch tests pass. Four build-identity tests pass;
  five legacy benchmark tests pass and two manual macOS NVMe tests remain skipped.

Both value measurements preserve every one of their 240 raw samples (30 per four
workloads and implementation), the same immutable optimized executables, original
wrong-body controls and unchanged comparator. The first measurement is genuinely
**blocked** by MCP and daemon p95 and stays under `original-loaded-*`. Concurrent
compiler activity was observed when it was inspected. Root then coordinated a
quiet window; the entire fresh 30-pair measurement was rerun without sample
exclusions. Quarter-second CPU/load records and process snapshots are retained
under `linux-quiet-load.json`. The quiet measurement passes:

| Workload | Go p95 (ms) | Rust p95 (ms) |
|---|---:|---:|
| CLI | 7.741956 | 4.328724 |
| MCP | 8.320250 | 4.206590 |
| Daemon | 20.641629 | 20.406333 |
| Fetch | 2.617087 | 0.997628 |

The native executable is 10,598,888 bytes. The established historical Darwin
arm64 baseline gives 42.17884% size improvement; the same-host Linux Go executable
is 26,611,127 bytes. These timing/size observations remain supplemental Linux
evidence, not native six-target release certification. Source-bound receipts
verify 30 fetch inputs, ten harness/workflow inputs, 148 native build inputs and
415 immutable Go build inputs. Go provenance remains DC/SDK1.26.7 rather than
the measurement checkout's revision.

Literal logs include the earlier nonfinal Clippy range suggestion and a failed
Go wrapper that referenced an absent local SDK directory. They are excluded from
acceptance; the final clean-source lint and actual pinned-Go commands pass.
Native six-target CI, Windows paired measurements, complete pipeline parity and
daemon #772 still gate #773 closure/default cutover. No GitHub write is part of
this handover, and the author cannot independently approve this correction.
