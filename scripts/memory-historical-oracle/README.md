# Historical Store repair gate (#649)

This prepared gate runs real frozen Go and native CLI processes against 37
generated migration-prefix databases with synthetic rows, not captured
historical installations. Each process receives an independent copy. No Go
source or frozen fixture is edited. The original 314-file census, including the first rejected 20-pair
recipe, is retained under `migration/evidence/memory-historical-649/census-6ae`.

Run only after the coordinator allocates the existing exclusive target and
compiler resources. This source checkpoint has **not** executed the gate.

The integrated default/WAL successor offers a full local runtime driver after
explicit coordinator resource allocation. It uses the released existing target
and an oracle whose exact hash is supplied from preserved build/source evidence.
For the retained Linux census oracle that hash is
`a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`.
Provide explicit CARGO_HOME/RUSTUP_HOME/GOMODCACHE/GOCACHE SDK cache paths and
the pinned Go1.26.7/Rust SDK PATH, and run under the lifecycle subreaper:

```sh
python3 /tmp/symaira-subreaper.py python3 scripts/memory-historical-oracle/validate.py \
  --go /workspace/oracles/symbrain-go-dcddcef0 \
  --go-sha256 a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41 \
  --go-source /workspace/oracles/daemon772-go-source \
  --target ALLOCATED_EXISTING_TARGET \
  --actionlint /workspace/toolchains/bin/actionlint \
  --output /tmp/NEW_SOURCE_BOUND_REPORT_ROOT
```

This is preparation, not authorization to use another lane's target or port.
The driver preserves raw failed stages, checks zero test failures/ignored tests
and all expected historical function names, records actual Cargo executable
hashes, then builds the production CLI explicitly before process comparisons.
Its isolated child HOME/XDG environment contains no operator credentials; cache
paths are the approved SDK inputs rather than application data. The complete
driver has not run at this source checkpoint.

```sh
python3 scripts/memory-historical-oracle/replay.py \
  --go /workspace/oracles/symbrain-go-dcddcef0 \
  --go-source /workspace/oracles/daemon772-go-source \
  --native "$CARGO_TARGET_DIR/debug/symbrain" \
  --output /tmp/memory-historical-649-clean-head
```

The output directory must not exist. Each child gets disposable HOME/XDG roots,
an empty PATH and an absent Go fallback. Every raw stdout/stderr/exit and complete
SQLite snapshot is retained. Original UUIDs/timestamps and relation triples are
exact; only newly generated blank relation IDs are bound separately to UUIDv4
values. New ledger timestamps must fall within their actual process interval;
existing ledger timestamps are exact. Engine-specific FTS shadow encodings are
retained rather than compared across engines; native repeated opens must retain
every shadow cell. No application-row field is broadly normalized.

`run.sh --output NEW_DIRECTORY` builds frozen Go from a temporary git archive
and runs both the 37-prefix gate and nine actual SQL/child control pairs. They
cover recognized false completion, healthy NULLs, claimed-old FTS, ignored
backfills/markers, unknown FTS, a late index collision, nonempty duplicate IDs
and a custom sync trigger. Each pair retains the actual Go outcome even when
Go's marker shortcut or per-file commit intentionally differs from corrective
native repair. A failed native repair must preserve the entire prior snapshot.

Schema verification combines all Go-owned index/trigger identities, added column
declarations, foreign keys, unique-key collations, CHECK effects and FTS external
integrity. The approved native extra `idx_entity_relations_id` is explicit.
The private Rust tests additionally cover false-marker eligibility, nullable
data preservation, ignored/aborted callbacks, atomic late-index rollback,
unknown FTS/trigger owners, missing constraints and file permissions.

The default correction verifies every owned original/additive declaration before
row repairs or new markers, and again at final verification. Public-opener tests
reject missing/wrong omission defaults with full rollback and preserve the exact
known old query-log default through reopen and real writes. Run the additional
source-bound SQL census with `defaults_inventory.py --output NEW_DIRECTORY`.
It compares all 184 owned columns/62 defaults against the 37 frozen resources
and known native base, and repeats the original three owned oplog SQL invariants.
These Python SQLite controls are not compiled Go/Rust runtime acceptance.

The required allocation-stage commands are:

```sh
cargo test -p symbrain-memory -p symbrain-cli -p symbrain-gateway -p symbrain-mcp --all-targets --all-features
cargo clippy -p symbrain-memory -p symbrain-cli -p symbrain-gateway -p symbrain-mcp --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
actionlint
```

Also run the unchanged Memory CLI `replay.py` (590 Unix/553 Windows plus both
real controls) and `write_gate.py` (60 Set/16 Delete/13 literal fallback/10
callback-output pairs plus three real controls), the existing evidence and DB
oracle tests, and the approved inherited gateway/catalog and HTTP/DOM gates once
their source parents and target ownership are allocated. Obtain exclusive port
11434 ownership before any inherited embedding control. Do not silently merge
an unreviewed UI/catalog branch into this Store checkpoint.

Windows/macOS signature checks are not native runtime evidence. Independent
review, exact-head native three-OS CI and a shipped Rust release remain required;
this checkpoint does not close #649 or #758.

## Completed Linux successor validation

The original source-checkpoint statements above remain historical. Author1fc5910 and independent63fbd320 have now each completed all seventeen allocated Linux stages with392 passing tests,37 historical repairs and37 complete-state reopens. Complete raw observations and original failures remain under `migration/evidence/memory-historical-649`; the independent review is `root-63fbd/review.md`. Actual native macOS/Windows and integrated-head CI remain required.

The native Memory CLI workflow selects Python3.13.7 explicitly. `run.sh` and
direct historical replay also require the actual SQLite checker preflight:
historical REAL defaults must pass integrity, FTS5 porter/integrity must work,
and a genuinely corrupt NOT NULL fixture must fail. Receipts retain the engine
identity. The original host SQLite3.45.1 false-positive failure is preserved;
see `docs/adr/memory-historical-sqlite-checker-803.md`. No product acceptance
criterion is waived by changing the checker interpreter.

The pinned Linux Python distribution dynamically links the host SQLite library;
the Python pin alone does not select a working checker engine. Linux CI prepares
official SQLite3.50.4 in its exclusive `memory-cli-checker-provider` prefix,
verifies the retained ZIP/source hashes and SONAME, and selects it only for the
historical checker/replay/control Python processes. The actual extension binding
and unchanged semantic preflight must pass before historical Go/native children.
The selector is consumed and no loader or Python environment path changes.
Windows and macOS retain their native engines; the pinned macOS extension embeds
SQLite3.50.4 statically. All native semantic and full process gates remain required.
See `docs/adr/owned-sqlite-checker-provider-803.md` for original distribution
identities, the cancelled Windows job and the separately scoped CI job budget.
