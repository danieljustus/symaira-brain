# Guard Rust fixtures: account for the kernel's exact filename domain (#805)

The complete current macOS job111449257217/run37206698770 first failed at
`doctor_warnings.rs:68`: the owned `fs::write` for bytes
`6f776e65642de2823c263e` (`owned-` plus incomplete UTF8 plus `<&>`) returned
EILSEQ92 before the raw Doctor role ran. The full215177-byte job log, original
content JSON, original test bodies/native metadata and all4057 parent Git
body/mode bindings are preserved before this correction. Parent cdb2aca remains
immutable. This is fixture admission evidence, not a Guard product failure.

Decision: only the exact owned raw component, actual failing create operation,
and observed errno92 on macOS can be reported as UNEXECUTED-kernel-unavailable.
Linux retains every raw case. Windows retains its four original Doctor roles
and Unix-only audit boundary. Every other errno, component, changed parent,
unproved cleanup, duplicate or missing case remains a failing test. There is no
new ignore, timeout, dependency, production selector or diagnostic projection.

The warning test still actually verifies healthy, semantic, type and discovery
roles, then attempts the original raw write. Completed roles enter the accounting
only after the original child/result/read-only assertions. A rejected raw name
is logged separately with full OS bytes, operation/error, before/after entries,
zero children for that case, requested/executed/unavailable IDs and strict root
removal. The helper emits direct stderr I/O so a passing libtest does not hide
the native admission record in its normal output capture. UNEXECUTED is never
represented as raw parity, and the original requested domain remains incomplete.

`raw_diagnostics.rs` contains the same component in its owned `fs::create_dir`.
The failed job did not reach it; its inclusion is a connected source-derived
fixture correction, not a second observed CI failure. Its independent help
assertions now precede that creation and still execute. If the actual kernel
rejects the name, all five raw audit-risk inputs and the invalid-JSON input are
accounted as UNEXECUTED with zero product children; otherwise every original
raw byte/deny/error/JSON assertion executes unchanged.

The existing Python kernel_admission/raw_paths/config_warnings/config_paths
criteria, accounting controls and corpus IDs are unchanged. The Rust helper is
narrower: it admits only this exact connected component, with one of the two
explicit fixture operations. New data-only Rust tests and pure source controls
reject wrong platform/errno/name and omitted/duplicate accounting. Pure checks
are not Rust execution or kernel observations. Native Linux/macOS/Windows whole
workspace/focused runs and independent review remain mandatory. This source-only
successor does not claim new CI passes, release, full #770 parity or closure.
