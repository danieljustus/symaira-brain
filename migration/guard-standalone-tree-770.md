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

See [ADR 0011](../docs/adr/0011-standalone-rust-guard.md) and [reproduction](../scripts/guard-standalone-oracle/README.md).

The actual #805 macOS job111449257217 first rejected the Rust Doctor test's
owned invalid-UTF8 `fs::write` with EILSEQ92. The bounded source successor accounts
for only that exact kernel-unavailable component/operation and the connected
source-derived raw audit-directory fixture; it preserves every admitted role,
help assertion, raw comparison and existing Python gate/control. A native record
must distinguish UNEXECUTED inputs from parity and prove zero children and owned
cleanup. This is source preparation, not a new native pass. See
[the fixture decision](../docs/adr/guard-rust-kernel-fixture-admission-805.md).
