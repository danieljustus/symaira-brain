# Native memory CLI parsing and configuration (#758)

Status: implemented increment; native three-OS acceptance and full #758 pending.

## Decision and rationale

Split the former 1,613-line `memory_cli.rs` by behavior: routing, help, flags,
paths, lightweight reads, rules, query logs, search and writes. Every changed
production module stays below 400 lines. Keep one raw-argument flag parser for
the six store verbs, with per-verb definitions and usage text. Independent
parsers had drifted on aliases, duplicate flags, flags after positionals, help
precedence and Go integer syntax; a shared parser prevents the same divergence
from recurring in each verb.

Preserve the immutable Go flag contract, including error bytes and exit codes.
Help `-h`/`--help` takes precedence anywhere, even when supplied as another
flag's value. The three positional verbs reorder flags before parsing, while
read verbs stop at the first positional. Integers use Go base-zero syntax and
overflow precedence; explicit booleans accept exactly Go's spellings. Unknown
commands and usage failures stay native without opening a store or loading
configuration. Unix error diagnostics preserve raw argument bytes.

Implement the one-shot memory configuration seam locally. Brain has one memory
CLI consumer and an existing TOML parser; adding a new shared CoreKit package
would create an API/dependency obligation without a second consumer. Follow
global `symmemory/config.toml`, then `.symmemory.toml`, then typed `SYMMEMORY_*`
overrides. Relative XDG config roots are ignored. Validate every known field,
including fields unused by CLI reads: any loader error makes the shipped CLI
discard the entire partially merged configuration and start from defaults.
Unknown fields are ignored. File zero values are ignored except pointer bools;
nonempty environment strings can explicitly set false/zero. Configkit rejects
file maps and skips environment maps. TOML duplication, syntax and type errors
also reset the whole config. Database argv/environment overrides preserve raw
OS paths. Search uses the configured Ollama endpoint/model, then the existing
hash fallback; plain `SearchMemories` keeps its Go default ranking weights.
Routing, path resolution and embedding generation share the first cached config
snapshot, as Go configkit does, so a changing file cannot split one CLI invocation
across different configurations.

Fix the native list default to the shipped CLI's 100 rows, retaining its 1,000
row maximum, and make aliases use the last supplied value. Apply Go HTML escaping
to list JSON as search already does; parsed-JSON equivalence alone missed this
observable byte difference. Keep dynamic governed
writes and configured Hamming-prefilter search on Go until their complete
stateful behavior is implemented and proven. Parser support alone is insufficient
to remove those gates: governed set also owns provenance, conflict/deduplication,
entities, extracted subfacts and grounded evidence.
The prior canonical-kind/five-scope write boundary stays in place; parsing kind
aliases does not ungate additional valid write shapes without state evidence.

## Evidence and boundaries

The tracked replay archives immutable Go `dcddcef0` and verifies CoreKit v0.17.0
configkit SHA-256. A supplemental reflection program executes Go to verify all
86 descriptor names/types. Native processes use disposable HOME/USERPROFILE/cwd
and XDG roots; runtime PATH is empty and the explicit Go fallback binary absent.
Literal stdout/stderr bytes and exit codes are compared. Configuration and read
cases also compare every SQLite table, row, column and blob before/after both
processes, with no dropped state fields. Seeded rules/list/query-log cases cover
1,100 memory rows, 80 log rows, limits and repeated aliases.
Both table and successful JSON output are exercised. Three additional searches
use a loopback-only fixture to compare the actual Go/Rust embedding request path
and model/input payload under global/project/env overrides; no paid endpoint is
called, and the empty store remains unchanged.

The final harness executes 590 cases on Unix and 553 on Windows. Two executable
negative controls run the real candidate and separately mutate a same-length
semantic output token or success exit, requiring a real failed replay while the
database remains identical. Source/binary hashes, SDK, exact candidate revision
and dirty status bind every receipt. The native Linux/macOS/Windows workflow
must pass before accepting this increment.

Initial argument probing found three boolean error-prefix differences; these
were corrected against the actual Go process. Initial schema coverage extraction
omitted the digit-containing `bm25_weight` name; the tracked harness now includes
digits and fails unless executed Go reflection exactly matches all 86 entries.
These observations are retained separately from final passing receipts.
Eight seeded list JSON cases also exposed missing native HTML escaping while
parsed values and every database row were already equal. That complete failed
process receipt is retained; the final gate requires literal bytes and successful
read/search exits, so matching usage failures cannot masquerade as JSON proof.

Full #758 remains open: governed write state, every database open/error shape,
new database file/directory permission parity, configured Hamming prefilter and
Go-compatible evidence JSONL decoding still need implementation and proof.
This increment does not certify MCP work owned by #760/#761, nor remove `serve`
or synchronization fallback. #649 stays open until the repair is shipped in a
Rust release and its Doctor diagnostic is verified. Go source and frozen
fixtures remain unchanged.

## Independent review corrections

Independent review of clean `1d8a090` executed all initial 583 cases, two real
controls and fifteen additional config probes, then found two real P2 failures.
Those original reports, literal stdout/exit/state measurements and source hashes
remain tracked under `migration/evidence/memory-cli-758/independent-review-1d8`.
The rejected source worktree stays unchanged; corrections use a separate worktree.

Memory's frozen `internal/paths` contract differs from Brain's general data-path
contract: only absolute XDG data roots are accepted, and current
`base/symbrain/memory` and legacy `base/symmemory` share the same selected base.
Directory existence takes precedence over ordinary files. The correction is
memory-local so audit and other domains retain their independently specified
relative-XDG behavior. Seven new real process cases prove relative/unset/empty
XDG behavior, legacy selection, directory/file type and current precedence while
every seeded database remains unchanged.

Rules JSON also needs Go HTML and U+2028/U+2029 escaping. Initial seeded reads
populated memories but left the separate rules table empty, so the rules shapes
did not exercise serialization. The corrected fixture seeds real global/project/
agent rules, metadata keys/values and both actor fields containing all escaping
characters. Rules cases require actual rule IDs in successful output; an empty
table cannot pass this assertion. The archived original binary fails four of
seven new path cases and three populated rules JSON cases; the fixed binary
passes all of them. No Go production or frozen fixture is modified.

The focused fixes were normally merged with actual main
`31de72294521701fc4b1a0bce39f03cc34d72e7e`; clean combined source is
`2572f1bf28ac57c3af348245f1444aa6172ce997`. The new CLI SHA-256 is
`71091ff9a963d5effdcb0f741085ff7c6f4723e83a0cd0a8b5a7d7f78c2d170e`.
This preserves the accepted Activity cutover in the shared CLI dispatcher and
keeps the rejected original source and binary receipts separate.

Combined Linux acceptance passes all 590 memory comparisons, both actual process
failure controls, all 265 actual Go Activity validation comparisons, 117 CLI lib
tests and both Activity process fixture tests (119 Rust tests total), plus strict
Clippy/format/actionlint. Exact source/binary hashes and literal process outputs
are tracked under `migration/evidence/memory-cli-758/review-fixes-2572`.
The first broader lib invocation omitted this environment's subreaper and failed
the Doctor retained-descendant cleanup check. Its literal failed log is retained;
the unchanged source passes all 117 lib tests with the lifecycle wrapper.
The prior 333-test record remains evidence for the earlier source; it is not
claimed as a fresh full-suite execution of these corrections. Native three-OS
and the documented full #758/#649 release gates remain pending.

Integration decision: extend existing PR803 with the independently reviewed
native Memory CLI. Store repair, evidence and CLI share one database contract
and both three-OS workflows must pass on the integrated head. Preserve the
original two P2 findings and real failures alongside corrected590 pairs,265
Activity pairs and119 independent tests. A fresh parent check separately
passed59 Memory/Gateway tests,83 actual Go evidence observations,4 Go DB
tests and3 rejecting controls. Historical333 is not relabeled as a new run.
This keeps one reviewable domain change and avoids merging a store with an
unverified CLI adapter. Full #758 and release-level #649 remain open.

Native Windows CI found needless_pass_by_value in the non-Unix byte helper
at160f24b. Borrow its input and preserve both platform decoding rules; no
lint waiver. Retain both original complete job logs and require fresh native
acceptance. Targeted Linux Memory CLI tests and strict Clippy pass, then
replay all590 actual process contracts on the new immutable source.

## Close oracle SQLite handles before removing private fixtures

Fresh native Windows head e8d2970 passed strict Clippy and component tests, then
failed the actual process oracle while removing an owned configuration fixture:
Python's SQLite connection context commits or rolls back but does not close the
connection. Windows rejects removing env.db while that handle remains open.
Use explicit closing around the existing transaction context in all three
fixture seed/snapshot paths. Keep every comparison and all input counts; do not
ignore cleanup errors or weaken assertions. The original complete failing job
111293527292 is retained. This changes only fixture resource ownership; fresh
Linux process proof and native Windows acceptance remain required.

## Default-path fixture handle ownership

The subsequent exact-head9c Windows run37155195743, job111297047149,
passed the corrected configuration fixtures, then exposed the same retained
SQLite handle in the separate default/legacy-path seed helper. Close that fourth
owned connection after its transaction exits. Preserve all seven path cases,
selected IDs and before/after database comparisons; do not ignore cleanup errors
or retry deletion. The complete original Windows failure is retained as
windows-cleanup-9c-job-111297047149.log. Fresh full Linux replay and the two
actual process controls are repeated; corrected native Windows remains required.

Fresh clean d7affb25 full replay passes all590 comparisons and both actual
process controls (32 rejected pairs each). The retained original Windows log
is explicitly binary-attributed; its complete CRLF bytes match the downloaded
job log. The first Git add normalized the log to LF; that historical commit
remains, and this evidence commit restores the exact original without rewriting
source history. Production Rust and all fixture assertions remain unchanged.
