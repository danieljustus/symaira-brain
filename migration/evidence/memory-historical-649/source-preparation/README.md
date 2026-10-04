# Static Store-repair checkpoint

This checkpoint contains source and gate preparation, not validation or approval.
The coordinator has not allocated a target/compiler/runtime/port for this lane.
No Cargo, compiler, application process or operator database was accessed.

`static.json` binds the 37 exact frozen SQL copies and all prepared migration
modules/gate sources. All 314 original census inputs/results and retained gzip
round trips were reverified against their original hashes. Python sources were
parsed as AST only; the shell wrapper received `bash -n`; Rust files were
formatted directly with rustfmt. `git diff --check` passed. Those operations do
not establish compilation, Clippy, native OS behavior or SQLite runtime parity.

There are 24 prepared migration test functions, including all three previously
approved tests. The public concurrency test retains its 10 rounds × 8 openers;
the historical test covers all 37 ordered prefixes. Callback/index/constraint
controls compare every schema/table/FTS-shadow cell before and after the real
public Store opener. The actual-process scripts prepare 37 frozen-Go/native
pairs, 37 native reopens and nine SQL/child control pairs, without broad ID or
timestamp normalization. None has executed against this new source yet.

The current schema's approved flattened column order is retained. Literal owned
SQL supplies missing effects; final metadata/effect postconditions certify the
completed schema. The sole constructor addition tightens existing Unix database
and WAL/SHM modes after commit, like Go. Doctor inspection, UI, catalog, JWT,
configuration, retrieval admission and CLI cutover are outside this patch.

See `docs/adr/memory-historical-migration-repair-649.md` for the concrete
false-marker/backfill/FTS/unknown-object decisions and their limitations. A full
source-bound build/test/oracle/strict gate run, different-author review, native
Linux/macOS/Windows CI and a shipped release remain required before closure.
