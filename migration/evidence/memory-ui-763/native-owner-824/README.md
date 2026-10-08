# Bounded Memory HTTP owner: final Linux evidence

Validation source: `824a95e0bdbe6a25108e1dbadaaebf7d89dc8cc4`.
Production source: `d1f4faa77dc722da59a9cea1ff000566ce033f8f`.
The two successors strengthen complete SQLite no-write inspection and label
same-status/different-error desired divergences. All Rust, assets, Cargo and
workflow bytes are identical to the tested production source. The normal
approved Store parent is `27001b1bd6948cd7985773655f3a43bc41e947b3`, with main
`e3dbda6cbb95429237d15a9b176209b7d107792c` as ancestor.

[validation.json](validation.json) maps 459 original files to deterministic gzip
copies with original and retained SHA-256. Decompression returns original bytes;
original paths remain present. It binds 167 candidate source files, all 2,438
files in the actual frozen Go archive, primary executables, all 33 executed test
binary paths, and the separate executable archive receipts. The final executable
snapshot contains 526 ELF artifact paths / 492 unique byte sequences, compressed
and round-trip verified under `/workspace/oracles/symaira-memory763-final-binaries`.
Unchanged blobs reuse the preserved source813 archive. A target snapshot does not
claim every stale auxiliary artifact was compiled from the current source.

The final clean-source run records 52 HTTP pairs, seven native-only 501 boundaries
with every SQLite table/column/row/blob unchanged, two actual full-replay failure
controls, 18 explicit desired same-origin pairs and actual jsdom runs with 11
native UI checks plus five original-defect checks. Same-origin desired behavior
is distinct from Go parity. DOM transport explicitly models browser write Origin;
no graphical browser, layout or CSP execution is claimed.

Fresh inherited gates on this final source pass 590 read cases, 60 Set cases,
16 Delete cases, 13 delegated boundaries, ten failure/output pairs (including
Unix closed-reader/sink behavior) and three actual mutation controls. Default
port 11434 was exclusively handed over by the coordinator and released only
after all write/control processes finished; the final listener check was empty.
The 320 ordinary CLI/Memory tests, strict all-targets/all-features Clippy, fmt and
actionlint were run on production sourceD1. Their original logs and executable
mapping remain sourceD1 evidence; unchanged production bytes justify reuse.

Original setup failures, CSS path/header mismatches, WAL-copy empty-data failure,
transient DOM observation and the incorrect creation/update equality assertion
are retained alongside their later corrected runs. Source007 inherited write
receipts overlapped another lane's default-port control; those are historical,
not exclusive final proof. Source813 and final824 exclusive reruns passed.
Earlier sourceD1 no-write snapshots covered six named domain tables; final824
alone claims the stronger complete-SQLite state boundary. The original 13 CLI /
17 HTTP reachability inventory and original same-origin refusal pair remain in
their separate unchanged evidence directories.

No complete native `memory serve` admission, historical-schema compatibility,
Windows/macOS runtime acceptance, independent approval or full #763 completion
is claimed. Configuration/key resolution/rotation, unrestricted policies and
writes, sync/relay/stats and the full HTTP lifecycle remain gated in Go.

Run from the worktree with the owned target released exclusively to you:

```sh
. /home/agent/.cargo/env
export PATH=/workspace/toolchains/go1.26.7/bin:$PATH
export CARGO_TARGET_DIR=/workspace/symaira-memory763-ui/target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_BUILD_JOBS=2
cargo test -p symbrain-memory -p symbrain-cli --all-targets --all-features --offline --locked
cargo clippy -p symbrain-memory -p symbrain-cli --all-targets --all-features --offline --locked -- -D warnings
cargo fmt --all -- --check
python3 /tmp/symaira-subreaper.py python3 scripts/memory-http-oracle/run.py \
  --native "$CARGO_TARGET_DIR/debug/examples/http_probe" \
  --node /opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node \
  --jsdom /tmp/symaira-memory763-dom/node_modules/jsdom --report-dir /tmp/new-memory-http-proof
python3 /tmp/symaira-subreaper.py python3 scripts/memory-cli-oracle/replay.py \
  --go /workspace/oracles/symbrain-go-dcddcef0 --rust "$CARGO_TARGET_DIR/debug/symbrain" \
  --report /tmp/new-memory-read-proof.json
# Coordinate exclusive11434 ownership before the following command.
python3 /tmp/symaira-subreaper.py python3 scripts/memory-cli-oracle/write_gate.py \
  --go /workspace/oracles/symbrain-go-dcddcef0 --rust "$CARGO_TARGET_DIR/debug/symbrain" \
  --report-dir /tmp/new-memory-write-proof
python3 migration/evidence/memory-ui-763/native-owner-824/verify.py
```

Native three-OS acceptance is wired in `.github/workflows/memory-http-native.yml`
and remains pending actual job results. This evidence-only successor changes no
production source, comparator, case or workflow.
