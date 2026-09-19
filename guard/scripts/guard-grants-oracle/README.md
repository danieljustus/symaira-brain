# Guard grants Go oracle

`run.sh` builds the pinned Go `symbrain guard grants` command and records exact
stdout, stderr, exit codes, and the resulting grant-store files. The cases
cover empty and ordered listing, fractional/offset timestamps, single and all
revocation with persistence, missing IDs, malformed and top-level-null storage,
legacy nullable/missing fields, JSON string escaping, and argument validation.

Run every local action through the external-storage wrapper:

```bash
bash guard/scripts/guard-grants-oracle/run.sh check
bash guard/scripts/guard-grants-oracle/run.sh test
bash guard/scripts/guard-grants-oracle/run.sh parity target/debug/symbrain
```

`parity` takes an explicit native binary and compares all thirteen cases against
the frozen fixture, including exit code, stdout, stderr, and grant-store
state (file content and modes). It also fails closed on macOS without the
external-storage wrapper.

The oracle fails closed on macOS without that wrapper. The fixture is bound to
the pinned Go commit and hashes the grant CLI/store source files; a source
change requires an explicit reviewed `write` run.
