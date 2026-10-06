# Keep SDK arithmetic and immutable evidence during PR804 CI repair

Status: source-only successor of `cb2e852ed55b95d5e9f62eecbcf3b3eb7a858cd2`;
different-author review and fresh actual/native CI remain required.

The original PR804 Linux job `111449314987` and Windows jobs `111449315080`
and `111449315107` rejected four Rust lint violations in the transferred
Go1.26.7 binary64 parser. Go job `111449314855` rejected formatting drift in
two retained original evidence files, rather than live Go production. Complete
original logs, SHA values and native file observations, the complete original
Git tree map and 114 original source/SDK bodies are retained under
`migration/evidence/usage-retry-after-768/ci-minimal-successor`.

Keep the SDK rounding and fallback expressions verbatim. Two function-local
Clippy allows retain `shouldRoundUp`'s parity modulo and Eisel-Lemire's low-nine-
bit mask test. Their comments identify the SDK contract; the workspace's strict
Clippy settings and every other function remain covered. Rewriting these exact
transcriptions just to use a newer lint's preferred spelling adds comparison
work without improving this numeric contract. Byte comparisons remove only the
new attributes and prove both function bodies unchanged.

Quote `ParseFloat` as code in the module documentation. Give the internal
`Number` value `Clone, Copy`: its five fields are `u64`, `i64` and three booleans,
with no allocation, external resource or destruction policy. This makes its
existing by-value hexadecimal conversion explicit without changing the scanner,
rounding implementation, signatures or visibility. A byte comparison removes
only the derive and documentation backticks and proves the original algorithm
unchanged. No dependency or toolchain version changes.

Define formatting ownership consistently in `make fmt`, `make fmt-check` and
the release workflow: immutable originals under `migration/evidence/` are data,
so format only the remaining owned source inventory. Keep the original evidence
paths and bytes because historical receipts bind them. The current tree has
four such `.go` files: two copied Skills SDK reference files and the two Usage
Go path probes identified by the failed format check. No live `cmd/`,
`internal/`, `scripts/` or non-evidence migration tool is excluded. Existing
nested-module/check-out exclusions, NUL-safe path handling, bounded batches,
formatter failure handling, Go vet/build/test and source-bound oracle inputs
remain intact. The evidence is still hash-checked, not silently rewritten.

Extend both existing real-Go formatter regression tests with deliberately
unformatted evidence and explicit unchanged-byte checks. Their existing
ARG_MAX, drift and release syntax-error assertions remain. These two Go tests
are prepared, not executed in this source-only allocation. Nine executed owned
shell/Make controls use a fake formatter to check routing, newline paths,
release batches of 100/100/7, live drift/failure propagation, actual `make fmt`
ownership and two mutants removing the evidence exclusion. They establish
source boundary behavior, not Go formatting or Rust runtime correctness.

Fresh Rust formatting, workflow actionlint, extracted Bash syntax and Git
whitespace checks pass. The first source-inventory check incorrectly assumed
the two CI drift paths were the entire evidence inventory. Its complete failed
check and correction to the four actual paths are retained; candidate bodies
and assertions were unchanged by that correction.

Before merge or release require different-author review, actual strict Clippy and
affected/full Rust tests, both real-Go formatter tests and `make lint`, original
six Usage gates, original Retry1600/1164 and additive1681/1356 process gates with
all seven real controls, and fresh exact-source Linux/macOS/Windows CI. After
independent review, the candidate may be pushed normally without rewriting
remote history to start those fresh CI checks; that push implies no runtime
acceptance. This
checkpoint executes no Rust/Go compiler, SDK, product, HTTP peer, port or Cargo
target and creates no current executable. Prior accepted or prepared evidence
keeps its original source/platform identity. PR804 and issue #768 remain open.
