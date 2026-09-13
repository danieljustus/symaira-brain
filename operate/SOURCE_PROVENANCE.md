# Operate target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09 package 3 (Operate).

- **Source repository:** `symaira-cockpit`
- **Source commit:** `08077447eb88f9c06ab16b2cee14c4eba2ba60b0`
- **Source subject:** `fix(operate): remove stderr update nag from MCP serve entry point (#266)`
- **Resync date:** 2026-09-11 (second pass, superseding the `528985c` pin below)
- **Source path:** `operate/`
- **Receiving path:** `symaira-brain/operate/`
- **Imported tracked files:** 80 (89 source tracked files minus 9 explicit exclusions) — unchanged from the prior pin; this resync only refreshes file content, no paths added or removed.
- **Source exclusions:** `.swiftlint.yml`, `AGENTS.md`, `CHANGELOG.md`, `LICENSE`, `Makefile`, `NOTICE`, `SAFETY_AUDIT.md`, `assets/branding/product-logo.png`, `docs/assets/social-preview.png`
- **Intentional adaptations:** none; all 80 imported files match source paths, modes, and blobs.
- **Support source:** `history/` at the same source commit, received once at `symaira-brain/history/` (14 files); Operate uses that shared `SymCockpitHistory` package.
- **Manifest pins:** `symaira-appkit` exact `0.14.2`; `history` remains a local package dependency.

This is source intake and an independently runnable target package, not Brain consumer cutover or release. The original Cockpit source, dispatchers, legacy names, binaries, configuration, permissions, TCC identities, and data remain supported and untouched. Brain does not automatically start this package; the existing conservative read-only compatibility route remains unchanged.

**Verification contract:** the receiving tree was compared against `symaira-cockpit` `08077447eb88f9c06ab16b2cee14c4eba2ba60b0` programmatically by tracked path, file mode, and blob hash. The source tree and receiving copy are expected to remain independently distributed; no retirement or routing change is implied.

## Resync history

- **2026-09-11, first pass:** pinned `528985cc21efbd5318e781954c29aec0680a3874` (`test(operate): make permission probes injectable (#265)`); 80 files, none adapted.
- **2026-09-11, second pass:** pinned `08077447eb88f9c06ab16b2cee14c4eba2ba60b0` (`fix(operate): remove stderr update nag from MCP serve entry point (#266)`). This is a fix, not an intake sweep: `symaira-brain`'s own `operate-native-swiftpm` CI job (`scripts/operate-mcp-smoke.py`, a strict stdio smoke check) was failing deterministically because `serve` could write an "Update available" notice to stderr after a background, non-blocking update check landed independently of the JSON-RPC handshake — see PR573 postmerge run `34608846617`, job `operate-native-swiftpm`. Upstream cockpit#266 removed the nag from `serve` (leaving `symoperate version`/`updates check` as the explicit ways to learn about a release) and added regression coverage. Only two files changed and were refreshed byte-for-byte from the pinned tree: `Sources/SymOperateCLI/SymOperateRun.swift` and `Tests/SymOperateSmokeTests/CLITests.swift`. Verified locally in this receiving copy: `swift build --package-path operate` clean; `swift test --package-path operate` passes (311 `SymOperateCoreTests` + 14 `SymOperateSmokeTests`, 0 failures); `python3 scripts/operate-mcp-smoke.py <built symoperate> --timeout 10` now exits 0 (previously reproduced the exact CI failure text against the pre-fix binary).
