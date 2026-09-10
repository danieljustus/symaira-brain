# Scope target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09 package 3a.
It preserves Cockpit source behavior and tracked-file inventory, with exactly
these two intentional, reviewed deviations from the source commit:

1. `scope/Sources/SymScopeMCP/SymScopeMCPServer.swift` explicitly advertises
   `capabilities.tools.listChanged` as `false` in the MCP initialize response.
2. `scope/Tests/SymScopeMCPTests/SymScopeMCPTests.swift` tests that explicit
   MCP capability metadata and the receiving copy's seven-tool contract.

These two files are not byte-identical to Cockpit; all other imported tracked
files are expected to match the source commit. The source commit and imported
file counts below are the comparison/hash provenance convention for this
intake.

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
