# Independent full review: state-key startup ownership, source 3db69a
Decision: REQUEST CHANGES — one P2 correctness finding.

Reviewed immutable source 3db69a655e43ef8cffa3776ad00a823d947ff31b and evidence-only publication 3c2d731b2b113df81db3601b73f3036c8a558e5c. The complete integrated Registry, state-key authentication/cleanup, Core provider resolution, scoped supervisor lifetime ownership, Windows job ownership, CLI, and MCP changes were reviewed against main e3dbda6 and previously independently approved Registry parents. No candidate sources were edited.

## P2: preserve native path bytes in the new startup-owner API
At browse/crates/symbrowse-core/src/key_sources/startup_owner.rs:40, lookup builds OsString arguments and immediately converts every argument with to_str(). A valid Unix absolute provider path containing byte FF is rejected as “startup provider argument is not UTF-8”, before its provider starts. This breaks the public SystemKeySources::with_programs(PathBuf).with_startup_owner(...) composition. The same actual provider executable succeeds through the CLI's owned internal route; ASCII and Unicode path controls succeed through both routes.

The actual public probe SHA is 9939df8439b94e2c73953d0d107d50779d9f9885165ffcbc8dce13641b72bb73; actual CLI SHA ecb9bcefb4fa1c8bf208ab5a08f8ea64c513b28be0a16c83533806e186f69707; actual original owned provider SHA e231f7b6707a4cff1714057f00c67ec454d5e2aa696dee9a217c2e7ece9062d1. Three real paired path observations, exact argv/streams/status/provider-invocation witnesses are /tmp/symaira-state-key-3db-independent-paths.json and its Python/log. This is a new public API restriction, not a claim that default Go CLI PATH lookup regressed.

Required correction: preserve OsStr/OsString through the private supervisor command boundary; keep public provider references and existing standalone GoVault command semantics intact. Add permanent actual raw-path and ASCII/Unicode positive controls and a real regression mutation.

## Fresh independent evidence
- 31 actual Go/native key-startup pairs, including the original 17 inputs and frozen encrypted v1/v2/v3 files, plus 3 actual negative controls.
- 8 actual ownership cases, 9 Unix refusals, 5 Core boundaries and missing-provider behavior; held-writer and false-absence controls reject.
- 63 real daemon requests and 3 actual controls.
- 510 Registry CLI observations, 20 raw frames, 8 concurrent clients, 7 roots, 3 actual Go API persisted cases, and 8 actual controls.
- 13 actual literal MCP byte pairs and a correctly harness-owned full workspace run: 310 passed, 0 failed, 3 existing ignores, 67 summaries; 3 actual Cargo101 controls reject.
- Affected all-feature Core/protocol/daemon/CLI plus MCP library rerun: 224 passed, 0 failed, 3 existing ignores, 34 summaries. This overlaps the full workspace and is not added to it.
- Strict workspace all-target/all-feature Clippy, workspace formatting, actionlint; 8 portable pipe tests; matrix 88 contracts/17 work items/acyclic DAG all pass.
- Candidate192 and frozen-Go670 file maps independently verified against actual current bytes and immutable Git objects;190 changed-file hashes verified;32 original final author evidence files unchanged. All149 original archived ELF paths/115 unique binaries roundtrip-verified. Every current ELF, including test artifacts, verified/archived before cache retirement, with a separate successor map.

The first direct affected run failed four MCP raw-frame tests because it omitted the harness-generated Go output fixtures and endpoint; that original101 log is retained. The next direct subset attempt exposed this environment's unrelated /usr/bin/go executable (“Go: Unknown option: build”); its original101 log is retained. The successful pinned-Go rerun is separately recorded. Neither mistake changed production or weakened assertions. The initial provenance verifier attempted a missing report-level Go-ref field; original script/log are retained and the successful version binds the known immutable Go ref explicitly.

Prior exact 30-second provider cancellation proof remains retained: both clients exit at about5 seconds and providers are gone at16.5 seconds; no instantaneous cleanup claim. Scoped lifetime cancellation, keyed v3 authenticated cleanup and failed-supervisor classification remain verified. Standalone Core provider body remains byte-identical SHA a8507eca1c5c1dc4ab6767c6106be8fed5a28fe32b8f6c27fa4e925ef94bc029. Windows process-wrap10.0.1 is pinned/target-scoped; source cross checks do not establish native Windows behavior.

Current published c31 Windows Registry runs were cancelled after already-passing process/lifecycle stages. The cause remains unproved. Add bounded incremental stage/case receipts in a separately described harness correction; do not enlarge timeout, skip tests or claim a native fix. macOS/Windows six-platform CI, full772/release acceptance, root-prefix session parser gap and documented keyless overwrite compatibility remain open.

## Preservation and target release
/tmp/symaira-state-key-3db-root-verification.json records full byte verification, current ELF SHA/archive references, zero users and retirement. Only the exclusively owned /workspace/symaira-daemon772-registry/target cache was removed after verification to recover2.2GB; original Brain target and source/history/proofs remain intact. Original author receipt is immutable; the independent successor map records any newly compiled artifact. All root raw logs/JSON/scripts and this report must be carried into the next evidence-only successor unchanged, before its build. A new source checkpoint, complete fresh gates, independent review and native proof are required before publishing.

