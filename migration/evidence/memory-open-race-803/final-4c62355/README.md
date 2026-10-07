# Focused Store-open author validation

Tested source: `4c6235573f6d03bf9c2302217d9bd23fb814697f`.
Normal integration parent: main `5e232700bb9031fc34d4995465a4037837540abb`.
Rust/Cargo bytes match the previously frozen `d751daa` source. The publication
commit adds only these evidence/docs files after the clean source checks.

`run.sh` records the actual owned HOME/XDG paths, pinned SDK paths, sole released
target, jobs=2/debug=0/incremental=0 and 700 MiB stage floor. Run from the
worktree through `python3 /tmp/symaira-subreaper.py bash .../run.sh`.
Only the Memory library was compiled/executed; no CLI application, Go oracle,
operator database, default embedding listener or GitHub mutation was used.
Go SDK version inspection is recorded separately from application execution.

All 40 library tests passed with zero failures/ignored tests. This includes the
33 original tests, their unchanged ten rounds of eight concurrent public
opens/writes, and seven focused WAL tests. Strict Memory all-target/all-feature
Clippy, whole-workspace fmt, actionlint and diff whitespace checks passed.

The exact old production pragma batch fails with actual SQLite BUSY 5 and zero
busy callbacks under a retained RESERVED writer. The new helper observes that
real failure, succeeds after its owner releases it, then completes unchanged
IMMEDIATE migration with the retained row intact and timeout restored to 5000.
The persistent writer made 980 genuine early failures within 5.000072542 seconds;
the persistent reader exhausted its actual handler wait in 5.006185954 seconds.
The mixed RESERVED-to-SHARED-reader control made 196 early failures, released
the writer at 1.000886987 seconds and gave the genuine reader-blocked last
statement only 3994 ms, finishing at 5.003546279 seconds. It asserts the actual
SQLite timeout and reader wait rather than a portable upper wall-clock limit.
The compiled full-budget-each-attempt mutant failed with timeout=5000 ms and
6.01 seconds total. Its original source patch, log and executable receipt are
retained under `mixed-a112189`.

`validation.json` binds source/SDK/raw logs and the actually executed test
executable (SHA256 `2cd38f22f96327fc9d9c62bd0484d3054ea0105d79033d2a410a2f2333816b08`).
The archive receipt maps all 513 current ELF files/477 distinct byte hashes to
freshly verified gzip round trips; this includes build objects and transferred
cache files and does not assert that those are current executed test binaries.
Unchanged archive bytes are referenced directly rather than duplicated.

The original Windows CRLF log and decoded observation, first failed reader
assumption, initial actionlint PATH failure, Clippy test diagnostic, successful
intermediate checks and all original executed test bytes remain unchanged.
The outer Windows unwrap does not identify an internal phase; Linux mechanism
proof cannot establish Windows closure. Independent review, complete inherited
CLI gates, exact-head native Linux/macOS/Windows runs and shipped historical
repair remain pending. No #649/#758/full Memory closure follows from this slice.
