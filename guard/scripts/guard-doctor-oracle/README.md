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

`Go:` is therefore an **accepted difference**, not a verified match: a Rust
binary reports its own toolchain and can never print a Go version. Every other
line is compared byte-for-byte.

## Status: fixture only — the Rust verb is not ported

`symbrain guard doctor` still returns `None` in
`rust/symbrain-cli/src/guard_cli.rs` and therefore runs on the Go fallback. The
fixture exists so the next slice can compare against frozen bytes instead of
re-deriving seven scenarios.

Known gap in the corpus: `discovered_server_secret_risk` currently produces the
same `[DENIED]` line as `discovered_server_denied`; the plaintext-secret
warning appears to require a server that is on the spawn allowlist. The case
should either be given an allowlisted server or renamed, once the port needs it.

A previous attempt hardcoded the Go lines into a Rust `doctor` module; that is
not a port and was discarded.
