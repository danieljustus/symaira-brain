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
report cases plus 237 portable validation cases and twenty-eight raw Unix argv cases
against real immutable Go processes. Three actual fixture mutation controls
reject missing files, changed exits and incomplete case sets; exact-head native
Linux/macOS/Windows CI is required before merge. Importer and activity-store extensions remain in #761.
Dynamic memory configuration and unported memory commands retain their
existing Go fallback. `profile remove` is now native as well: it uses the harness registry
for binding checks, fails closed on unsafe/unreadable/malformed configs unless
`--force` is explicitly supplied, and retains capability-rooted profile parents
through no-follow removal. The Go implementation remains the source-bound oracle
for this seam until all release gates pass.

The #758 memory increment adds Brain-owned grounded evidence to
`symbrain-memory`: source refs, UTF-8 byte spans, exact/normalized/fuzzy
alignment, strict validation, Go-compatible JSONL encoding, evidence persistence
and transaction-aware reparenting. The additive oracle executes frozen Go
CoreKit v0.17.0 for 32 alignment pairs, 48 validation boundaries, three JSONL
records and four existing production database tests. Actual schema inspection
continues to repair #649's five missing columns despite applied migration names;
DDL, repairs, indexes and bookkeeping now commit atomically. The native
three-OS workflow is required before accepting this increment. Full memory CLI,
dynamic configuration, governed writes and JSONL decoding remain open; no Go
fallback route is removed, and #649 remains pending the shipped Rust release.
The rationale is recorded in `docs/adr/758-native-memory-evidence.md`.

The isolated #649 historical-repair successor preserves the complete original
314-file census and proposes all 37 ordered migration effects, including rule
timestamps, relation UUIDs/nullable intervals, attribution/target columns,
unique indexes, FTS porter rebuilds and sync-exclusion triggers. The source
draft checks actual postconditions while retaining the approved IMMEDIATE
transaction and concurrency reservation. Known native false-completion repairs
are explicit; unknown custom FTS/trigger/index definitions fail with rollback,
and unrelated legitimate NULLs remain unchanged. The new 37-prefix process
gate and focused state/rollback tests are prepared but **not executed** at this
checkpoint. Target/compiler/runtime allocation, independent review, native
three-OS CI and the shipped Rust release remain pending. The decision is in
`docs/adr/memory-historical-migration-repair-649.md`.

The focused Store-open successor preserves the actual PR811 Windows BUSY
failure and bounds only pre-migration WAL plain-BUSY retries to the remaining
five-second budget. BEGIN IMMEDIATE, migration rollback and all 80 existing
concurrent-open assertions stay unchanged. At combined source `4c62355`, 40
Memory library tests and strict checks pass, including actual reserved-writer,
reader and mixed-lock controls with a rejected compiled timeout-reset mutant.
The original Windows failure's internal phase, independent review, full CLI
gates and exact-head native three-OS verification remain pending; this does not
complete historical repair or close #649/#758. See
`docs/adr/memory-wal-open-race-803.md` and the preserved `memory-open-race-803`
evidence.

The isolated historical/default/WAL integration normally merges main 5e and the
independently approved 2fe WAL prerequisite. At source `1fc5910`, the full
allocated Linux driver passes 392 tests with no failures/ignores, all 28
historical cases, seven unchanged WAL cases, strict Clippy/fmt/actionlint,
37 actual frozen-Go/native constructor pairs, 37 whole-state native reopens,
nine actual repair/rollback controls and the 184-column SQL default inventory.
Inherited gates also pass: 590 reads/two 32-case mutation controls,
60 Set/16 Delete/13 fallback/10 output cases/three process mutants, and the
32/48/3 evidence corpus/four production Go DB tests/three negative controls.
Original failed 252 fixtures, the later incompatible handwritten integration
fixture and first strict lint run are preserved with actual executed binaries
before precise fixture/style corrections. All 37 SQL resources and approved
WAL behavior remain unchanged; no lint or callback/state assertion is weakened.
The complete 23-file default static review and Root WAL prerequisite review
remain separate lineage. Author Linux validation is not independent acceptance:
full different-author runtime review, native three-OS CI and the shipped Rust
release remain pending, and #649/#758 stay open. Decisions and exact proof
bindings are in `docs/adr/memory-historical-wal-integration-649.md` and
`migration/evidence/memory-historical-649/final-1fc5910`.

The follow-up #758 CLI increment splits the oversized memory module into focused
behavior modules, ports raw Go flags/error grammar and all 86 typed memory
configuration fields, uses configured Ollama query embeddings and corrects the
list default to 100. The supplemental immutable-Go replay compares 590 Unix/553
Windows cases, including full seeded database state for configuration/read
commands and two executable failure controls. Native three-OS acceptance is
pending. Governed writes, configured Hamming prefilter, every database open/error
shape, new-file permission parity and JSONL decoding remain open. The decision
and scope are recorded in `docs/adr/memory-native-cli-758.md`.
The bounded governed-write successor adds native provenance/trust/policy,
configured embeddings and binary storage, entity linking/audits, kind/staged
updates and delete access-feedback/audits. Existing-store direct writes are native
only when the full reachable redaction/extraction/conflict pipeline is bypassed:
staged writes skip conflicts; other writes require effective config false.
The prior unconditional default-conflict insert now delegates Go. Malformed or
unproven hydration and new-file modes also retain Go. Stateful process proof
compares application rows with explicit UUID/timestamp bindings and logical FTS
integrity, with executable identity/audit/exit mutants and actual delegation
observations. Full #758 and native three-OS acceptance remain pending; the
rationale and exact boundaries are in
`docs/adr/memory-native-governed-writes-758.md`.
Clean source `14d2414` passes Linux 60 native set / 16 native delete pairs,
13 literal delegated-Go boundaries, three write controls, all 590 baseline pairs
and two baseline controls, plus 161 affected Rust tests and strict lint gates.
Tracked receipts retain the actual delete-audit sequencing regression and
earlier rejected harness assumptions. Independent review requested changes for
zero-row governance updates and failed output on newly native metadata Set.
The correction checks both governance row counts and Set writer errors while
preserving Go's committed failure state; a supplemental real-process gate adds
six callback pairs and two Unix sink pairs where available. Original findings
and reviewed executable hashes remain retained. Corrected independent review
and native three-OS acceptance remain pending.
Clean corrected source `647477f` passes fresh Linux 60 Set/16 Delete/13 actual
Go boundary pairs, six governance callback pairs, two real Unix output sinks,
all three write controls, 590 baseline pairs and both baseline controls. All
163 affected Rust tests and strict lint gates pass. The old independent failure
bytes/executable archives and new full state proof remain tracked separately.
The subsequent independent review closed both original findings and requested
actual Unix Set stdout SIGPIPE parity. The focused successor keeps library
writers as checked errors, adds both real closed-reader process formats and an
actual CLI child test, and normally integrates main `e3dbda6c`. Its decision is
in `docs/adr/memory-set-stdout-sigpipe-758.md`; exact-source independent review,
native three-OS acceptance and full #758 remain pending.
Clean combined source `458e8fa5` passes 165 affected Rust tests and strict lint
gates, fresh 60/16/13 state pairs, six callback and four Unix sink/closed-reader
pairs, all three write controls, all 590 baseline pairs and both baseline
controls. Both original independent closed-reader inputs now match literal Go
SIGPIPE/quiet output while retaining complete committed state. The full fresh
source/binary-bound evidence and previous executable archives remain separate;
these Linux author results do not replace independent or native three-OS review.

Independent review found two configured-read differences in the initial CLI
candidate: memory-specific XDG/legacy resolution and populated rules JSON HTML/
JavaScript-separator escaping. Corrections remain memory-local, preserve the
original failed process reports and add meaningful directory/file and populated
rules cases to the replay. Other domains' data-path convention stays unchanged.

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
source handle. This is a separate Rust-only contract deviation (SKL-006).
Exact-head run `37068639832` at `682177d99816357716a3f0387733d22982a88b80`
passed all eleven acceptance checks, including native Linux/macOS/Windows
init/link tests. The Windows log confirms all ten cases, including root/ancestor
aliases and protected rename refusal. Independent full-layer review found no
code defects; its sole missing-Windows-evidence condition was resolved from that
actual log, not relabeled as reviewer approval. PR #792 was regularly squash-merged
as `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`; #490 is closed.
Windows ordinary and extended drive/UNC prefixes are equivalent only for
lexical root comparison; device namespaces remain distinct and no ambient
resource access is added. Cap-std's Windows directory handles deliberately
exclude `FILE_SHARE_DELETE`, so a live source-root rename is refused with a
sharing violation rather than permitted as on Unix. The regression asserts
that refusal and continued same-source hashing/copying; handle protections
are not loosened to make the Unix replacement scenario work on Windows.

Rust-only native preflight correction #793 closes two additional bypasses found
while scoping #764. Native `skills sync` validates missing values, unknown flags,
Go boolean spellings, target/scope, help and positional termination before any
filesystem work, reusing the existing Go-compatible quoter and whole-vector
normalizer. Raw argument values survive until validation, including Unix non-UTF-8
bytes and separated flag-like values. Target validation trims Go Unicode
whitespace at valid UTF-8 boundaries while preserving invalid interior bytes;
invalid scope diagnostics retain their original padding. All 198 isolated flag
probes (including both previously failing padded targets and Unicode/control
boundaries) match stdout, stderr and exit code against the actual frozen Go binary
built from a complete archive of `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`.
OpenCode fallback eligibility reuses the common no-follow marker reader, which
now uses the existing regular-file, same-handle, size/growth-bounded reader and
its nonblocking Unix final open. Typed `MarkerState::Rejected` keeps unsafe
marker inputs native even when an ordinary malformed marker or dynamic config
would otherwise select Go. Ordinary malformed/future-schema fallback remains.
Marker directory probing uses the existing-only no-follow opener; missing or
concurrently renamed directories remain absent without status creating paths.
Marker reads retain the existing per-file `MAX_INPUT_SIZE`; this slice does not
claim a new aggregate marker budget or complete native config/all-target status
coverage. Parent #476/#764 stay open and #621 no-follow/product policy is unchanged.
Bounded child-process regressions and the complete affected three-package suite
pass on Linux x86_64: 421 top-level tests, zero failures, two ignored child
entrypoints. Formatting and strict all-target Clippy pass. The cloud checkout
uses standard 0644/0755 tracked-file permissions and umask 022; a subreaper
reaps adopted test children because the container PID 1 is not a reaping init.
Fresh native three-OS CI and independent full-layer review remain required.
Earlier macOS and Windows cross-build records describe their original code heads.

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

### Skills sync Windows raw argv follow-up (#793 / #794)

Source preparation corrects the earlier Windows replacement assumption: pinned
Go 1.26.7 preserves unpaired UTF-16 surrogates as WTF-8. Skills sync now keeps
its normalized argv as raw Go byte vectors and uses the existing byte quote
formatter. The original 315 cases, 459-test checkpoint and native CI proofs
remain historical acceptance of their exact source. New native Windows wide
CreateProcessW comparisons, unchanged Unix315 regression, affected tests/strict
checks and independent review are pending; copied SDK source is no Windows
runtime evidence. This does not complete broader Skills/Rust cutover.
