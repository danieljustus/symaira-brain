# Historical Go record and differential paths (#1004)

The Go implementation was removed in #1003. The last tree that still contains
it is `57c26c76e238e458d9027d5ee4362df24d30b47e` (the parent of the removal
commit). Observable Go behavior is pinned by sealed fixtures
(`scripts/oracle-freeze`, `migration/evidence/oracle-freeze-781`); the replay
tooling compares the Rust binary against them and needs no Go.

Scripts that still *record* from, or run *live* against, a Go binary are kept as
history. Each carries a `HISTORICAL RECORD MODE (#1004)` banner naming that
revision. None is run by `make rust-check parity-smoke init-differential` or by
a workflow. Do not change sealed fixtures to follow them; a deliberate
re-record needs a pre-removal checkout, Go, and a reviewed re-seal
(`make oracle-freeze-seal`).

## Fenced under `scripts/`

| Path | What needs Go | Supported replacement |
|---|---|---|
| `scripts/rust-differential.py` `record` and live `GO_BINARY RUST_BINARY` modes | a Go `symbrain` binary | `replay` (`make parity-smoke`) |
| `scripts/init-oracle/compare.py` `record`/live (`build_go_reference_binary`) | `go build` of the pinned commit's sources | `replay` (`make init-differential`) |
| `scripts/mcp-differential.py` | a Go `symbrain` binary | gateway/mcp oracles replay |
| `scripts/memory-cli-oracle/replay.py`, `controls.py` | `go build` of the reference and a control wrapper | sealed `memory-cli` fixtures |
| `scripts/skills-native-oracle/raw_layer_oracle.py`, `tools_oracle.py` | `go build ./cmd/symbrain` (now fail fast without `go.mod`) | sealed `skills-native-oracle` fixtures |
| `scripts/guard-standalone-oracle/run.sh`, `windows_path_reference.py` | `go build` of copied Go handlers | sealed guard fixtures |
| `scripts/config-harness-oracle/run.sh` | `go build` (its helper `run-go-oracle.sh` is gone) | sealed `config-harness` fixtures |
| `scripts/memory-http-oracle/replay.py`, `dom.py` | frozen Go binary and the memory web console assets, retired in #763; both now exit immediately | none; the console is gone |

Removed outright: the Go-compiled Windows stub builder in
`scripts/rust-differential.py`. Native Windows is not a parity target (#990,
#992).

## Provenance field `go_toolchain`

`go_toolchain` is a field name inside sealed provenance and fixture metadata
(`scripts/oracle-freeze/freeze.py`, `scripts/guard-standalone-oracle/command_tree.py`,
`guard/scripts/guard-decide-oracle/*`). It records which toolchain produced the
evidence and cannot be renamed without re-sealing. A grep for `go_tool` matches
it; that is expected.

## Not covered by this change

Outside the paths this change was allowed to edit, and still to be deleted or
fenced the same way:

- `browse/port/harness/*.go.in` (11 files) and `daemon_state_key.py`,
  `daemon_provider_discovery.py`, `daemon_registry.py`, `daemon_process.py`,
  `daemon_mcp.py` with their `--go`/`--go-tool` modes; `browse/port/bench`.
- `guard/scripts/guard-scan-oracle/oracle.py` (`go build` in record mode).
- `migration/evidence/**/*.go.in` (4 files) are sealed evidence and stay as is.
