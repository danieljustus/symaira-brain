# Direct CLI write replay

Run `python3 scripts/memory-cli-oracle/write_gate.py --report-dir /tmp/memory-write-proof`
with the native `target/debug/symbrain` built and Go 1.26.7 on PATH. Optional
`--go` and `--rust` select already bound executable artifacts. The runner archives
immutable `dcddcef0`, uses an owned build HOME and records source/SDK/binary hashes.

The current gate runs 60 native set pairs, twelve native delete pairs and twelve
actual delegated-Go boundary observations, plus three executable mutants. Runtime
HOME/USERPROFILE/XDG/cwd are disposable, PATH is empty, credentials/proxy variables
are not inherited and every embedding endpoint is owned. Existing Go production
and frozen fixtures remain unchanged; the `.go.txt` helpers are additive wrappers.

Each set/delete pair starts from the same actual Go-created database backup at
the same path. Full raw SQLite rows/blobs are retained. Generated UUIDs/timestamps
have explicit validation/binding receipts; all other application values stay
literal. FTS shadow bytes are retained and hashed but are not compared as an
application contract: logical FTS rows and integrity checks cover the index.

The set samples exercise canonical kinds/aliases, staging, scope (including empty),
empty/Unicode authors, metadata overrides/null/duplicates/escaping, project/Git
boundaries, name-before-alias resolution, reused/new/malformed-alias entities,
configured embeddings, wrong dimensions/cardinality, overflow and hash fallback,
float32 scientific boundaries/negative zero, binary sign bits and config layering.
The delete samples cover both output formats, creator/session audit attribution,
missing IDs, retained rows, surviving entity/association rows, evidence cascade
and the pre-delete access update's sync event. Routing refuses malformed hydration
instead of deleting a row Go cannot read.

The identity mutant returns another syntactically valid UUID absent from the
actual database. The audit mutant changes only the real stored set audit actor.
The exit mutant changes the successful exit. All execute the real candidate;
the runner requires the intended assertion to fail and keeps literal outputs and
state. Boundary helpers execute actual Go and record its result before forwarding
it; native output/exit must equal that exact delegated result. Delegated cases
never count as native set/delete proof.

Python SQLite connections are explicitly closed after transaction contexts so
Windows temporary-directory removal cannot race a retained handle. JSON receipts
are written as UTF-8 bytes with LF on all platforms. Linux evidence does not claim
native macOS/Windows acceptance; the existing three-OS workflow runs this gate.
