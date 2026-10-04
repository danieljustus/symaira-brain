# Independent full-layer review: native Memory CLI #758

Candidate: `1d8a0904929157d7928cfcd31782c374aa98501a` (clean).
Production source: `2c9a42054382fc55d361853e071fb9bb3d671b71`.
Parent: published Memory evidence candidate `97a78186`.
Verdict: REQUEST CHANGES. Two confirmed P2 findings; original candidate unchanged.

## P2: resolve the same default/legacy database as Go before ungating configured reads

`rust/symbrain-cli/src/memory_cli/paths.rs` uses the general Core data resolver,
which accepts relative XDG_DATA_HOME, and builds the legacy path directly from
HOME. The frozen Go memory resolver ignores relative XDG_DATA_HOME and checks
both current and legacy directories under the same selected absolute XDG/HOME
root, using directory type checks. Removing the configuration fallback exposes
these differences to configured read commands with no explicit database.path.

Actual Go/Rust CLI processes reproduce both errors with harmless configuration
`ollama.model="fixture-only"`, separately seeded databases and empty runtime PATH:
relative XDG_DATA_HOME selects HOME default in Go versus relative current in
Rust; absolute XDG_DATA_HOME with an XDG legacy directory selects that store in
Go versus HOME legacy in Rust. Both exit 0 but return different records. Every
database table/row/blob remains unchanged. Preserve the original receipt
`/tmp/symaira-memory758-cli-independent-paths.json`. Fix the memory-local resolver,
leaving Core's independently specified data resolver intact, and cover directory
versus regular-file type and current/legacy precedence in the actual process gate.

## P2: apply Go JSON escaping to populated rules output

`rust/symbrain-cli/src/memory_cli/rules.rs` prints RuleRow JSON without the final
Go HTML/JavaScript-separator escaping added to list/search. A configured store
with rule content `rule <private> & Unicode β` and U+2028 produces literal
characters in Rust versus escaped `\u003c`, `\u003e`, `\u0026`, `\u2028` in Go.
Actual CLI processes both exit 0, with identical persisted rows but different
stdout bytes. Removing the config gate exposes this configured read regression.
Receipt: `/tmp/symaira-memory758-cli-independent-rules.json`.
The 32 seeded-read shapes run rules against an empty rules table; seed actual
rules, including metadata/actor strings, so rules rendering is tested meaningfully.

## Scope, methods and evidence

Read all changed production modules, raw-argument parser and usage/error text,
Go-kind aliases, one-shot config cache, 86-field typed schema, decimal/hex-float
syntax and rounding boundary, database/read/write/search routing, literal JSON
rendering, test modules, replay/control/reflection programs, workflow, matrix
and ADR. Trace behavior against frozen Go cmd_memory*.go, internal/paths,
configkit v0.17.0 and actual binary metadata; verify domain boundaries and
retained governed-write/prefilter/sync/serve gates. No other actionable findings.

Independent clean-head replay: 583/583 actual comparisons (246 arguments,
302 configuration, 32 seeded reads, three owned configured embedding HTTP
requests), 86 fields verified by executed Go reflection. Two fresh executable
negative controls run the actual candidate and reject semantic stdout/exit
mutations across 32 cases each while keeping DB state unchanged. Fifteen
additional actual config probes cover unknown overflow/NaN/date/array/map,
valid and invalid typed numeric/bool values and inline tables: all match.
All 142 authored source hashes exactly match the independent run and current
candidate. CLI SHA-256 `5b76d21337248d59f42b9f1f70c9296559e23ff03929c7f9d03c7323c44a650a`;
actual immutable Go SDK 1.26.7 binary SHA-256
`a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`.
Author's 333-test/33-summary, zero-failure/ignored receipt and strict lint evidence
checked; not represented as a second full independent 333-test execution.
Go production and frozen fixtures unchanged; changed production files under 400
lines. Native Linux/macOS/Windows acceptance remains required after corrections;
full #758 and shipped-release #649 acceptance remain open as documented.
