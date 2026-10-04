# Fetch deadline fixture successor: Linux supplemental proof

Validated source: `6eb3cde9db6160d099a29670c4ba287f2faa5891`, normally based on
published `9b4ca8875f23b6668a2814ff73632354afcad777` with main `e3dbda6c`.
The ADR is `docs/adr/fetch-proxy-deadline-phase.md`. This is a test-only
correction; all 22 original Fetch production source files retain their bytes,
except the explicit `cfg(test)` module append. Cargo.lock is unchanged and the
existing exact Tokio 1.53.1 receives only a dev test-util feature.

The proper actual-Go/native MCP harness runs all 13 frozen fixtures, three
fixture mutants, and the complete Browse workspace: **303 passed, zero failed,
one existing ignore, 66 summaries**. Full workspace all-target/all-feature
Clippy, fmt, workflow actionlint, matrix88/17, benchmark-harness7 (two platform
skips), identity4, frozen static28/profile11/robots15 and ten Go packages pass.
The initial direct all-feature workspace attempt lacked the required generated
MCP outputs and native daemon context: **297 passed, four fixture setup
failures, one ignore**. Its complete raw log is retained; it is not relabeled a
passing gate. The owned harness closes this prerequisite with real Go outputs.

All five native proxy integration tests and all three actual response/task
component tests pass. Three fresh actual ELF positives and three owner/clock
mutants reject the intended retained-body, leaked-task and insufficient-time
defects. Each mutant receipt preserves its full stdout/stderr, exit, source,
artifact mapping and actual ELF SHA. Real OS close and successful peer-task
join follow controlled time, while the original client is still alive.

Frozen-Go/native process gates pass251/251 with five actual output mutants;
raw32, Referer7 exact + original E011, Linux owned TLS4, native jars4 and three
raw mutants; ordered auth114 and eight enforcing peers with three auth mutants.
Daemon63 and three controls, two exit tests, lifecycle and50 rounds with50
concurrent starters pass. All140 earlier reviewer byte/wire boundaries pass,
including256 octets and two three-hop chains. All original supplemental38
inputs run:34 match and the same four inherited origin URL nonUTF8 auth gaps
remain visible. Original literal-port5 match; original HTTP8 retains seven
matches and the identical automatic-fragment E011. There is no new exception.

This Fetch correction does not close #773/#772 or
permit an unproved default cutover. Native six-platform exact-head CI and full
independent review are still required. Parent240 value samples and120 wire/cost
pairs remain retained; no new-source benchmark or size claim is made here.

`validation.json` maps68 complete raw logs, reports, environment/source checks,
compiler-artifact records and exact runner inputs with gzip roundtrip hashes.
The unchanged process runner's actual generated Go binary was copied from its
owned temporary directory before cleanup and checked against its executed SHA.
All152 current ELF paths (121 unique), including shared/macro artifacts and
three separately identified Go binaries, are roundtrip archived at
`/workspace/oracles/symaira-fetch773-deadline-6eb3cde-binaries/receipt.json`.
The execution sidecar directly binds63 observed test/probe/CLI/Go paths; shared
artifacts and unobserved compiler outputs are not called executed tests.
Original full CI logs/ZIP/triage/30ms source/ec10 ELF and all169 original ELF
paths/113 unique objects were reverified against their retained bytes.

All runs use the single exclusive target at
`/workspace/symaira-fetch773-deadline/target`, pinned Go1.26.7, Rust1.98.0,
debug0/incremental0/jobs2, umask022 and the owned subreaper. The failed direct
workspace run and the provisional precommit focused/control/lint runs remain
separately recorded. No second target, production-clock change, weaker EOF
assertion, retry-until-green or native macOS/Windows claim is introduced.
