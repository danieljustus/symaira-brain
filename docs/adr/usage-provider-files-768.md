# Claude and Codex credential-file migration (#768)

## Work-in-progress checkpoint, 2026-10-03

The initial Claude/Codex changes in this branch are an unvalidated checkpoint.
They introduce shared capability-rooted read-only file access and ordered raw
JSON object decoding, typed Claude map merging, and exact-key generic Codex
JSON decoding. No compiler, test, Clippy, or fresh oracle result is claimed for
this checkpoint. Existing routing tests and oracle coverage still require work.

The lane was paused while independent review found a Hermes backing-slot
shrink/regrowth bug in immutable `273a93c0`. Preserve this checkpoint before
integrating the focused `9b37c53` repair; retain the original failing evidence.
The completed implementation must keep automatic Claude host-Keychain discovery
and nondeterministic selection among distinct nondefault account tokens on Go.
It must not read operator credentials or contact real provider endpoints.

## Implementation decision and required evidence

Claude files use Go's typed, ordered object semantics: Unicode/case-folded
struct fields, ignored raw metadata, scalar-null no-op, merging repeated root
maps, and fresh values for repeated account keys. The exact `default` account
wins. Otherwise identical tokens across accounts have a deterministic result;
multiple distinct nonempty tokens retain the Go route because Go map iteration
is unspecified. Invalid/unreadable files and wrong-typed known fields supply no
token, matching the unchanged Go constructor.

Codex files follow generic `map[string]any` semantics instead: exact keys,
last-value duplicate replacement, top-level token preference and nested fallback.
A valid unknown overflowing number invalidates the whole generic document in
Go; typed Claude metadata with the same number is ignored. Raw ordered fields
and an iterative numeric/depth check preserve that distinction without building
a recursive generic tree. All generic numbers must fit finite float64 and the
Go decoder's 10000-container limit. Deep ignored metadata therefore does not
inherit serde's generic Value recursion limit or overflow the native stack.

Tokens read from these files remain literal, including `env://`, `symvault://`,
`vault://` and `keychain://`. The Go file readers do not resolve them; interpreting
them as references would silently change request credentials. Environment
references continue to use the previously accepted secure shared resolver.
Explicit/empty CODEX_HOME overrides follow the existing Go path selection.

The existing capability-rooted Hermes reader and ordered raw object visitor are
moved to a shared private module without adding a dependency or widening APIs.
All three file families keep the selected parent boundary, regular-file check,
64 KiB cap and nonblocking Unix special-file rejection. The actual owned FIFO
Go timeout remains an explicit native safety difference. Sources remain
read-only: no credential refresh, rotation, lock, permission change or write.
The earlier Hermes shrink/regrowth correction and original failure are retained.

Claude's constructor now has the same private last-source injection point as Go.
The production wrapper still supplies the existing automatic Keychain adapter.
The oracle supplies an empty owned adapter and compares call counts, proving
file/environment precedence and that a usable token suppresses redundant reads.
This is not host inventory/ACL evidence; the existing automatic Keychain gate
stays. CLI tests seed an unresolved OAuth environment source to prevent operator
Keychain access, while separate actual constructor replays prove full file-derived
reports. Every request uses a synthetic token and canned 401 transport.

The new gate requires 86 distinct inputs: 85 complete report/auth-header/no-write
comparisons and one retained nondeterministic-account gate. Real CLI comparisons
cover every deterministic route before a request and both output modes for every
credential-free source. Five actual failure controls must reject corrupt or
incomplete replay inputs. The shared reader additionally receives the complete
92-report Hermes gate, its Unix filesystem observations, and the 44-report
reference gate on the same source revision. Native macOS and Windows receipts
remain required; Linux alone does not establish their contracts. Other #768
file families, host Keychain, numeric JWT overflow and configuration gates remain
open.
