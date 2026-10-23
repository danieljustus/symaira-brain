# Standalone Guard migration inventory (#770)

Reference: unchanged Go `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, `cmd/symbrain/cmd_guard.go`, and all public command packages in `guard/cmd/symguard/`. The frozen tree contains no standalone Go main. The supplemental oracle assembles those actual handlers into a process for the new standalone presentation.

| Reachable path | Native owner | State |
|---|---|---|
| no arguments, help, --help, -h, unknown command | symguard-cli standalone | native; standalone spelling |
| version, version --json | shared Guard version | native; compact handshake, double newline, honest Rust SDK |
| decide, decide help | shared Guard decide + guard-core | native bounded request and private audit; proven Unix directory audit-open diagnostic matched; Windows wording pending |
| grants help, list, revoke ID, revoke --all | shared Guard grants | native; real store/exit/output behavior |
| scan, format/help/error flags | shared Guard scan | native existing discovery/redaction contracts; full malformed input/error boundary audit still required |
| doctor | shared Guard doctor | native proven states; complex unsupported states remain explicitly fail closed in standalone and Go-owned in Brain |

`proxy`, `spawn`, `approval`, `proposal`, `sequence`, `discovery`, and `update` are not reachable command cases in the actual dispatcher. Existing corresponding libraries are not silently deleted or represented as ported CLI features. Their library coverage is tracked separately; `symbrain-guard-core` already owns policy, approval data contracts, sequence, capability, grants and chain verification. The auditkit chain/checkpoint core remains locally implemented with SEC-002 fixtures; no new CoreKit dependency is introduced.

The expanded process runner executes all124 selected cases rather than skipping unsupported doctor states. Current Linux proof has121 complete output/exit/persistence matches after validated runtime fields. Three selected TOML states assert explicit failure and remain unported: decoder type text before semantic validation, multiple invalid defaults with Go map ordering, and unknown-key warnings. Windows audit-open wording retains the explicit deny-only comparison until native proof supports a safe port. These counts describe the selected corpus, not every remaining diagnostic branch. Unsupported TOML syntax/type/representation, config and anchor I/O, anchor Unicode replacement, malformed discovery boundaries and output failure still need complete inventories/proof. The frozen decision suite's platform-specific broken-output case is not counted here and remains an explicit acceptance task. No zero-unported claim, issue closure, release or Go removal follows from this inventory.

## Machine-checked command tree (slice 1)

`migration/evidence/guard-command-tree-770/inventory.json` is generated from the Go oracle
(`scripts/guard-standalone-oracle/command_tree.py --write-fixture`, frozen `dcddcef0`,
`GOTOOLCHAIN=go1.26.7`) and verified without writing by the same script in `run.sh`, which also
runs every probe against the native binary. It records the help-tree walk (nine paths), the
package flag literals (`--format`, `--all`, `--json`, `--help`, `-h`), the dispatcher cases, each
probe's Go stdout/stderr/exit and the classification of all fourteen `guard/internal` packages
(imported by a command / not imported / Rust owner). `symguard-cli/tests/command_tree.rs` replays
the same probes against the Rust process with no Go toolchain. Result on macOS arm64: 9 paths,
9 native, 0 unported, 0 fallback, 80 probes matched, four actual process mutants and three
fixture mutants rejected. Native Linux/Windows runs are pending CI.

The seven `doctor` probes carry a generator-recorded `host_dependent_reason`: on a Windows host the
Go doctor itself exits 1 for an empty HOME (missing client-config parent directories are
`ERROR_PATH_NOT_FOUND`, which Go's string-based `containsNotExist` does not skip), so the Unix
capture is only byte-compared on Unix; off Unix the replay checks exit 0/1, a doctor report and empty
stderr, still byte-compares the other 73 probes, and the Windows doctor output is covered by the
host-recorded doctor oracle.

Library-only state (not command paths): `approval`, `capability`, `policy`, `sequence` are not
imported by any command and are owned by `symbrain-guard-core`; `archguard` and `proposal` are
not imported and have no Rust port; `spawn` is used only by `doctor` and is ported inside
the native doctor; `update` is imported by `version` but `CheckLatest` is called by no command
(the frozen tree has no `main`), so there is no network update check to port.

See [ADR 0011](../docs/adr/0011-standalone-rust-guard.md) and [reproduction](../scripts/guard-standalone-oracle/README.md).



The actual #805 macOS job111449257217 first rejected the Rust Doctor test's
owned invalid-UTF8 `fs::write` with EILSEQ92. The bounded source successor accounts
for only that exact kernel-unavailable component/operation and the connected
source-derived raw audit-directory fixture; it preserves every admitted role,
help assertion, raw comparison and existing Python gate/control. A native record
must distinguish UNEXECUTED inputs from parity and prove zero children and owned
cleanup. This is source preparation, not a new native pass. See
[the fixture decision](../docs/adr/guard-rust-kernel-fixture-admission-805.md).
