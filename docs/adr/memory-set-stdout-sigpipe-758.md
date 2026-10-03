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

This correction does not claim the inherited Delete/general output handling,
the delegated redaction/extraction/conflict pipeline, prefilter/JSONL decoding,
all database error/mode shapes or related MCP/sync/serve cutovers. Native macOS
and Windows acceptance still requires actual native execution, not type checks.
