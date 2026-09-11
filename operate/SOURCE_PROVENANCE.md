# Operate target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09 package 3 (Operate).

- **Source repository:** `symaira-cockpit`
- **Source commit:** `528985cc21efbd5318e781954c29aec0680a3874`
- **Source subject:** `test(operate): make permission probes injectable (#265)`
- **Resync date:** 2026-09-11
- **Source path:** `operate/`
- **Receiving path:** `symaira-brain/operate/`
- **Imported tracked files:** 80 (89 source tracked files minus 9 explicit exclusions)
- **Source exclusions:** `.swiftlint.yml`, `AGENTS.md`, `CHANGELOG.md`, `LICENSE`, `Makefile`, `NOTICE`, `SAFETY_AUDIT.md`, `assets/branding/product-logo.png`, `docs/assets/social-preview.png`
- **Intentional adaptations:** none; all 80 imported files match source paths, modes, and blobs.
- **Support source:** `history/` at the same source commit, received once at `symaira-brain/history/` (14 files); Operate uses that shared `SymCockpitHistory` package.
- **Manifest pins:** `symaira-appkit` exact `0.14.2`; `history` remains a local package dependency.

This is source intake and an independently runnable target package, not Brain consumer cutover or release. The original Cockpit source, dispatchers, legacy names, binaries, configuration, permissions, TCC identities, and data remain supported and untouched. Brain does not automatically start this package; the existing conservative read-only compatibility route remains unchanged.

**Verification contract:** the receiving tree was compared against `symaira-cockpit` `528985cc21efbd5318e781954c29aec0680a3874` programmatically by tracked path, file mode, and blob hash. The source tree and receiving copy are expected to remain independently distributed; no retirement or routing change is implied.
