# Keep active Go JSON token-state errors at end of input

Status: source-only prepared correction of independent F10. Parent
`fb43004be5e89e3f489f8a3f38c1224e2a98ff0e` / source `114f63fb` and its complete
F1–F9 source/proof packets remain immutable. Different-author full review,
compiled strict checks and all actual Go/native process/platform gates remain
required. No current compiler, SDK or product result is claimed.

The full independent review found that Go1.26.7 `encoding/json/scanner.go` calls
its active state once with ASCII space at EOF, before using the generic
`unexpected end of JSON input`. A state requiring a digit, literal continuation,
string escape or Unicode hex digit rejects that space with its exact current
context. The old native `next` helper instead returned the generic end error
for every exhausted lookup. This changed complete null-ID -32700 protocol error
messages despite preserving refusal/status. All eighteen original line/framed
wires and the complete four source owners were retained before correction.
Those observations are immutable source traces, not new SDK/product executions.

Preserve the general scanner-state rule at the required-token lookup seam.
`token_byte` returns the real byte, or the synthetic EOF space; existing grammar
branches produce their own errors. Its callers are only the active negative
number, fraction/exponent digit, literal continuation, escape and Unicode-digit
states, all of which reject space. It never appends or normalizes input bytes.
Container/value-start and ordinary-string states retain `next`'s generic end
error because Go accepts the synthetic space without completing those states.
Complete top-level numbers/literals/strings remain accepted. An actual invalid
byte keeps its original quote/context; it is not replaced by the synthetic byte.
The public error contains no new byte offset; Go's EOF step also does not add to
its consumed-byte counter. Existing frame owner, null parse ID, full error code,
stdout/stderr/status, read-only state and syntax-before-type precedence stay.

This focused seam avoids a second parser, terminal-text special cases or a
blanket error rewrite. It retains the full F7 string-aware depth10000 scanner,
F8 ordered linear metadata walk, F9 exact finite JSON conversion and all six
older path/capability/discovery/handler contracts. Core/Usage/dependencies are
unchanged. No existing comparator, input, control, fixture or assertion is
weakened. Binary attributes apply only to the eighteen exact new F10 wire files.

Prepared permanent Rust tests cover all partial numeric/literal/escape/Unicode
states in both transports, earlier typed failures, ordinary incomplete
containers/strings, completed top-level token contrasts, real-invalid-byte
contrasts and all eighteen exact original wire files. New process plans append
186 inputs after the unchanged926Unix/884Windows prefix, giving1112/1070. They
cover original eighteen wires, all required-token states in metadata, ignored
arguments and syntax-before-type positions, generic-EOF contrasts and valid
metadata counterparts. Only their exact parse-error IDs use the already strict
nil-ID comparator; valid inputs retain the original comparator. The original
seven actual-input controls remain, with two prepared actual EOF-input changes
following completely matched valid baselines. They require full exact errors,
exit/stderr and all filesystem file/raw-hash/validated-lock fields to match the
real Go baseline; bootstrap failure cannot satisfy them. All nine controls are
prepared, with zero current actual process runs.

Copied SDK source, standalone Rustfmt, Python AST/corpus projections and
synthetic comparator calls establish source preparation only. Full affected
Core/MCP/Gateway/Skills/CLI compilation, strict checks, original and additive
Go/native process/state tests, actual meaningful controls, existing independent
owner gates and native Linux/macOS/Windows acceptance remain mandatory. Full
Skills #764, whole migration and other unported codecs remain open. This source
checkpoint holds no Cargo target, cache, SDK execution or endpoint lease.
