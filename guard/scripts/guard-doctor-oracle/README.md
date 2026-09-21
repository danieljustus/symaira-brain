# guard-doctor-oracle

Frozen production output of the Go `symguard doctor` command, captured through
`symbrain guard doctor`.

`main.go` imports the production package
`github.com/danieljustus/symaira-brain/guard/cmd/symguard/doctor` and calls
`doctor.Run(&buf)` with an injected `HOME`, `XDG_CONFIG_HOME`, `XDG_DATA_HOME`
and `SYMGUARD_CONFIG`, so the fixture is the real command's bytes and not a
re-implementation. Seven cases: empty machine, healthy config (1 rule + 1
spawn allowlist entry), invalid TOML, audit log without anchor, audit log with
a corrupt anchor, a discovered Cursor MCP server that is not on the allowlist,
and a discovered server whose environment carries a literal secret value.

```sh
# regenerate the fixture (pinned toolchain, from the repository root)
GOTOOLCHAIN=$(awk '$1 == "go" { print "go" $2; exit }' go.mod) \
  CGO_ENABLED=0 go run ./guard/scripts/guard-doctor-oracle

# verify the tracked fixture still matches the pinned Go revision
make guard-doctor-oracle-check
```

## Normalization (why the fixture is guardable at all)

Two parts of the production output are not stable across invocations and are
replaced with placeholders before the fixture is written:

- **Fixture root paths** → `<root>`. `doctor` prints absolute paths of the
  temporary roots it inspects, and every invocation uses a new `mktemp` root.
- **The toolchain line** → `Go:        <go>`. It is `runtime.Version()` of the
  Go binary that built the command, so a byte fixture would drift with every
  Go patch release (`go1.27.1` locally versus `go1.26.7` under
  `run-go-oracle.sh`, which pins the toolchain from `go.mod`).
- **The platform line** → `OS/Arch:   <os/arch>`. It is `runtime.GOOS`/
  `GOARCH` of the machine running the command; the fixture was recorded on
  macOS/arm64 and a Linux CI runner legitimately reports something else.

`Go:` and `OS/Arch:` are therefore **accepted differences**, not verified
matches: a Rust binary reports its own toolchain — it prints the row as
`Rust:` with the real `rustc` version, never a fabricated Go one — and its own
real platform. Every other line is compared byte-for-byte.

## Status: the Rust verb is ported, five of seven cases natively

`rust/symbrain-cli/src/guard_doctor.rs` implements `doctor` natively —
`guard/internal/config`'s path resolution, TOML schema and `validate`,
`guard/internal/spawn`'s allowlist matching, `guard/internal/discovery`'s
client sources, parsing and plaintext-secret heuristic, and
`guard/internal/audit`'s anchor probe. Five of the seven cases here
(`empty_machine`, `healthy_config`, `audit_log_without_anchor`,
`discovered_server_denied`, `discovered_server_secret_risk`) are produced by
the Rust binary and compared against this fixture byte-for-byte by
`rust/symbrain-cli/tests/guard_doctor_oracle_tests.rs`, which runs with no
`SYMBRAIN_GO_BINARY` and an empty `PATH`.

The remaining two fall back to Go **before any byte is written**, because both
print another library's error text verbatim and no Rust re-implementation can
reproduce it honestly:

- `config_error` — `BurntSushi/toml`'s parser message
  (`toml: line 1: expected '.' or '=', but got '[' instead`);
- `audit_log_corrupt_anchor` — `encoding/json`'s message
  (`invalid character 'o' in literal null (expecting 'u')`).

The same gate covers every unreproducible state beyond these two fixtures: a
config that fails `validate`, an unreadable file, a discovery source that
exists but does not parse, and a single server carrying more than one
plaintext secret key (Go emits those in `EnvKeys` order, which comes from a Go
map range and is not stable between two Go runs — see the defect note below).

An earlier revision of this file claimed `discovered_server_secret_risk`
"currently produces the same `[DENIED]` line as `discovered_server_denied`"
and that the plaintext-secret warning "appears to require a server that is on
the spawn allowlist". That was wrong: `printSecretRisks` in `checks.go` does
not look at `Allowed` at all, and the frozen fixture's secret case does emit
the `Plaintext secret risk:` block and the symvault advisory line alongside the
`[DENIED]` line. The corpus has no such gap and the case needs no change.

Known Go-side defect, not fixed here (the fixture is frozen and only exercises
one secret key per server, so it is not observable in this corpus):
`mcpcfgkit`'s scan fills `Server.EnvKeys`/`EnvValues` with `for k, v := range
env`, so a server with two or more plaintext secret keys produces a
nondeterministic key order in doctor's `Plaintext secret risk:` line — two Go
runs on identical input can disagree.

A previous attempt hardcoded the Go lines into a Rust `doctor` module; that is
not a port and was discarded.
