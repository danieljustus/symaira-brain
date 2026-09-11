# Scope target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09 package 3a (Scope).

- **Source repository:** `symaira-cockpit`
- **Source commit:** `528985cc21efbd5318e781954c29aec0680a3874`
- **Source subject:** `test(operate): make permission probes injectable (#265)`
- **Resync date:** 2026-09-11
- **Source path:** `scope/`
- **Receiving path:** `symaira-brain/scope/`
- **Imported tracked files:** 29; all source tracked paths are present.
- **Intentional adaptations (2):** `scope/Sources/SymScopeMCP/SymScopeMCPServer.swift` explicitly advertises `capabilities.tools.listChanged` as `false`; `scope/Tests/SymScopeMCPTests/SymScopeMCPTests.swift` tests that metadata and the receiving copy's seven-tool contract.
- **Support source:** `history/` at the same source commit, received once at `symaira-brain/history/` (14 files); Scope uses that shared package and no second copy.
- **Manifest pins:** `symaira-appkit` exact `0.14.2`; `history` remains a local package dependency.

All other imported Scope files match source paths, modes, and blob hashes. This is source intake and an independently runnable target package, not Brain consumer cutover or release. The original Cockpit source, dispatchers, legacy names, binaries, configuration, permissions, TCC identities, and data remain supported and untouched. Brain does not automatically start this package; the existing compatibility route remains unchanged.

**Verification contract:** the receiving tree was compared against `symaira-cockpit` `528985cc21efbd5318e781954c29aec0680a3874` programmatically by tracked path, file mode, and blob hash, with only the two listed reviewed adaptations excluded from byte equality.
