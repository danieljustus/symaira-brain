# Independent focused Windows daemon pipe-harness review

**APPROVE the focused source correction; zero blocking findings. Native Windows acceptance remains required.**

Reviewed immutable clean HEAD `c4f953af726ff7c4f3185011bbf1ab05de53eeb7` against its direct published parent `7f6ff3b8406b2d8861e4c90691919d6bdae77be6` in `/workspace/symaira-daemon772-registry`. Reviewed all 11 changed files: transport, caller, permanent tests, workflow, public ADR, binary log attributes and original retained proof. No source edits, Cargo builds, executable targets, Go processes or GitHub writes occurred.

## Source and native API contract

`CreateFileW` uses a pointer-width HANDLE result, explicit seven-argument signature, read/write access, OPEN_EXISTING and FILE_FLAG_OVERLAPPED. INVALID_HANDLE_VALUE comparison has pointer width. `WinDLL(use_last_error=True)` and immediate error capture preserve Win32 connection codes. The Go pipe listener defaults to byte mode; this client retains one newline-delimited request per connection.

All OVERLAPPED fields have the native pointer/DWORD types and default-zero initialization, including the equivalent Offset/OffsetHigh union layout. Each operation owns a manual-reset, initially-unsignaled event and retains buffer/OVERLAPPED storage through completion. Immediate success reads its actual transfer count through GetOverlappedResult. ERROR_IO_PENDING waits against the remaining request deadline; timeout, failed wait and failed completion cancel that exact operation and drain completion before freeing storage and closing the event. Cancellation racing with already-completed I/O still drains safely. The client handle closes exactly once after the request boundary. Synchronous cancellation completion is required for safe buffer release, rather than freeing potentially live overlapped storage.

Only precise ERROR_PIPE_BUSY is retried before any payload. A CRT errno22 without winerror propagates unchanged. Partial writes advance only the unsent suffix. All transfer OSError/TimeoutError failures become RuntimeError after connection, preventing wait_for_request from replaying a possibly committed mutation. Response accumulation and the first JSON line are bounded. Unix transport is unchanged. This is a bounded 3-second Windows request operation; the existing outer5-second readiness polling loop still has its existing nested-request semantics, not a newly claimed strict total wall-clock bound.

## Fresh independent verification

- 8 permanent portable pipe tests pass.
- 2 existing DaemonExit harness tests pass.
- 15 additional independent tests pass: immediate/pending I/O, partial count handling, pointer-width handles, exact signatures/open flags, event creation failure, expired deadline, pending timeout, failed/abnormal waits, completion error, cancel/completion race, drain-before-close, errno22 propagation and postconnection no-replay.
- 3 deliberately mutated helpers fail their exact selected tests with exit1: omitted cancellation drain, blanket errno22 retry and truncated CreateFileW HANDLE type. The initial broad-retry control timeout remains retained; final deterministic clock-bound control records the exact assertion failure.
-Changed workflow actionlint and complete range whitespace check pass.

These 25 tests and3 controls run on Linux with mocked Win32 calls. Linux ctypes DWORD layout is not the Windows ABI; the tests verify the actual declared native types/signatures and operation/control ordering, not a native kernel, actual pipe completion or Windows structure-size execution. Genuine AMD64/ARM64 Windows CI plus the existing six-platform exact-head gate remains required before merge. No further daemon production approval or full #772 completion is inferred.

## Original failure and provenance

Independently verified all 11 changed file hashes, all 4 retained original files and5 exact ZIP-member hashes. ZIP SHA256 `6b8e50c09f39e7174c9290b5c9cd084c0eab5015eea1795e81e91c9b20f10a62`; actual original runner source was `b19ec439a74e5bdb91a29da1051f8bd25043cdb1`, not a claim of a c4 native run. Original native Rust 41 passed plus3 explicitly owned child ignores, lifecycle1×1 and races50×50 passed.

The demonstrated AMD64 failure was Python CRT open raising errno22 without winerror. `daemon_process.observe` called kill_tree before printing process.returncode; the subsequently logged Go exit1 does not prove an original Go process failure. The original ARM64 run passed 35 actual daemon request pairs before its registry step reached the job deadline; its cancellation root cause remains unproven. Both full raw logs and downloaded binary ZIP remain exact tracked evidence with appropriate binary .gitattributes. ADR0008 accurately distinguishes these observations and the remaining native proof.

Full receipt `/tmp/symaira-registry-c4-pipe-independent-review-receipt.json` binds clean source, retained originals and all current scripts/logs/negative controls. Source remains immutable and no reviewer processes are left running; there was no target ownership or access to release.
