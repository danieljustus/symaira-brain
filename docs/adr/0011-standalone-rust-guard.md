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

The current scoped runner executes 80 cases: 76 exact matches after owned-path and truthful toolchain handling, three explicitly unported doctor states checked fail closed, and one retained audit failure diagnostic deviation. These are scoped process contracts, not zero-unported acceptance. Native Linux/macOS/Windows CI must run the new named gate and retain the receipt. The standalone binary is buildable from source; release/signing/distribution and issue closure are not claimed by this ADR.

The product decision restores independent Guard usability while preserving the Brain/Guard and Room/Guard boundaries. It supersedes the Go-only standalone-retirement assumption for this Rust entrypoint, without inventing currently unshipped CLI commands or retiring future Guard library capabilities.
