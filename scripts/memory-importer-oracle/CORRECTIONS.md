# Three independent source corrections (#761)

Everything here remains prepared, not executed. The original README, 70 fixture
recipes, actual callers, comparator and three original controls are unchanged.
See `docs/adr/memory-importer-local-contract-corrections-761.md` for the complete
rejected-review retention, decision rationale and remaining admission limits.

After Root separately allocates a full materialized runtime source, exclusively
owned existing target, Go cache and guarded subreaper/timeout/disk resources:

1. Build the unchanged `main.go` public constructor caller in a private copy of
   the exact frozen Go module. Build `retention.go` as a separate private command.
   Build the two native examples `importers761_oracle` and `retention761_oracle`
   against the exact current source. Preserve all binary bytes/SHA/source maps.
2. Set `IMPORTER761_RUNTIME_ALLOCATED=1`; run `correction_gate.py` with all four
   executable paths and a new owned `/workspace` `--work-root`. It compares 93
   complete literal constructor reports and readonly states, 12 real-Go-seeded
   retention pairs with full errors/counts/schema/rows, and the three new controls.
   These counts must be reached by actual complete successful processes. There
   is no expected-data normalization or bootstrap-failure acceptance.
3. Run all three unchanged original `mutants.py` controls against the original
   Go input/output and the actual current native binary; require all 93 reports,
   successful native exits and intended complete-report comparator failures.
   Run the 32 prepared unit functions, all ordinary/strict affected packages and
   the full prior Memory/Activity/CLI/MCP source-bound parent gates. Native three
   OS CI and different-author full review remain required before admission.

The sparse checker `verify-corrections-static.py` binds all parent index paths
and actual materialized bytes without checking out the 117 MB parent tree. It
preserves original source maps and permits only the five local owner/test paths
and three ledger paths to change. Its own recursive receipt is excluded from
its content map and is bound separately in the final handoff. A successful
static check is not a passing Go/native operation or compiled test.
