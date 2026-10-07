# Prepared runtime handoff after normal main935 integration

This plan has NOT executed. Allocate one exclusive target/compiler explicitly
and verify at least 2 GiB of real headroom before beginning. Existing Brain or
Source targets are not implicitly borrowed. Preserve original executed bytes
and all source/failed attempts before any cache/package cleanup. Monitor a
700 MiB floor on target, output, TMPDIR and private Go-cache mounts at every
stage; all new local outputs belong under owned /workspace paths.

Use pinned Go1.26.7 and Rust1.98, jobs2, dev/test debug0, incremental0, umask022,
private synthetic HOME/XDG roots and existing source-bound Go builders. Linux
process/lifecycle tests run under the repository's approved subreaper wrapper.
No operator credentials, paid providers or public peer is required.

After source verification and exclusive resource allocation:

1. Fresh affected tests for symbrain-usage, symbrain-cli, symbrain-core,
   symbrain-audit and symbrain-skills, with --locked --all-targets --all-features;
   complete the workspace and strict Clippy/fmt/actionlint gates required by CI.
   Record actual tests/ignored counts, not the historical parent355 as a result.
2. Run all six unchanged Usage process gates: usage-fetch-oracle,
   usage-credential-oracle, usage-hermes-oracle, usage-provider-files-oracle,
   usage-copilot-kimi-oracle and usage-next-oracle via their tracked run.sh
   entrypoints, each with a separate owned JSON/evidence path. Preserve all
   owner/argv/admission/status/depth/request/report/read-only checks and actual
   negative controls. The usage-local-files-baseline Go-only observation remains
   available separately and must not be represented as native acceptance.
3. Run bash scripts/usage-retry-after-oracle/run.sh OWNED/original.json: exactly
   1600 scalar/1164 TLS constructor inputs plus four real rejected controls.
4. Run bash scripts/usage-retry-after-oracle/run.sh OWNED/decimal.json --decimal:
   exactly 1681 scalar/1356 TLS constructor inputs plus seven real rejected
   controls. All original prefixes/IDs/input bytes/assertions stay unchanged.
5. Verify actual Go/native SDK, target architecture, source/build/binary bindings,
   raw TLS request/response/header bytes, full reports and read-only filesystem
   metadata. Archive every executed binary losslessly before later variant
   builds, preserve failed bootstrap and comparison attempts, and restore Go
   caller/evidence bytes exactly after controls.
6. Complete the unchanged Skills and Activity acceptance required by the
   integrated main workflow. Native Windows Skills uses actual wide launch
   arguments/PEs, not a Linux WTF8 source projection. Existing protected CI
   criteria, exact counts and thresholds remain intact.
7. Obtain different-author full production/harness/provenance runtime review;
   exact-head Linux/macOS/Windows native CI remains mandatory. No publication,
   merge, whole #768 closure or cross-platform equivalence follows from this
   source-only handoff.

Historical SDK-only81 observations, parent constructors, archived binaries and
prior reviews retain their original source/platform identities. Prepared tests,
Python projections and current formatting are not native runtime observations.
