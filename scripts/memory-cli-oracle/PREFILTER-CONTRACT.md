# Hamming search source preparation (#758)

Status: SOURCE_ONLY, not compiled, executed, accepted or published. This additive
family is deliberately NOT part of `replay.py --family all`: original case IDs,
frozen counts, write boundaries and all three write mutants remain unchanged.
After a fresh resource/slot grant and source-bound Go/Rust builds, the existing
producer entry is `replay.py --family prefilter_search --rust <candidate>
--report <owned-external-report>`. Run it under the owning native gate, never as
an authorization to bypass the resource hold. Omitting `--go` builds the real
pinned `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` oracle/CoreKit v0.17.0.
A prebuilt `--go` hash alone is not an accepted compiler-input closure.

## Prepared production path

The real one-shot config loader still merges global TOML, project TOML, then
validated environment values. `hybrid_search.prefilter_enabled` now selects the
new Store entry point in `memory_cli/search.rs`. The staged routing change is
publication-held until differential proof exists. It admits only existing
64-bit-target stores passing both the original whole-table JSON admission and
a conservative full scalar/time/binary/dimension read. Missing/legacy schema,
new-store modes and malformed representations remain delegated, including an
excluded malformed row. Admission is read-only, not atomic against other writers.

The new entry preserves Go's actual order: LSH scan (cap 2000, batches 64),
projected scalar/time scans before metadata JSON, dimension filter, Hamming
selection, selected-only full hydration, default scoring, result limit and
best-effort feedback on returned IDs only. The CLI's Go `SearchMemories` always
supplies default ranking weights; configured service spreading/trust/policy/time
filters do not enter this command. This is NOT service-weight or spreading support.

Hamming uses a fixed 96-byte sign vector (`<= 0`, LSB first, truncated at 768
components, remaining bits zero), unequal length distance 768 and complete
8-byte words only. Selection is distance/original-index ordered when filtering;
no filtering retains original candidate order. Any NULL binary disables the
whole query's prefilter; an empty BLOB is not NULL. The keep count applies
signed machine-int wrapping multiplication by four before minimum 64 and the
candidate-count cap. CLI nonpositive limits become five before the Store call.
Unknown machine width stays delegated. No saturating unsigned limit shortcut.

The existing private writer sign helper is read-only and cannot be called from
retrieval. Its short algorithm is copied with a `ponytail:` boundary comment;
there is no writer change, new dependency or generic options framework.
`search_ranked`, HTTP `search_checked` (including its pre-feedback admission),
and deterministic conflict recall keep their original modes and signatures.
Conflict plus prefilter still delegates. SOURCE155's error ordering, stderr
EPIPE, hydration, f64 ranking and default-conflict boundary sources are preserved.

## Prepared regression and real-process sources

`retrieval_prefilter.rs` contains selection/sign/word/NULL/overflow checks and
an in-memory production Store regression with more than 64 candidates, excluded
versus selected hydration failures, projected scan order and exact feedback.
CLI tests cover conservative admission and signed-limit preservation. None ran
in this source-only card, including the in-memory tests.

The additive producer reuses `replay` isolation/output/snapshot and the existing
write-family full-state/FTS inspector. An actual pinned-Go `list/set/delete`
creates each store and the seed vector, binary and LSH. Explicit cloned fixture
inputs provide 80 eligible rows, or 2010 for the cap case. Both processes receive
independently restored identical stores, controlled localhost embeddings, an
empty PATH and an absent fallback. One labeled malformed-state case verifies
no-Go rejection and then real delegation; it is not native parity evidence.
The server's controlled failure case selects real deterministic hash embeddings,
not operator endpoints or calculated Go answers. Runtime env drops credentials
and proxies; all HOME/USERPROFILE/XDG/TMP roots are disposable.

Prepared cases cover signed/default/overflow limits; distinct/tied distance and
original-index order; NULL/empty/short/95-byte/97-byte binaries; disabled and
empty searches; scope/source/quantization/dimension/LSH/filter/candidate-cap
boundaries; selected versus excluded hydration; projected metadata/time failure
ordering; best-effort feedback failure; file/environment precedence and CLI
independence from service ranking weights. Output bytes and whole exit compare
strictly. Every database table/column/blob and exact schema/trigger text are
recorded. Logical state/FTS compare, with physical FTS shadows retained as raw
observations. Only validated feedback and newly inserted oplog timestamps are
normalized. Feedback IDs, counts, prev_access and unselected rows are asserted.

Failures and raw process records are saved before assertions and re-raised.
An additional real Rust invocation disables filtering on the positive baseline;
the same output/full-state comparator must reject BOTH its output and feedback
state. No self-computed Go result or second acceptance database is introduced.

## Outstanding gates

Independent same-card SPEC/parity_SOURCE_ONLY and QUALITY/security_SOURCE_ONLY
review must bind the cumulative candidate plus the SOURCE155 delta. Prior
approval does not cover the changed retrieval/routing seams.

Compilation, formatting, strict all-target Clippy, full affected memory/CLI/HTTP/
conflict caller tests, coverage/security and actual original/additive process
comparisons remain NOT_EXECUTED. They require fresh scheduler resource PASS,
slot A/jobs 2 through the existing build-slot wrapper then dev-external, stable
external target/HOME/XDG/TMP roots and complete compiler-input provenance.
Busy exit 75 is NOT_EXECUTED, not PASS. Native macOS/Linux/Windows evidence is
still required. No source approval changes that requirement. Integration,
publication, release, fallback removal and Go retirement are separately held.
