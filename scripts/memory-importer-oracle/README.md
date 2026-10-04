# Importer source checkpoint (#761)

This directory is **prepared, not executed**. Neither a successful Go baseline
nor a native compiler/process result is claimed. Runtime remains unallocated.

`main.go` calls the frozen module's actual five public importer constructors,
discovers references and imports each twice. `importers761_oracle.rs` calls the
new native library. Both retain full ordered reports, raw path/content/metadata
hex, exact spans, timestamps, errors and nil-versus-empty result slices.
`compare.py` checks the entire report without normalization and rejects an
empty or missing positive bootstrap for any family. The 70 owned recipes cover
five baseline classes per family plus source-specific boundary/history cases.
The original Go testdata are retained separately in the source preparation
evidence and are not rewritten to match the new implementation.

After Root allocates an existing exclusive target and a Go cache, run `run.sh`
under the owned `/tmp/symaira-subreaper.py` with all four required allocation
variables. It copies the exact frozen commit to private scratch, adds only the
oracle caller, uses an owned HOME/XDG and TZ=UTC, and checks all fixture paths,
bytes, modes and mtimes before/after the constructor processes. It does not
invoke any provider, email client or real credential lookup. No target is
created implicitly. Allocate a bounded timeout and check the 700 MiB floor
before builds; archive actual binaries/hashes before later reuse.

After a genuine passing baseline, `mutants.py` prepares three separate input
controls. Run the same actual native executable on each modified packet and
compare against the original Go output/input. Each native process must exit
successfully with all 70 reports, and the comparator must reject an actual
full-report difference. A compiler/import/bootstrap failure is not a mutation
success. Preserve every original input, stdout/stderr/exit and readonly state
receipt before running controls. These controls are currently **not run**.

Additional required gates before acceptance are the prepared 24 native unit
functions, shared Store marker and Activity retention actual-Go process pairs
(including rollback and committed-count/cleanup errors), generated HOME/raw
path ownership, typed I/O diagnostics, native Windows path/default-HOME/UTF16
contracts, non-UTC local timestamps, timestamp-domain overflow, all previous
Memory/Activity/CLI/MCP suites, strict Clippy/fmt and native three-OS CI. No new
CLI/HTTP/MCP route is admitted by this checkpoint. The ten remaining importer
families and the registry's extraction/governance flow are still pending.
