# Startup state and history opt-in (#834)

**Status:** accepted target contract, ported onto `main` from the preserved
`handoff/20261012-pr813-reviewed` ADR (tracking issue #1002). Only the Rust
`symbrain mcp` behavior is specified here; the removed Go implementation and
its sealed fixtures are unchanged evidence.

## Effective startup rules

- **Memory.** A profile whose memory core is disabled (or exposes no memory
  or activity tool) never opens the embedded store: no memory database, no
  memory directory, no JWT/authentication key. An enabled profile opens the
  store as before. Rust never creates the legacy Go JWT key.
- **Audit.** Audit is on only when the selected profile **and** the resolved
  global `audit.enabled` both enable it. A profile opt-out is authoritative
  even when the global default is enabled; a global opt-out (config file or
  `SYMBRAIN_AUDIT_ENABLED`) vetoes a profile opt-in. A disabled audit creates
  no audit directory or log.
- **Validation order.** The global config file is parsed and its
  `audit.enabled` value checked *before* `SYMBRAIN_AUDIT_ENABLED` is applied
  and before any child backend starts. A malformed file or a non-boolean
  value exits with code 2 even when audit is disabled by profile or
  environment, so an opt-out cannot hide a broken configuration. A missing
  file is fine and still permits the environment override. A non-table
  top-level `audit` key is ignored (distinct from an invalid
  `audit.enabled`). Accepted booleans: `1 t T true TRUE True 0 f F false
  FALSE False`, as a TOML boolean or string.
- **History.** Episode history (`data/symbrain/patterns/<profile>.jsonl`) is
  opt-in. The gateway has no automatic episode writer; the read-only
  `patterns` tool treats an absent file as empty and creates nothing. A
  future episode writer must stay disabled until explicitly enabled.

These rules govern Brain capability/startup state only. They add no Guard
per-call conduct policy, approval prompts or risk classes.

## Verification

`rust/symbrain-cli/tests/mcp_startup_state_tests.rs` runs the real binary in a
disposable HOME/XDG tree and snapshots every created path: disabled
memory+audit leave the tree empty, profile opt-out beats global/env opt-in,
global config/env opt-outs veto a profile opt-in, both-enabled creates the
audit log, enabled memory is the positive control, and invalid global values
fail with exit 2 without creating state. Table tests for the parser live in
`rust/symbrain-cli/src/mcp_cli/audit_config.rs`.

## Deliberately not ported

The branch ADR's Go/Rust process-pair harness (`scripts/skills-native-oracle`
startup controls, Windows ACL probes) compared against the removed Go
implementation and is not reproduced; main has no Go oracle.
