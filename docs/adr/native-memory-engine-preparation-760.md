# Native memory engine ownership and first deterministic implementation

## Decision

Keep the engine in `symbrain-memory`, alongside its SQLite store and grounded
evidence. Reuse the existing pinned `symaira-core-llm` dependency for later
provider calls. Do not place memory-domain policy in Brain's capability gateway,
Guard's conduct policy or Vault. Importer DTOs and source discovery adapters
belong to #761; their raw content and checked evidence bridge feed this engine.
The first prepared increment implements complete fixed pattern rules, offline
extractive summarization, the aging curve and the context budget algorithm.
These are additive production library APIs, not a new user command.

All new APIs take their data explicitly. Aging takes `now`; summary and token
estimation accept bytes. Budget layer IDs retain bytes. No constructor opens a
database, reads operator configuration, spawns children or contacts a provider.
Existing CLI fallback predicates and MCP behavior remain byte-identical. Native
admission must expand only after actual full-pipeline Go/native comparisons,
including error paths and native Linux/macOS/Windows execution.

## Preserve the original algorithm, including observable quirks

- Pattern rule order, multiple matches per sentence, byte-length thresholds,
  duplicate suppression and single-pass replacements follow the frozen Go
  implementation. Go's `\s` matches ASCII whitespace; using Rust's Unicode
  whitespace would admit a different trigger. Grounding reuses #758's existing
  evidence alignment and leaves source identity for the caller.
- Summarization retains pairwise selection swaps, rather than replacing them
  with a stable sort. Its limit is the nonnegative domain used by both Go
  consumers (both currently pass 5). Go's first-byte capitalization can create
  malformed UTF-8; native output therefore remains bytes. It is not converted
  to a replacement-character String. The negative-limit Go panic is outside
  this prepared nonnegative API and remains a separate compatibility boundary.
- Aging retains the original constants, nonpositive configuration fallbacks,
  zero-time handling, duration saturation, lack of access boost without recorded
  access, and IEEE NaN behavior. There is no inferred cleanup or retirement
  transaction: the Store pass must preserve Go's per-row writes and first-error
  behavior before it is implemented.
- Budgeting retains the first two *positions*, regardless of layer name, and
  Go64 wrapping integer totals. Its `fit` flag reports the original set's fit;
  Go does not recompute it after dropping. This increment preserves that
  behavior. Rune counting consumes one malformed byte per Go RuneError.

Pattern extraction currently accepts valid UTF-8 `&str`; this is an explicit
prepared API boundary, not evidence that Go rejects malformed input. A complete
byte-valued source/evidence bridge is mandatory before importer or governed-write
cutover. The raw #761 importer boundary must not call a lossy conversion.

## Source-backed census and next production integration

The companion inventory records all ten packages, 115 function declarations,
and 22 imported-symbol calls in non-test `cmd/` and `internal/` Go source.
Original sources, tests, callsites, ledger and matrix are retained losslessly.
All 39 engine Go files match the immutable dcddcef0 checkout. Issue #760 is open;
its actual acceptance requires package differentials on three native OSes.

The next coherent wired increment is governed Store extraction: carry the
existing provenance, redaction, sanitizer, attribution, entity resolution,
embedding namespace, evidence and sync/audit order through the extracted facts.
Then add full conflict candidate/verdict policy and its configured local fake
LLM transport. Only then can the current CLI `set` fallback admit those inputs.
MCP uses the same owner; it must not maintain a second direct-write shortcut.

Consolidation/undo, aging Store passes, working compaction, temporal MCP filters,
full entity candidate ranking, context assembly and discovery remain required
parts of #760. The absence of an application constructor for a library does not
remove its exported contract. Full Memory config and eager runtime construction
are shared prerequisites with #759, and importer registry composition is
coordinated with #761. Historical schema repair remains #649, not an excuse to
invent new migration or data-rewrite behavior here.

## Prepared validation, not runtime acceptance

Ten unit tests and explicit Go/native package probes are prepared. The Go helper
is materialized only into an owned frozen checkout's contextassembler test
package, so it calls the original private budget method rather than a copied
algorithm. The native probe calls the new production functions. Summary and
layer bytes are hexadecimal; aging outputs full float64 bits. JSON records
compare complete fields without path rewriting, float tolerances or reordered
arrays. Empty test plans and missing output are fatal.

Future tests use private HOME/XDG/TMP roots and local fake endpoints on allocated
private ports; no default 11434 or paid provider is permitted. Clock/IDs and all
source paths must be bound in receipts. Input mutations, zero-case selection
and checked-output failures must reject the real executed comparison.

This checkpoint is source-only: no Cargo, compiler, Go SDK, product process,
HTTP endpoint, target or runtime suite is allocated or used. Formatting is a
source check; neither the prepared assertions nor probes have executed. Exact
Unicode case behavior, floating-point bits, all error/order contracts and the
full store/MCP/provider integration still require actual native acceptance.
