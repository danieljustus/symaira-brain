# Doctor stdout failure boundary

Status: implemented successor; independent review and native three-OS CI pending.

The fifth independent review reproduced two inherited Doctor output problems against frozen Go `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` with Go 1.26.7. A closed stdout reader let native `doctor --fix` continue probing and installing binaries after its first header failed. Non-fix Doctor also replaced concrete JSON write errors with a generic error and returned a failure for human formatting errors that Go ignores. The original source `e6e96a2` and publication `b3ad77a8`, every independent input/result, and actual binaries are preserved before target reuse.

## Decision and reasons

Use the existing process-stdout adapter for Doctor, just as Setup and Memory use it. On Unix, a real OS EPIPE on the actual stdout invokes SIGPIPE at the failing write. No global signal disposition changes, final-command signal simulation, or rollback are introduced. For an admitted native invocation, `doctor --fix` submits its header before loading enabled cores, managed probes, or repairs. Read-only fallback admission remains the existing conservative typed-config boundary; it does not emit output or perform repairs. Work already completed before a later failing write remains committed. Resolving the managed home still precedes the header, matching Go's missing-home error precedence.

Keep checked JSON completion separate from human output. JSON returns its concrete write error and exit 1. Human Doctor and repair formatting ignore ordinary write errors and continue with exit determined by diagnostics/repair, matching Go. Each human formatting operation submits one complete byte buffer; an error or short write does not trigger a suffix retry or suppress later operations. This matters for embedded writers that recover after one failure. The final repair line preserves raw path bytes in one submission.

The public library receives a caller-owned writer, not actual process stdout. Even a raw syscall EPIPE in that writer remains an ordinary callback error. Only the process entry point admits signal behavior. The adapter retains concrete operation/path/cause for actual stdout errno failures; synthetic callback messages stay unchanged.

## Evidence and limits

The new real-process fixtures use owned homes, files, publisher fixtures, absent Go fallback sentinels, bounded pipe readers, and `/dev/full`. They compare stdout/stderr/exit and complete resulting filesystem state. Additional frozen-Go/public-library callers exercise cumulative byte budgets and recoverable errors. Intended mutants must fail for their named output/signal/state field. Previous Setup/Source/Doctor, Guard, Memory and output corpora remain required; no prior assertion is weakened.

Linux OS sink observations establish Linux behavior only. Native Windows and macOS gates remain required. This correction does not complete issue #765 or the broader migration, typed configuration, or source worker cutovers.


## Full independent acceptance, 2026-10-04

Different-author review approves tested58fe50/source-neutral publicationfb6d68 for this bounded correction. Fresh501 tests (zero failed/ignored,54 summaries), strict lint and all Source111/Doctor285/Setup201, Guard124+raw63, Memory590/60Set/16Delete/13boundaries/10output, old18+24 and new13+27 output gates and actual controls pass. The original fifth-review145 proofs, all prior executable/source maps and new142 reviewer raw bindings remain preserved. Current main2b6d49f is already integrated normally. Current-head protected and native three-platform checks still precede merge.

Actual read-only fd1 file/directory probes expose a remaining inherited Doctor JSON EBADF gap: Go reports the real OS error and returns1; Rust stdio suppresses it and returns0 at both e6 and this source. A later native process-writer owner must fix that boundary; it is not waived. Public caller-owned Rust Write retries valid Interrupted errors via its standard contract, while Go io.Writer reports them; inherited attribution is retained separately. Invalid Go short-success callbacks are robustness observations, not parity findings. Human formatting documentation describes logical operation boundaries, not a guarantee that Rust Write will never retry an interrupted syscall. No full errno/writer equivalence or765 closure is claimed.

Retain executable SHA/length/source mappings and lossless archives outside Git rather than add compiled ELF payloads to source history; track complete raw reports, inputs, scripts and receipts in migration/evidence/doctor-stdout-765/independent-58fe50. Only the fully preserved, explicitly released ignored build cache was retired to make room for remaining ports. Original source, raw failures, executables and archives remain intact.
## Coverage child environment, 2026-10-04

Actual full-workspace LLVM CI at ac81b94 fails the missing-managed-home fixture's complete filesystem snapshot: its newly added direct child command clears LLVM_PROFILE_FILE, so the instrumented CLI creates a default profiler file inside the owned test root. The raw 10.8 MB job and original ZIP are preserved under migration/evidence/doctor-coverage-environment-765/original-ac81; formatted CLI log views omit the very large assertion line and cannot replace the original evidence.

Preserve only LLVM_PROFILE_FILE after the existing environment clear, as the established coverage helper and the other repair test already do. Profiling writes to the runner's existing output path; all product-owned filesystem/output/exit assertions and the complete workspace's 80% floor remain unchanged. With profiling absent the environment is unchanged. This test-only correction requires current-head CI with actual instrumentation; the prior full independent production review remains source-bound to unchanged58fe50. No profile-file exclusion, test skip, floor reduction or production change is introduced.
