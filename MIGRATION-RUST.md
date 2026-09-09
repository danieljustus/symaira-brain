# Go → Rust migration

The migration is deliberately incremental. Go remains the executable oracle
until every contract row is green. Every oracle command is built from the
immutable revision selected by `GO_ORACLE_REF` (default: the checked-out
`HEAD` resolved to a commit) through a temporary `git archive`; dirty source
files cannot silently become the reference behavior.
The Rust binary already owns top-level help, `version`, all `config` actions,
and `profile help|list|show|add` plus `audit tail`; the native core also owns
profile policy, the deterministic merged tool catalog, and the redacting
hash-chained audit store. Pattern promotion and bounded activity-read contracts
are native as well; DB-backed activity commands still delegate until the memory
SQLite phase. `profile remove` is now native as well: it uses the harness registry
for binding checks, fails closed on unsafe/unreadable/malformed configs unless
`--force` is explicitly supplied, and retains capability-rooted profile parents
through no-follow removal. The Go implementation remains the source-bound oracle
for this seam until all release gates pass.
The native MCP foundation now models JSON-RPC null/omission semantics and
accepts both newline-delimited and `Content-Length` framing with the Go
implementation's 1 MiB bounds. Gateway dispatch remains on Go until Phase 5.
Both protocol decoders have nightly libFuzzer targets seeded from the checked
Go corekit v0.16.2 remains the framing oracle for ordinary cases. Rust
intentionally hardens two known upstream defects tracked as
[symaira-corekit#225](https://github.com/danieljustus/symaira-corekit/issues/225)
and [#226](https://github.com/danieljustus/symaira-corekit/issues/226): aggregate
framed-header byte/line limits, and JSON-RPC envelope validation for `-32600`.
Rust also flushes complete framed responses so buffered writers expose them
immediately. These are an intentional pending parity boundary, not a claim
that Rust matches the pinned Go defect. MCP/GW matrix rows stay
`fixture-ready` until corekit is corrected, released, bumped here, and the
P5-3 CLI executable fixtures are regenerated. `notifications/cancelled` is
still treated as a silent notification; no per-request cancellation behavior
is claimed because Go does not implement it. Connection cancellation does
propagate through concurrent gateway dispatch into real broker calls. Both
protocol decoders have
nightly libFuzzer targets seeded from the checked Go oracle corpus;
`make rust-fuzz-smoke` runs bounded local campaigns without mutating the
tracked seeds.

The native broker now owns child spawn, initialize, tools/list, tools/call,
crash detection with backoff restart, and graceful shutdown. Gateway dispatch
remains on Go until Phase 5.
Its integration suite uses a Rust fake MCP child and compares lifecycle results
against the production Go broker oracle, including concurrent lazy start,
timeouts, protocol mismatch, restart counts, and descendant process cleanup.

Phase 6 instruction Slice 0+1 is frozen in `scripts/instructions-oracle` and
implemented by `rust/symbrain-instructions`, split into block rendering, source
resolution/reads, and capability-rooted path walks. The fixture records the pinned
Go rollback baseline, current and rollback SHA-256 provenance for instruction
sources and five managed-block goldens, a generator digest, isolated source
inputs, a true prefix-code escape sentinel with begin/end and literal-sentinel
collision cases, and 32 deterministic render/source cases. Negative source
verdicts compare exact normalized diagnostic bytes between Go and Rust, including
the `instructions: read <path>:` wrapper; only isolated temporary roots are
normalized and ordinary diagnostic redaction remains unchanged. Both languages
now retain capability-rooted source parents, reject symlinked project/XDG/
home parents and trusted roots, use bounded regular-file/no-follow reads with
nonblocking Unix final opens, and preserve byte-identical managed blocks. Go
and Rust atomic replacement preserves complete mode bits and readable extended
attributes (including POSIX ACL xattrs); macOS `com.apple.provenance` is an
OS-owned xattr that cannot be transferred and is explicitly excluded. Windows
replacement copies owner/group/DACL security data without requesting
`ACCESS_SYSTEM_SECURITY`; SACLs are excluded unless a future explicit
privileged mode enables `SeSecurityPrivilege`. Windows directory-entry flush is
best-effort where the filesystem rejects directory handles: file data is flushed
before rename and unsupported post-rename directory flushes do not report a
committed mutation as rollback-able failure. Rust's Windows source and adapter
walks open drive/UNC anchors once and then traverse every caller-controlled
component with reparse-point no-follow handles.

Phase 6.2 adapters are implemented in `rust/symbrain-adapter`, split into target
selection/rendering, capability-backed atomic I/O, and target-path validation.
Adapter selection is derived from the nine-entry `symbrain-harness` registry: only
Claude, Cursor, Antigravity, and Agents render instruction targets; the other
five registered harnesses are explicit skips. The source-bound adapter oracle
covers fresh output, user prefix/suffix preservation, malformed and absent
markers, CRLF, non-UTF-8, marker-bearing content, target paths, and provenance.
The adapter core rejects traversal and absolute targets, retains root/parent
capabilities, rejects root symlink/reparse components, uses nonblocking final
reads, and provides durable atomic writes with Unix mode/xattr/POSIX-ACL
preservation and Windows owner/group/DACL preservation. Windows SACL copying is
not requested for ordinary writes and remains an explicit privileged boundary.
CLI wiring and skills sync remain later slices.

Native install/uninstall now retains one capability-rooted directory handle
from trusted-root resolution through config reads, collision-safe backup and
temporary creation, replacement, rollback, cleanup, and directory sync. Root
components are opened without following symlink/reparse ancestors; configuration
input, JSON values/strings, and generated output are bounded. Windows replacement
uses a preserved rollback name rather than deleting the original before the new
file is installed. Native Windows runtime remains unclaimed; the matrix records
Windows target compilation only.

Managed release metadata, asset naming, SHA-256 verification, secure tar.gz/zip
binary selection, and atomic executable installation are native Rust contracts.
The Go oracle now rejects traversal, absolute, link, duplicate, and ambiguous
archive candidates before the Rust fixture is generated.

`setup` and `setup --fix` now run natively against the embedded manifest. The
Rust installer downloads pinned-tag assets over HTTPS, prefers manifest-pinned
checksums, falls back to release checksum files, verifies Cosign publishers by
default, supports the explicit `--allow-unsigned` escape hatch, and probes
installed versions with a bounded child process. Local immutable HTTP fixtures
compare output, JSON, installed bytes, and modes against Go without touching
live releases.
Primary assets now fall back to legacy names only on HTTP 404; checksum,
publisher-verification, and other failures stay fail-closed. Version probes
have a three-second bound and reap descendant process groups on Unix.

This is not the final cutover. It is the reversible dual-runtime seam that lets
individual capabilities move without breaking the shipped `symbrain` command.
The complete task order and acceptance gates live in
[`migration/implementation-plan.md`](migration/implementation-plan.md); the
machine-readable status is tracked in
[`migration/contract-matrix.csv`](migration/contract-matrix.csv).

## Measured baseline

- Go production and test source: 545 files in the root module plus 67 files in
  the nested guard module; 142,783 total lines.
- Packages: 93 root-module packages plus 19 guard-module packages.
- `make test`: 74.28 seconds wall time and 458,833,920 bytes maximum resident
  set on the migration host (2026-09-05, commit `4b6be3b`).
- Existing contract baseline: all Go tests passed before Rust files were added.

## Local gates

```sh
make rust-check
make parity-smoke
make rust-fuzz-smoke
make test
```

`parity-smoke` builds both implementations with the same `dev` version and
runs language-neutral black-box cases in isolated HOME/XDG directories. To
recheck a historical candidate, pass its immutable Go reference explicitly,
for example `make GO_ORACLE_REF=<commit> rust-check parity-smoke`.

## Cutover order

1. CLI framing, output selection, help, version, XDG paths.
2. Profiles, policy, catalog and audit (pure deterministic core).
3. Broker, managed setup/vault passthrough, and the MCP gateway foundation.
4. Harness registry/adapters and the skills store/render/install pipeline.
5. Guard approval/security state machines and usage providers.
6. Memory SQLite schema, migrations, CRUD, retrieval, and importers.
7. Return to the MCP cutover once native skills, usage, and memory handlers
   exist; then complete doctor, sync, release assets, and native smoke tests.
8. Remove the Go fallback only after every contract row and release gate is
   green on macOS, Linux and Windows.

The SwiftUI applications remain Swift. Their CLI JSON contracts are migration
inputs, not candidates for translation to Rust.
