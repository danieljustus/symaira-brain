# Coverage policy and executable evidence

The root Rust workspace has a hard **80% line-coverage floor**, recorded in
`rust/coverage-policy.json`. Every PR and main push runs the complete locked
workspace with all features on Linux x86_64:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.6.21 --locked
make rust-coverage
```

The pinned Rust toolchain is selected by `rust-toolchain.toml`. The command
writes full LLVM JSON and a source-bound summary to `target/coverage/` (the
existing external target root on local macOS). A failed test, missing report,
inconsistent denominator, omitted workspace member, or coverage below 80%
fails the gate. The CI artifact retains both reports for 14 days. The job
summary includes each crate, so weak packages remain visible in the aggregate.

The measured baseline is **33,698 / 41,701 lines = 80.80861%**, with 953 passing
tests and two existing ignored child-harness entrypoints. All 18 workspace
members are represented. `migration/evidence/coverage-682/linux-baseline.json`
retains every file summary and aggregate denominator, normalized source paths,
tool/source provenance and the complete raw report's SHA-256. The full raw
export also contains execution segments/functions; CI preserves those.

Source filtering uses cargo-llvm-cov's standard exclusions for external
Rust/toolchain dependencies, generated target files, and integration-test,
example and benchmark source directories. Executions of those tests still
contribute to product source coverage. In-file unit-test code remains in LLVM's
source aggregate. No extra file/function/package exclusions are added. The
separate Browse workspace, nested Go Guard module, and other native platforms
retain their own validation gates; these are not part of this root/Linux
measurement. Functions and regions are reported as diagnostics, not separate
hard floors. This is a repository policy, not a release or parity substitute.

Hermetic Rust subprocess tests preserve only `LLVM_PROFILE_FILE` alongside their
existing explicit variables. Instrumented children otherwise write `.profraw`
files into disposable fixture trees and lose their counters on cleanup. The
existing byte/mode/filesystem comparisons and their mutation controls remain
active. With profiling absent, their environment is unchanged.

## Go oracle execution and the published Go denominator

`make coverage` additionally instruments four deterministic Go generators
(policy, catalog, audit, patterns/activity) using the pinned Go toolchain,
`-cover`, and disposable HOME/XDG roots. Their existing `-check` contracts must
still pass without source or golden edits. Other oracle programs remain in the
complete unit-suite denominator. This adds measured execution to the existing
Go coverage report and badge; it defines no Go coverage floor.

The frozen CI unit input has 19,867 blocks and 29,386 statements. Instrumented
execution increases covered statements from **19,357 (65.87%)** to
**19,586 (66.65%)**, retaining exactly those 29,386 statements. The merge rejects
new blocks or changed statement counts instead of quietly changing scope.
Per-package percentages are calculated from the merged profile, and a separately
labelled product-code diagnostic excludes `scripts/` only from that diagnostic.
The all-source aggregate and published denominator retain those programs.
Raw unit/combined profiles and hash-bound receipts are uploaded on PRs and main.
`migration/evidence/coverage-682/go-oracle-statements.json` records the baseline
CI artifact and current actual four-oracle replay. The older issue's 64.6% Go
result is historical and is not the Rust baseline.
