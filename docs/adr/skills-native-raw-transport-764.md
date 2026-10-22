# Skills MCP raw JSON/EOF transport layer (#764, slice a)

Status: implemented and locally verified on a branch from main `9bf83c3f2`.
Native Linux/macOS/Windows CI is pending. Salvaged from PR #813 (head
`578d201f5`); that PR's longer ADR series (`skills-native-json-depth-float-764`,
`skills-native-json-eof-764`) holds the review history and is not reproduced.

## Decision

Go decodes every `tools/call` with `encoding/json`, so a request naming a Skills
tool fails with Go's exact message before any handler runs. The native
transport reproduces that for the eleven Skills tool names only:

1. **Syntax first.** One iterative, string-aware scan validates the whole
   frame (depth limit 10000, number grammar, escapes, literals). Errors become
   null-ID `-32700` replies with Go's text. The scan reads actual bytes; no
   prefix search establishes ownership of a malformed frame.
2. **Active-state EOF.** Go calls its active scanner state once with a space at
   end of input; required-token states (negative number, fraction/exponent
   digit, literal continuation, escape, `\u` digit) reject it with their own
   context, other states report the generic end error.
3. **Ordered typed fields.** `jsonrpc`, `method`, `params.name`, `_meta` are
   decoded in encounter order with case-folded keys; the first type error is
   reported as `-32602`. `_meta` numbers are converted with Go's float64 rules
   (`symbrain-core::go_json_float`, Eisel-Lemire with 800-digit fallback,
   validated against 81 actual Go SDK numbers).
4. **Strings.** Invalid UTF-8 and unpaired surrogates become U+FFFD, as in Go.
5. **Scope.** Other names keep the inherited generic `serde_json::Value`
   decoder. The gateway asks `raw_skills_params` first and only then
   `decode_params`; a recognised call carries a repaired argument `Value` in
   which an ignored out-of-range member is `null` (Go skips it unless a typed
   field targets it; the six native readers only consult named string/bool
   members).

## Consequences

* The five Go Skills tools main does not implement stay unregistered; the
  layer already recognises their names, so their outer errors match Go.
* Handler-level typed argument decoding (Go's anonymous argument structs) is
  later work.
* Four recorded cases (`name` is a number / lone surrogate) diverge from Go
  because they belong to the inherited decoder; the oracle test asserts the
  divergence so it cannot be forgotten.

## Evidence rule

Expected values are never hand-written: `scripts/skills-native-oracle/
raw_layer_oracle.py record` stores Go's actual replies; `check` fails if the
fixture differs from a fresh Go run. Isolated-state tests forward
`LLVM_PROFILE_FILE` (or point it outside the tree) so instrumented runs stay
immutable.
