# ADR 0011: Restore standalone Guard through a shared Guard command crate

Status: scoped implementation decision for #770; full migration acceptance remains open.

## Context

The reachable Go source has five public command packages under `guard/cmd/symguard/` and dispatches them through `cmd/symbrain/cmd_guard.go`: version, doctor, decide, grants (list/revoke), and scan. There is no Go standalone main package in the frozen revision. Proxy/spawn, approval, proposal, sequence and update are library/planned functionality, not additional reachable CLI verbs. Older documentation describing the standalone retirement and broader issue prose do not change this actual inventory.

The Rust command handlers were located in `symbrain-cli` even though their policies, discovery, grant storage and audit belong to Guard. Making an independently usable security tool depend on the entire Brain CLI would also link memory, gateway and broker dependencies into its presentation layer.

## Decision and rationale

Move the existing Guard handlers into `symguard-cli`, with a native `symguard` entrypoint and a small Brain compatibility adapter. Both entrypoints call the same Guard-owned code. Reuse the pure Guard policy/audit kernel and the existing capability-safe raw JSONL appender. No Brain exposure policy, MCP gateway, approval authority, master key or legacy child executor enters the standalone dependency graph.

Preserve the shipped Go command streams, schemas, exit codes and persistence semantics for the proven states. Standalone top-level help uses its own command spelling. Toolchain lines report Rust honestly; they are validated explicit platform/runtime differences. Actual process comparisons exposed an inherited Guard version JSON defect: Rust pretty-printed the payload, while Go emits compact JSON and two newlines. The shared version handler now preserves those actual bytes and propagates output failures.

Keep unsupported doctor diagnostic states explicit and fail closed with exit 1 before a healthy-looking report or fallback execution. Do not hide the unresolved states or call the issue complete: complex TOML/type/validation and audit-anchor shape diagnostics, discovery error contracts, the inherited audit error diagnostic difference, and complete platform/output-boundary acceptance still require work. The Brain adapter keeps its existing Go fallback for those doctor states during migration. Existing audit append capability and no-follow restrictions are retained rather than weakened for Go diagnostic spelling.

## Verification and consequences

The supplemental Go process entrypoint imports unchanged public command packages from a complete archive of frozen revision `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`. It is a `.go.txt` oracle input, never production Go or a rewritten frozen fixture. Disposable HOME/XDG roots contain only synthetic configuration, grants and decision requests. Process receipts retain exact stdout/stderr/exit and file modes/content, source and binary hashes. Dynamic audit IDs/times are validated against real execution windows before replacing only those fields for comparison; raw bytes remain retained.

The initially published scoped runner executes80 cases: 76 exact matches after owned-path and truthful toolchain handling, three explicitly unported doctor states checked fail closed, and one retained audit failure diagnostic deviation. These are scoped process contracts, not zero-unported acceptance. Native Linux/macOS/Windows CI must run the new named gate and retain the receipt. The standalone binary is buildable from source; release/signing/distribution and issue closure are not claimed by this ADR.

The product decision restores independent Guard usability while preserving the Brain/Guard and Room/Guard boundaries. It supersedes the Go-only standalone-retirement assumption for this Rust entrypoint, without inventing currently unshipped CLI commands or retiring future Guard library capabilities.

After Activity PR800, normal-integrate main31de722 and retain both complete
Guard and Activity acceptance/artifact blocks on all three OS jobs. Repeat
the actual80-case Guard process gate, component tests and Brain adapter
contracts; preserve the3 unported Doctor states and1 inherited diagnostic
deviation. This verifies the current CLI combination without converting
partial #770 evidence into completion. Fresh native CI still gates merge.

## Native Windows temporary-root correction

Current-main head3cc1046 reached464 ordinary Go/native CLI comparisons on Windows,
then failed before the standalone Guard comparison: `RUNNER_TEMP` was a native
Windows path and tar could not open the resulting Go source directory. Convert
only the owned Bash/Tar temporary root with `cygpath -u` before extraction;
retain the existing native executable path conversion. The original complete
job111289182217 log is tracked. Frozen Go, production Guard and all80 cases are
unchanged. Fresh local process proof and actual native Windows CI remain required;
this path correction does not establish a platform result by itself.

## Native semantic validation and typed audit diagnostics

Normally integrate approved published Guard2f50 before the diagnostics successor.
Preserve the earlier dirty4a9 semantic-validation checkpoint: exact five source
snapshots,94-case/89-match report and21-test/strict logs remain SHA-mapped under
`migration/evidence/guard-diagnostics-770/original-wip-4a9`. It is historical WIP,
not clean acceptance. Frozen production Go and all original fixtures remain exact.

Decode every known TOML field before semantic validation, including unprinted
proxy/audit/remote fields. A library decoder error must still precede an invalid
policy value. Native validation preserves Go's ordering for a single invalid
map entry, rules, sequence threshold (explicit zero resolves to three), then
spawn paths. Unknown keys/type diagnostics and several invalid defaults keep
an explicit unsupported-state gate; Go's randomized map order is not fabricated
or replaced with an unapproved equality exception. Inline/alternate TOML shapes
and general I/O/discovery failures also remain incomplete.

Audit-anchor inspection validates syntax with the shared Go scanner, then
preserves object fields in wire order as raw values. This preserves the first
typed error even if a later duplicate would overwrite it; null scalar fields
are accepted, names use Go ASCII/long-s/Kelvin folding, unknown numeric values
are ignored without float64 overflow, and integer errors retain their original
literal. SchemaVersion follows the target Go int width, not an arbitrary int32.
Unsupported Unicode replacement remains gated rather than falsely healthy.
Thirty actual process cases cover these boundaries. The first prototype's
number-to-string error included an extra literal; its real failure is retained
and the correction matches Go's distinct string/integer diagnostics.

Preserve the Unix raw EISDIR diagnostic at the Guard presentation boundary:
`decide: open audit log: open <path>: is a directory`. The underlying capability
appender, private modes, retained directory handles, no-follow checks, nonblocking
open and short-write behavior are unchanged. There is no ambient stat/reopen to
infer a cause after failure. All other syscall/capability messages remain honest;
Windows stage/error spelling remains unported rather than falsely translated.
The actual failed audit still produces deny and no decision record.

The expanded selected process proof is124 cases: current Linux121 full byte,
exit and filesystem matches, three explicit TOML gates, zero selected audit
wording deviations. Windows is expected to retain its separately asserted
wording difference, but native three-OS CI has not run for this new source.
Three real native-output mutants must be rejected: hide the config error, hide
an anchor type error, or allow an otherwise permitted decision without audit.
Do not weaken or remove any original ordinary equality assertion. Earlier WIP,
initial prototype failures and fresh clean-source full reports remain retained.
These selected counts do not close#770/#769 or claim all diagnostic shapes.
