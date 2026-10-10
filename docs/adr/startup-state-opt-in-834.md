# Startup state and history opt-in (#834)

**Status:** approved target contract. **Foundation:** exact PR #813 base `2f6880815a4aa65c4a2d7810240ef0cfd1effe4d`. This contract changes the candidate only; the immutable Go oracle at `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` and its captured filesystem effects remain unchanged.

## Effective startup rules

- A disabled profile memory core is absent before memory configuration, SQLite, or JWT initialization. It creates no memory database, authentication key, or memory startup directory. An enabled profile continues to expose and initialize the native memory implementation; Rust does not create the legacy Go JWT key.
- Audit is enabled only when both the selected profile and the resolved global `audit.enabled` setting enable it. A profile opt-out is authoritative even when the global configuration defaults to enabled; a global opt-out also vetoes a profile opt-in. Rust reads the same global config file and `SYMBRAIN_AUDIT_ENABLED` override before starting child backends.
- Episode capture is metadata-only and explicitly opt-in. Go `patterns.enabled` defaults to false; a TOML or environment `true` enables capture, `false` disables it, and an absent setting does not create an episode file. The existing read-only `patterns` tool and the on-disk history schema remain unchanged. Rust receives no copied Go episode writer under this issue.

These rules govern Brain capability/startup state only. They do not add Guard per-call conduct policy, authorization prompts, or approval behavior.

## Legacy/target acceptance boundary

The frozen Go process is expected to retain its historical startup behavior: memory initialization is unconditional, audit is enabled by either profile or global config, and absent `patterns.enabled` defaults to recording. Those observations are evidence, not desired parity. The native target uses profile-gated memory, profile-and-global audit, and no automatic history capture.

`scripts/skills-native-oracle/startup_controls.py` adds five actual Go/Rust process pairs: profile audit opt-out against global opt-in with absent history config; explicit audit and history opt-ins; global audit veto; explicit history false; and enabled-memory positive control. It retains raw stdout/stderr/exit/PID and complete filesystem entries/modes/raw file SHA-256 observations. The classifier accepts only the named startup-owned paths (`config/symbrain/memory`, `data/symbrain/memory`, `data/symbrain/audit`, and `data/symbrain/recipes`) after checking each case's exact owner/profile-derived presence, absence, type, private file mode, shared-path modes, and legacy Go file evidence. The Rust target must have no disabled-memory key/database, no disabled audit log, and no episode history. Any other file, mode, link, response, or filesystem difference remains a failure; Skills cache/render contents and hashes are never projected away.

The process controls and Rust integration regressions are source-prepared here. Rust/native process acceptance remains pending the parent-owned build, comparator execution, and independent review. No CI, native readiness, or publication is implied by the focused Go verification.