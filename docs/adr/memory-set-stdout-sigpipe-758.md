# Memory Set completion on a closed Unix stdout pipe

Status: focused correction; exact-source independent review and native three-OS
acceptance remain pending. Full #758 remains open.

Independent full review of publication `55e3038`, source `647477f`, closed the
original governance row-count and full-device output findings. It confirmed a
further actual process difference: nonempty-metadata Set with a closed stdout
pipe completes persistence, then Go exits quietly through SIGPIPE while native
prints a broken-pipe diagnostic and exits one. Both JSON and table formats have
the same committed application state and healthy FTS index.

Keep Go's usual Unix pipeline termination at the actual Set stdout completion
boundary. Use the already pinned signal-hook default-handler emulation after a
real OS BrokenPipe error, after persistence has completed. Do not reset SIGPIPE
at startup: a global runtime change would also affect unrelated commands and
embedded callers. No unsafe code or new dependency is needed.

The CLI executable enters through `run_stdio`, which owns the real process
streams. Existing `run`, `run_with_executor` and `run_in_process` interfaces retain
their embedded-writer contract. The explicit stdout identity reaches only native
Memory Set completion; custom writer failures remain checked errors/exit one,
including injected BrokenPipe errors and raw OS errors on library writers. On
non-Unix platforms, checked output errors retain the previous contract. Real
`/dev/full` errors still print the exact Go diagnostic and return one. No completed
write is rolled back.

Move raw CLI flag normalization and delegated-flag stop logic into a focused
module before extending the oversized dispatcher. Its public normalization
export and behavior remain the same. All changed production files stay below
400 lines. Normally integrate the reviewed profile-publication main commit
`e3dbda6c` before validating the combined executable.

The permanent state gate adds two actual closed-reader subprocess pairs, requiring
SIGPIPE, empty stderr, complete metadata/staging/audit/sync state, validated IDs
and timestamps and real FTS integrity. The Unix Rust child test exercises the
real executable/stdout and reopens its committed staged row. Portable writer
tests prove both output formats remain ordinary errors for embedded writers;
even a custom BrokenPipe kind at the process boundary cannot trigger termination
without an OS errno. The existing full-device pairs remain in the same gate.

Preserve all 46 independent report/proof/input/log artifacts under
`migration/evidence/memory-cli-758/sigpipe-review-55e3038/`. Before reusing the
released target, archive and decompression/SHA-verify the actual reviewed CLI
`b1314acb` and five executed test binaries under
`/workspace/oracles/symaira-memory758-647-binaries/receipt.json`. The original55
source, previous14d archives and all rejected observations remain unchanged.

Clean combined source `458e8fa5` passes 165 affected Rust tests (119 CLI, 44
Memory, one integrated Activity test and one actual Unix stdout child test),
strict Clippy, formatting and workflow actionlint. Its actual executable SHA256
is `74baed15ec319a894fc5fc3001c0afb03572c0a12b2d0690b52802fe664fc804`.
Fresh actual Go/native gates pass all 60 Set, 16 Delete, 13 delegated-boundary,
six callback, two full-device and two closed-reader pairs, all three write
controls, all 590 baseline pairs and both baseline controls. The unchanged
independent closed-reader inputs also pass both formats with literal quiet
SIGPIPE and matching committed state. Full source/hash-bound logs and raw
receipts are retained separately in
`migration/evidence/memory-cli-758/sigpipe-fix-458e8fa/verification.json`.
All 163 candidate-file and 1,215 frozen Go source hashes, seven actual current
executables and all six previous reviewed executable archives were verified.
These local Linux results are author validation, pending independent approval
and actual native macOS/Windows acceptance.

This correction does not claim the inherited Delete/general output handling,
the delegated redaction/extraction/conflict pipeline, prefilter/JSONL decoding,
all database error/mode shapes or related MCP/sync/serve cutovers. Native macOS
and Windows acceptance still requires actual native execution, not type checks.

Root's complete independent review approves publication1a60/source458 for this
bounded slice. Fresh broad all-target CLI/Memory suites pass320 tests with
zero failures/ignores and32 summaries; strict Clippy/fmt/actionlint pass. All
60 Set/16 Delete/13 boundaries/10 failure pairs,590 baseline, three write and
two32-case read controls pass. Both original independent closed-reader cases
match quiet SIGPIPE and complete committed state/FTS. Verify163 current and
1215 frozenGo source hashes,36 author/46 prior proof artifacts and all actual
current/original executables. Complete independent report/raw receipts/logs
are retained under `independent-sigpipe-458/`; native current-head Windows and
macOS checks remain required. Existing delegated/full758 gates remain open.


## Main documentation integration, 2026-10-04

Main2b6d49f is normally integrated. Only documentation and retained historical evidence change; production, tests, workflows, dependencies and the contract matrix remain byte-identical to reviewed27001b1. Retain the full independent320-test/read/write/state/FTS proof and existing native runtime receipts, while requiring protected and Memory CLI/Evidence three-platform checks at the new published head. This avoids rebuilding unchanged code for a documentation merge without loosening acceptance. Full758/649 and MCP/UI/legacy ownership remain open.
