# Direct CLI write replay

Run `python3 scripts/memory-cli-oracle/write_gate.py --report-dir /tmp/memory-write-proof`
with the native `target/debug/symbrain` built and Go 1.26.7 on PATH. Optional
`--go` and `--rust` select already bound executable artifacts. The runner archives
immutable `dcddcef0`, uses an owned build HOME and records source/SDK/binary hashes.

The current gate runs 60 native set pairs, sixteen native delete pairs and thirteen
actual delegated-Go boundary observations, plus three executable mutants. Runtime
HOME/USERPROFILE/XDG/cwd are disposable, PATH is empty, credentials/proxy variables
are not inherited and every embedding endpoint is owned. Existing Go production
and frozen fixtures remain unchanged; the `.go.txt` helpers are additive wrappers.
The default `localhost:11434` endpoint is owned on IPv4 and, when localhost
resolves to it, IPv6 before any invalid-configuration case can use the default.

The failure-state supplement also runs six healthy governance callbacks in both
output formats: ignored kind, ignored staging and removal during the set audit.
It requires exit one, no success reply and the exact Go error bound to the actual
committed audit ID. Complete application state, generated-time relations and FTS
integrity must match. Hosts exposing `/dev/full` add two nonempty-metadata Set
pairs with literal OS error diagnostics and preserved committed staged writes.
Other native hosts still execute the portable Rust failing-writer test; the
receipt states whether a real Unix sink was exercised.

Unix hosts also run two actual closed-reader stdout pairs. Both CLIs must die
quietly through SIGPIPE after the complete committed metadata/staging write;
all application state, ID/time bindings and FTS checks remain strict. This
process contract is distinct from portable injected-writer tests, which require
ordinary checked errors and preserve the library caller.

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
and the pre-delete access update's sync event. Real SQLite callbacks also change
audit attribution or remove the row during that access update; replies and audits
must follow the subsequent Go observation. Routing refuses malformed hydration,
including Chrono's accepted leap seconds that Go rejects,
instead of deleting a row Go cannot read.

The identity mutant returns another syntactically valid UUID absent from the
actual database. The audit mutant changes only the returned primary row's real set audit actor;
pre-existing seed audits remain unchanged.
The exit mutant changes the successful exit. All execute the real candidate;
the runner requires the intended assertion to fail and keeps literal outputs and
state. Boundary helpers execute actual Go and record its result before forwarding
it; native output/exit must equal that exact delegated result. Delegated cases
never count as native set/delete proof.

Python SQLite connections are explicitly closed after transaction contexts so
Windows temporary-directory removal cannot race a retained handle. JSON receipts
are written as UTF-8 bytes with LF on all platforms. Linux evidence does not claim
native macOS/Windows acceptance; the existing three-OS workflow runs this gate.

## Deterministic conflict source preparation (not accepted parity)

`conflict_writes.py` is an additive producer wired into `write_gate.py`. It is
SOURCE_ONLY: no new Go capture, native process comparison, compiler, formatter,
Clippy or product test has run for this change. The original captures/pin and
MEM-CLI-002 status remain unchanged. Existing historical counts above are not
counts for this candidate.

The prepared CLI path admits existing-store, extraction-free writes with the
deterministic checker and configured decimal thresholds/integer candidate cap.
Threshold normalization and narrowing to float32 follow frozen Go. Nonempty
verdict providers, Hamming prefiltering during an enabled check, unsupported hex
or nonfinite threshold representations, floating-point candidate caps and unsafe
row hydration retain Go. Staged/disabled writes bypass checking. HTTP/service
admission remains conflict-free; the public `DirectWrite` shape is unchanged.

The producer invokes real pinned Go to create the database and seed rows, backs
it up once per case, then executes both real CLIs from that same state with Go
fallback unavailable to Rust. An owned loopback embedding server supplies
labelled input vectors. For cosine/ranking scenarios, fixture SQL assigns the
query's actual Go-produced LSH bucket to seeds; these are controlled eligibility
inputs, not claims about natural LSH recall. A probe write/delete and all seed
transcripts remain in the report and never count as native parity comparisons.

Cases cover hash repeats (including expired/other-space rows), best near-duplicate
supersession, ambiguous/no-match bands, scope and candidate exclusions, staging,
disabled checking, configured thresholds/caps, post-admission checker failures,
entity/governance effects and feedback/audit/supersession callbacks. Both output
formats are exercised. The producer retains full raw rows/blobs, exact schema,
logical FTS/integrity, requests, replies/exits and explicit UUID/time bindings.
Supersession closure is tied to the actual winner creation timestamp, and repeat
identity/provenance/access must stay unchanged. Failure receipts retain the raw
current/earlier processes and preparation, not a synthesized expected database.
Additional provider/config boundary observations invoke the real Go fallback
wrapper and compare its literal returned transcript; they count as delegation,
never native conflict proof. No verdict request is needed on the empty store.

Review152 corrections preserve the conflict diagnostic stream. The CLI renders
Go's default slog/log timestamp, level, message and ordered quoted attributes;
repeat diagnostics precede entity/governance work, checker errors precede the
unchanged save, and audit/supersession warnings follow their actual callbacks.
Retrieval feedback and ordinary set-audit failures stay silently best-effort.
The HTTP entry point does not install this diagnostic callback or checker.
The additive producer now validates every stderr payload and line order,
including repeat followed by governance failure. It binds only a well-formed
local log timestamp within the process interval and IDs already validated
against retained database state. No diagnostic line or error detail is stripped.

Conflict hydration retains Go's float64 importance until the final composite
score narrows to float32. Added cap-one scenarios force equal Go-produced
embeddings and known recall order, then exercise close ordinary importance
values across a score-rounding midpoint and finite f64 values above f32's range.
They require the higher-ranked seed alone to receive feedback and supersession.
These remain prepared sources, not executed regression/parity counts.

Review154 corrections apply the existing raw-OS-EPIPE boundary to conflict
diagnostics only for the executable's real streams. Unix stderr-closed-pipe
sources cover repeat before entity/kind callbacks and degradation before save;
stdout remains captured, the actual signal/exit and full state are compared,
and the unavailable stderr bytes are recorded as null, not invented empty
output. Injected/raw-EPIPE library writers and non-EPIPE process failures retain
best-effort behavior. HTTP and shared stdout handling remain unchanged.

Strict recall and full-row hydration scan every SQL field in projection order
before decoding metadata and embedding JSON. Prepared post-admission cases
compare metadata-only, time-only and compound errors, including the projected
created_at index 4 versus full-row index 9. Invalid input timestamps stay literal
in state comparison, never normalized as generated times.

### Authorized original boundary integration (source only)

`write_boundaries.py` retains its original `default-conflict` input: `hello world`,
`--kind fact`, `--json`, default conflict settings, an empty Go-created database
and the original embedding handler/model. This is NOT the different additive
`default-empty` case and performs no probe/reseed. It uses the same real pair
comparator as the new conflict suite, with fallback unavailable and strict
output/exit/schema/full logical state/FTS/request comparison plus validated
UUID/time bindings. All other original boundaries still require the actual Go
wrapper receipt and literal delegated output. The report separates one planned
native pair from twelve delegated observations; these are not executed counts.
All original suites, three mutants and raw failures remain; the outer gate
continues independent suites after an error and fails closed. No historical
fixture, pin or capture was rewritten. Boundary source authority was consumed,
not mistaken for runtime acceptance.

Before execution, the scheduler must obtain a fresh resource PASS and grant
slotA/jobs2 through its existing fcntl wrapper and `dev-external`; busy75 is
NOT_EXECUTED. Use external evidence/build/tmp roots, never the historical `/tmp`
example above. Whole-crate/caller tests, fmt, strict Clippy, security/coverage,
actual three-OS native differential evidence and independent exact-candidate
review remain PENDING. This source package grants no publication or cutover.
