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
