# Historical Store repair gate (#649)

This prepared gate runs real frozen Go and native CLI processes against 37
independently copied historical databases. No Go source or frozen fixture is
edited. The original 314-file census, including the first rejected 20-pair
recipe, is retained under `migration/evidence/memory-historical-649/census-6ae`.

Run only after the coordinator allocates the existing exclusive target and
compiler resources. This source checkpoint has **not** executed the gate.

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
