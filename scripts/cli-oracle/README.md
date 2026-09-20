# cli-oracle

Frozen command-tree and flag behaviour for `CLI-006`, captured from the real
production binary built at the pinned revision (via `run-go-oracle.sh`), never
hand-written. 81 cases: the top-level usage text, `help`/`-h`/`--help`, no
arguments, unknown commands, `version` and its flag boundaries, `config`,
`profile`, `doctor`, `init`, `install`, `uninstall`, `sync`, `memory`,
`skills`, `activity`, `audit` and `guard` — each with the measured stdout,
stderr and exit code, including the negative cases (unknown flag, missing flag
value, extra positional, bare `-`, `--` terminator).

```sh
# regenerate (pinned toolchain, from the repository root; needs a Go binary)
GOTOOLCHAIN=$(awk '$1 == "go" { print "go" $2; exit }' go.mod) \
  CGO_ENABLED=0 go run ./scripts/cli-oracle \
  -go-binary "$(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go"

# verify the tracked fixture still matches the pinned revision
make cli-oracle-check
```

## Isolation (why this oracle is safe to run)

An earlier version of this oracle ran the binary with the ambient environment.
It executed `setup`, which downloaded and installed managed binaries into the
operator's real `~/.symaira/bin`, ran `doctor --fix`, and let `sync` write
managed blocks into this repository's `AGENTS.md`. All three are fixed:

- **Environment**: every case runs with `HOME` and all four `XDG_*` roots
  pointing into a throwaway directory, so no case can read or write the
  operator's config, profiles or managed binaries.
- **Working directory**: every case runs in a scratch directory, so commands
  that write project files (`sync`) cannot mutate the checkout.
- **Dropped cases**: the bare `setup` and `doctor --fix` cases were removed
  entirely. They have real side effects (network downloads, binary
  installation) and their output is environment-dependent by construction, so
  they cannot be frozen. `setup --unknown` and `doctor --unknown` stay: they
  only exercise flag parsing.

## Normalization

Placeholders are substituted before the fixture is written, because the real
values differ per invocation or per machine:

- `<root>` — the per-invocation isolation root (both `/var/...` and
  `/private/var/...` forms).
- `<repo>` — the checkout location, which differs per machine and CI runner.
- `<go>` and `<os/arch>` — the building toolchain and host platform printed by
  `version`. These are **accepted differences**, not verified matches: a Rust
  binary reports its own toolchain and can never print a Go version.

Everything else is compared byte-for-byte.

## Status: fixture only — no Rust consumer yet

`CLI-006` stays non-complete. No Rust test loads
`rust/symbrain-cli/tests/fixtures/cli_tree_expectations.json` yet, so the row is
`fixture-ready`, not green. `migration/cli-tree-inventory.md` carries the
readable inventory that goes with this fixture.

Produced by a delegated worker that was cut off before writing the Rust
comparison test; the isolation, the dropped cases and the normalization were
added by the coordinator after the worker's version proved unsafe to run.
