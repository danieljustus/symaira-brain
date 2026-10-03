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
are native as well; activity search/get/status, including flag and window
validation, run natively. The ACT-CLI acceptance runner checks the original five
report cases plus 173 portable validation cases and twelve raw Unix argv cases
against real immutable Go processes. Three actual fixture mutation controls
reject missing files, changed exits and incomplete case sets; exact-head native
Linux/macOS/Windows CI is required before merge. Importer and activity-store extensions remain in #761.
Dynamic memory configuration and unported memory commands retain their
existing Go fallback. `profile remove` is now native as well: it uses the harness registry
for binding checks, fails closed on unsafe/unreadable/malformed configs unless
`--force` is explicitly supplied, and retains capability-rooted profile parents
through no-follow removal. The Go implementation remains the source-bound oracle
for this seam until all release gates pass.

The native memory embedding adapter uses CoreKit's Rust `symaira-core-llm`
transport at the exact Git revision pinned in `Cargo.toml`. Brain retains its
Ollama URL normalization, two-second timeout, 768-dimension gate, and local hash
fallback; CoreKit owns the OpenAI-compatible HTTP request and response decoding.
CoreKit's one-result-per-input validation also matches the Go `llmkit` behavior.
This does not move memory-specific embedding policy or the Go cutover gate.
The adoption allows only CoreKit's repository in the dependency source policy.
Audit argument decoding explicitly preserves Go's float64 number semantics
under the shared JSON feature graph: negative zero, numeric overflow rejection
including overwritten duplicate keys, and ordinary object keys that resemble
serde_json's internal number marker. The existing JSON nesting bound remains.

`vault create`, `vault set`, and `vault delete` now execute natively, with
secrets sent only over child stdin. The remaining vault verbs retain opaque
argv, inherited stdio, and child exit/signal semantics. Discovery includes the
merged global/project configuration, typed environment overrides, managed
binaries, and PATH. Invalid configuration discards the override exactly as
Go does. Confirmation returns metadata only; `set` compares the requested
value without printing it, and `delete` requires symvault's not-found exit 2.
The shared Go-compatible field-value decoder retains the existing native JSON
nesting bound and fails closed when it is exceeded.

For #766, `scripts/vault-admin-oracle/replay.py` builds the complete immutable
Go archive at `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` and compares actual
process stdout, stderr, exit codes, child argv and stdin in disposable roots.
The Linux candidate passes 197 comparisons, including raw invalid UTF-8
path bytes and valid U+FFFD controls. Native process tests preserve the original
126 portable Go contracts and add 18 Go-only Unicode records (15 Unix-only).
Native Linux/macOS CI runs all 197 live cases and Windows runs its 182 cases; acceptance and issue closure require all three
jobs to succeed at the candidate head. No operator credentials are used, no Go
source is edited, and no broad Rust cutover or release is claimed.

For #620, the native usage request layer preserves all five Kimi CLI identity
headers and explicit Copilot enterprise authorities. Kimi's `1.26.7` value is
the frozen Go SDK identity, not an OS-version claim. Windows queries the same
physical DNS hostname API as Go instead of trusting `COMPUTERNAME`.
The supplemental harness in `scripts/usage-fetch-oracle` leaves existing Go
source and fixtures unchanged. Linux passes 133 complete reports, their request
walks, and 266 byte comparisons through the real JSON/table renderers across
all ten providers. Four actual negative controls fail as expected; 358 affected
Rust tests, formatting and strict Clippy pass. Native Linux/macOS/Windows CI
must still succeed at the published head before acceptance. The conservative
credential-source fallback restrictions documented in `usage_cli.rs` remain;
this does not complete the broader #768 cutover.

The native MCP foundation now models JSON-RPC null/omission semantics and
accepts both newline-delimited and `Content-Length` framing with the Go
implementation's 1 MiB bounds. Gateway dispatch is implemented natively;
Go remains the source-bound oracle and rollback path.
Go corekit v0.17.0 remains the framing oracle for ordinary cases. Rust
intentionally hardens two known upstream defects tracked as
[symaira-corekit#225](https://github.com/danieljustus/symaira-corekit/issues/225)
and [#226](https://github.com/danieljustus/symaira-corekit/issues/226): aggregate
framed-header byte/line limits, and JSON-RPC envelope validation for `-32600`.
Rust also flushes complete framed responses so buffered writers expose them
immediately. These hardening boundaries do not claim that Rust matches the
pinned Go defects. The current MCP/GW matrix rows are `green-native`; see
the [contract matrix](migration/contract-matrix.csv) for per-contract
evidence and remaining platform gates. `notifications/cancelled` is
still treated as a silent notification; no per-request cancellation behavior
is claimed because Go does not implement it. Connection cancellation does
propagate through concurrent gateway dispatch into real broker calls. Both
protocol decoders have
nightly libFuzzer targets seeded from the checked Go oracle corpus;
`make rust-fuzz-smoke` runs bounded local campaigns without mutating the
tracked seeds.

The native broker now owns child spawn, initialize, tools/list, tools/call,
crash detection with backoff restart, and graceful shutdown. Native gateway
dispatch uses this broker; Go remains the oracle and rollback path.
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
Windows probes retain Go's original-file existence check and PATHEXT candidate
selection instead of executing an extensionless PE directly. Release downloads
avoid reusing HTTP/1.0 connections closed after a missing-primary response.
The setup repair acceptance runner compares 151 real Go/Rust processes and full
fixture filesystems with no Go fallback. Source builds, module flags and doctor
repair remain open in #765; all three native CI receipts are required before
merging this repair increment.

Rust skill input hardening (#476) checks control-file and normalized frontmatter
bounds before allocating the full document or parsing YAML. Resource traversal
is iterative and rejects excessive depth, entry count, aggregate bytes, growth
since inventory, and special files. Source reads share an actual-I/O budget
through loading, hashing and materialization, including rejected headers and
control documents. Bulk runner, status, sync and MCP-list operations retain
that budget across bundles and targets. Growth at a read boundary is checked
on the same open handle without an over-budget probe byte. Library rejection
categories are typed rather than inferred from attacker-controlled diagnostics;
installed markers use bounded regular-file reads.
Classified input rejections stay native even with a library-directory override,
so they cannot re-enter the legacy Go reader. Other unsupported CLI shapes still
retain their existing fallback until #764 is complete.

Rust resource rendering (#490) accepts both relative and absolute directory
links confined to the trusted bundle. Absolute link text is mapped to a
root-relative path using the original trusted-root spellings; it is never
reopened with ambient authority. Bootstrap captures the canonical root, the
caller-supplied root, and a bounded trusted-root link chain including raw link
text. That preserves ancestor-alias spellings lost during canonicalization.
Those ambient metadata reads are limited to trusted-root bootstrap; resource
targets and subsequent reads never use them as ambient paths.
Resolution follows links through the retained
directory capability with finite depth/link limits. Inventory, Markdown reads,
hashing and copy use the same resolver and retain the shared actual-read budget.
Outside targets, parent traversal, link cycles and outside-target replacements
remain rejected; replacing the ambient root path cannot redirect the retained
source handle. This is a separate Rust-only contract deviation (SKL-006), with
native Windows link evidence pending rather than implied by Unix tests.
Windows ordinary and extended drive/UNC prefixes are equivalent only for
lexical root comparison; device namespaces remain distinct and no ambient
resource access is added. Cap-std's Windows directory handles deliberately
exclude `FILE_SHARE_DELETE`, so a live source-root rename is refused with a
sharing violation rather than permitted as on Unix. The regression asserts
that refusal and continued same-source hashing/copying; handle protections
are not loosened to make the Unix replacement scenario work on Windows.

The Go oracle and source fixtures remain frozen. These are documented Rust-only
security deviations, not a claim that the vulnerable Go paths have changed.
SKL-001 remains fixture-ready until the corrected contracts have current native
three-OS evidence. Local Rust tests pass, but the complete local CLI differential
currently stops during Go managed-install fixture setup on the external macOS
volume (`os.OpenRoot` through `/dev/fd/<fd>`); that is not a successful parity run.

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
