# Source module setup process proof

Run from the repository root with native Go, Rust and Python:

```sh
scripts/setup-source-oracle/run.sh /tmp/setup-source-parity.json
```

The launcher builds the full frozen production Go CLI at `dcddcef0` using the
existing immutable oracle launcher and the candidate Rust CLI with `--locked`.
It runs 111 real Go/Rust CLI cases on Linux and 95 each on macOS and Windows, plus two separate deliberately failing native
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
valid U+FFFD identity, Unicode whitespace and unusable worker temp roots.
Relative missing, file and existing temp roots additionally retain original Go
worker argv semantics and prevent implicit source-install behavior changes. Windows
also checks distinct implicit-CWD Git ownership with explicit GODEBUG opt-in,
CWD-only lookup and explicitly disabled implicit CWD. The corrected review
adds empty/unset PATH with an owned CWD executable, Windows relative-to-absolute
same-file continuation and different-file refusal, and raw Unix error paths for
temp staging, missing compiler payload, missing Browse directory and obstructed
managed publication, with successful raw temp and raw human-output controls. Tool identity cases also combine valid Unicode edge whitespace with malformed
interior UTF-8 and preserve the exact trimmed byte-valued provenance. Instrumented toolchains
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
# Failure observability

Run `python3 scripts/setup-source-oracle/test_progress.py` for direct owned-file
journal verification. It starts no product, Go or Cargo process and exercises
actual writes/replacement and injected write failure without weakening the
Source comparison. Current Linux execution is not a native Windows claim.

The replay checkpoints `OUTPUT.json.progress.json` before every invocation and
after every returned observation, outside compared fixture roots. Failed runs
retain all earlier pairs, nullable timeout streams and explicitly bounded live
fixture/capture/cache diagnostics. The unchanged final report and exact
comparisons still determine acceptance; progress cannot turn a failure green.

The source-only Windows experiment is an ignored owned integration test
`source_job_notifications::owned_job_diagnostic_helper`. Once its actual native
test executable has been built in the exclusive target, run
`python3 scripts/setup-source-oracle/windows_job_probe.py ACTUAL_TEST_EXE OUTPUT.json`.
It requires native Windows, retains all phases/raw streams/PIDs, and watchdogs
the deliberately historical blocking sequence. The bounded inner-wait and
successful-descendant comparisons must also complete. This diagnoses pinned
JobObject APIs; it does not establish the phase of the original CI failure or
establish production acceptance. No Linux/cross-target run counts as proof.

Two additive `source-cleanup` and `source-descendant` modes invoke the exact
production Windows helper: poll the inner parent, preserve completion-port
notifications, terminate the whole job and wait through its wrapper. They must
return successfully without watchdog cleanup and leave the recorded descendant
inactive. The original historical failure remains an observed negative control;
the original two parent-only-wait comparisons remain diagnostic comparisons.
Source's full native cases (111 Linux, 95 macOS, 95 Windows) and original controls
still determine acceptance, with the unchanged 25-second per-process limit.

On native Windows, the wrappers also retain actual Source Go/current CLI and
executed diagnostic test PE bytes and SHA/size/machine manifests outside the
compared roots, before temporary cleanup. CI uploads these additive files with
the complete reports. Original443artifacts omitted PE bytes; the retained
original source/observations explicitly preserve that gap. See the owned-job
notification ADR for the source-only correction and required native follow-up.
