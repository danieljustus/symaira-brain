# Operate target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09 package 3 (Operate).

Every imported file is byte-identical to the source commit — zero deviations,
unlike the Scope intake (`scope/SOURCE_PROVENANCE.md`), which had two
intentional, reviewed changes. This copy makes none.

- **Source repository:** `symaira-cockpit`
- **Source commit:** `528985cc21efbd5318e781954c29aec0680a3874`
- **Source subject:** `test(operate): make permission probes injectable (#265)`
- **Resync:** 2026-09-11; native permission probes remain the production
  default, while injected fixtures allow device-free denial tests.
- **Source path:** `operate/`
- **Receiving path:** `symaira-brain/operate/`
- **Support source:** `history/` at the same source commit, already received
  at `symaira-brain/history/` by the Scope intake (#551) — Operate depends on
  the same `SymCockpitHistory` product, no second copy.
- **Imported tracked files:** 80 under `operate/` (`Package.swift`, `.gitignore`,
  `README.md`, `Sources/`, `Tests/`, and three of the four `docs/*.md` files —
  `docs/assets/social-preview.png` and the repo-root boilerplate
  `CHANGELOG.md`/`LICENSE`/`NOTICE`/`Makefile`/`AGENTS.md`/`.swiftlint.yml`/
  `SAFETY_AUDIT.md`/`Package.resolved` were not carried over, matching the
  curation the Scope intake already established).
- **Manifest pins:** `symaira-appkit` exact `0.14.2` (identical pin to
  `scope/Package.swift`, no divergent-exact-version conflict); `history`
  remains a local package dependency as in the source package.

The original `symaira-cockpit/operate/` source, `symcockpit operate`
dispatcher, native permission/signing identity, TargetIdentity binding, safe
fields, cancellation, redacted opt-in history, submitted-versus-confirmed
effect semantics, legacy CLI/MCP names, installed binaries, configuration,
permissions, TCC identities and data remain supported and untouched. This is
source intake and an independently runnable target package, not a consumer
cutover or release.

The target package keeps Operate optional: no Brain startup path imports or
starts it automatically. Brain's existing `symcockpit operate` foreign-server
route (currently exposing only the three read-only tools `version`,
`permissions_status`, `get_policy`, per the operator brief's explicit
restriction) remains the compatibility fallback until the package 3 cutover
gates are separately verified — this intake does not change that exposure.

**CI verification scope, deliberately narrower than Scope's:** the operator
brief's acceptance criteria for Operate say "controlled fixture apps only; no
live desktop automation as a migration smoke test." `Tests/SymOperateCoreTests`
and `Tests/SymOperateSmokeTests` exercise real ScreenCaptureKit/Accessibility
APIs (with graceful `permissionDenied`/`unavailable` fallbacks written for a
sandboxed CI runner with no TCC grants, but still real device I/O attempts).
The `operate-native-swiftpm` CI job compiles the package and every test
target with `swift build --build-tests`, then executes only
`PermissionServiceTests`. These tests inject every TCC/prompt/Settings
operation, including the MCP `permissions_status` route; no native
permission request or desktop capture/input runs. CI also performs protocol-level
checks that touch no device state: CLI `--version --json` output and stderr
hygiene, and native MCP `initialize`/`tools/list` over stdio. Running the full
live-automation test suite is left to a later, explicitly authorized
Operate cutover verification pass with controlled fixtures, per the brief.
