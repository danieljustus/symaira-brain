# Claude/Codex credential files (#768)

`bash scripts/usage-provider-files-oracle/run.sh OUTPUT_JSON` uses immutable Go
`dcddcef0`, supplemental untracked tests, private HOME/XDG/provider roots, actual
constructors and canned 401 transports. Production Go and frozen fixtures remain
unchanged. No operator credential or real provider endpoint is used.

The gate requires 86 distinct source cases: 85 full report/auth-header/no-write
comparisons and one actual Go nondeterministic account case whose native route
must remain gated. Claude uses the production constructor's private Keychain
injection point in both implementations, proves exact last-source call counts,
and deliberately supplies no system credential. This proves file resolution and
ordering, not host Keychain inventory or ACL access.

Actual CLI processes compare all 85 deterministic routes using an invalid flag
before any request, plus table/JSON bytes for every credential-free source.
Claude CLI cases use an unresolved environment source to preempt operator
Keychain access; complete file-derived report states are proved separately by the
constructor replay. Codex Unix filesystem probes cover owned symlinks, a
directory and a FIFO. FIFO rejection is an explicit native safety contract;
immutable Go's actual timeout is preserved, not counted as parity. Windows does
not receive credit for Unix-only probes.

Five actual rejection controls require wrong CLI exit, absent CLI case, missing
Go fixture, changed full report and duplicate case coverage to fail real replay
processes. The fresh source-unit oracle is ignored in ordinary tests and is
explicitly executed by this gate; missing/empty evidence cannot pass.
