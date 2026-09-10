# Scope target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09 package 3a.
It is intentionally kept byte-for-byte compatible with the current Cockpit
source until a separately approved and verified cutover.

- **Source repository:** `symaira-cockpit`
- **Source commit:** `c22a4366a051274ae1651b431ffaf06ae23aa37d`
- **Source path:** `scope/`
- **Receiving path:** `symaira-brain/scope/`
- **Support source:** `history/` at the same source commit, received at
  `symaira-brain/history/`
- **Imported tracked files:** 29 under `scope/`, 14 under `history/`
- **Manifest pins:** `symaira-appkit` exact `0.14.2`; `history` remains a
  local package dependency as in the source package.

The original `symaira-cockpit/scope/` source, `symcockpit scope` dispatcher,
legacy CLI/MCP names, installed binaries, configuration, permissions, TCC
identities and data remain supported and untouched. This is source intake and
an independently runnable target package, not a consumer cutover or release.

The target package keeps Scope optional: no Brain startup path imports or
starts it automatically. Its direct worker surface is the existing `symscope`
CLI and stdio MCP executable, exercised in the target package's own isolated
build/test environment. Brain's existing `symcockpit scope` foreign-server
route remains the compatibility fallback until the package 3a cutover gates
are separately verified.
