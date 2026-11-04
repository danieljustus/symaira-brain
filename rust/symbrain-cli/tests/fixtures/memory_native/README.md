# memory_native fixtures

Frozen observations of the shipped Go `symbrain memory` implementation, used by
the Rust tests as the oracle for `list`, `rules`, `search` (including the
Hamming prefilter), `set` (conflict check, LLM verdicts, PII redaction, fact
extraction, entities), `delete` and `serve`. **Tests replay these files and
never build or run Go.**

| Directory | Test | Contents |
|---|---|---|
| `memory_native_read/` | `tests/memory_native_read.rs` | one `<scenario>.json` per scenario plus the Go-produced seed databases (`*.seed.db`, `seed-rows3.db`) |
| `memory_native_set/` | `tests/memory_native_set.rs` | same layout |
| `memory_native_conflict/` | `tests/memory_native_conflict.rs` | same layout, including the requests the fake LLM/embedding servers received |
| `memory_serve_oracle/` | `tests/memory_serve_oracle.rs` | one `<case>.json` per differential case plus `seed.db`/`seed.json` |

Every recording carries a `provenance` block: fixture kind, `go version` of the
oracle binary, its SHA-256, the Go source revision (`git_head` and the tree
hashes of `cmd`, `internal`, `go.mod`, `go.sum`), the platform and the exact
record command; `payload_sha256` guards the payload and (for the read/set/conflict
harness) `seed_sha256` guards the seed database, so an edited fixture fails the
replay. Each scenario payload stores the config, the seed steps, the SQL fault
injection (`mutate_sql`) and the exact argv of every step.

Normalised (documented) per-run bytes: sandbox root, log timestamps, generated
ids and timestamps, retrieval scores and `last_access`/`prev_access` (they blend
in `time.Since(updated_at)`; the ranking order stays compared).

## Re-recording

Only possible while the Go sources exist, from a clean `cmd internal go.mod
go.sum` tree:

```sh
go build -o <dir>/symbrain ./cmd/symbrain
SYMBRAIN_MEMORY_ORACLE=<dir>/symbrain SYMBRAIN_MEMORY_FIXTURE_RECORD=1 \
  cargo test -p symbrain-cli --test memory_native_read --test memory_native_set --test memory_native_conflict
# serve: delete memory_serve_oracle/seed.db first, then
SYMBRAIN_MEMORY_SERVE_GO_ORACLE=<dir>/symbrain SYMBRAIN_MEMORY_FIXTURE_RECORD=1 \
  cargo test -p symbrain-cli --test memory_serve_oracle
```

Recording runs the Go and the Rust binary against each other and refuses to
write a fixture on any difference.
