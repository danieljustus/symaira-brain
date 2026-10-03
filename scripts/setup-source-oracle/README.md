# Source module setup process proof

Run from the repository root with native Go, Rust and Python:

```sh
scripts/setup-source-oracle/run.sh /tmp/setup-source-parity.json
```

The launcher builds the full frozen production Go CLI at `dcddcef0` using the
existing immutable oracle launcher and the candidate Rust CLI with `--locked`.
It runs 87 real Go/Rust CLI cases on Linux/macOS (81 on Windows) and two separate deliberately failing native
process controls (`wrong-exit`, `wrong-source`). These controls execute the actual
Rust CLI and must produce exactly their intended observable disagreement.
The report binds runtime, native SDK, binary and source hashes, clean/dirty HEAD,
raw outputs and complete before/after filesystem contracts.

Owned native fixture Git, Go and Swift executables record exact argv, cwd and CGO.
They exercise selection, ordering, skips, inherited config/ENV, human/JSON/help,
metadata/build/missing-payload/publication/version failures and foreign-owner
preservation. The independent review expanded this corpus with precise malformed
and permitted dash-prefix grammar, separate/inline raw Unix module arguments,
raw unknown/bool flags, missing and successfully installed raw source/home paths,
valid U+FFFD identity, Unicode whitespace and unusable worker temp roots. Windows
also checks distinct implicit-CWD Git ownership with explicit GODEBUG opt-in,
CWD-only lookup and explicitly disabled implicit CWD. Instrumented toolchains
are fixtures, not real product builds.
A separate case uses the actual Go compiler on an owned dependency-free source
worker and checks the installed version and payload hash. The shared private
compiler cache is outside per-case snapshots and removed with the runner root.
Actual Go telemetry is disabled by an owned `mode` file and `GOTELEMETRYDIR`, not
by discarding any generated files or telemetry mismatches from the comparison.

Only fixture roots, the observed unique source staging and atomic rename suffixes,
and new UTC sidecar timestamps validated within the actual process window normalize.
All sidecar bytes, payload hashes, file modes/types, stdout, stderr, exit and tool
calls are compared. Installation failures retain partial effects exactly as Go.
An additional actual Rust source setup spawns an owned build descendant and must
return successfully with that descendant terminated. Unix process groups and
Windows suspended-before-Job-assignment avoid losing children to a spawn race.
Native macOS and Windows execution requires their CI jobs; Linux is not that proof.

This is Rust CLI orchestration of historical Go Browse/Swift Operate/Scope workers.
It does not establish Go-independent source setup or close #765/#783. Future Rust
Browse builds need an explicit trusted manifest/version contract and locked Cargo
builds; rejected/failed builds must not downgrade silently. Full invalid-config
loader diagnostics remain a separately tracked Go-owned boundary. Hardware, native
permissions, production signatures and the complete worker feature ports remain
outside this fixture proof. See the source setup ADR for decisions and rationale.
