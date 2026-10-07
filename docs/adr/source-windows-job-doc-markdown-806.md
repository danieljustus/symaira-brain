# Source Windows owned-job documentation lint

The published Source parent d0b58ab03f77e847cd92862f0332f5ebc4952b44
remains immutable. Two complete native CI logs, including original CRLF bytes,
were retained and round-trip SHA-checked before editing the isolated successor.
The original helper and complete parent Git body/mode map are retained alongside
native metadata. The logs are from jobs 111449738122 and 111449738205; their
actual checkout/integration identities remain visible in the full original logs
and are not silently relabeled as a locally executed d0b worktree.

The first strict-check failure identifies `setup_source_windows_job.rs:1`:
Clippy's `doc_markdown` rule requires the identifier `JobObject` in backticks.
The successor applies precisely that suggested documentation change. It adds no
lint allowance, alters no helper or lifecycle behavior, and changes no test,
corpus, control, deadline, workflow, dependency or frozen Go source. The complete
rest of the helper is byte-identical to the published parent. This fixes the
source of the observed documentation lint; it does not assert a new native run.

Later missing artifacts in these jobs are downstream of the earlier failed
strict gate. They do not establish a separate owned-job or process failure.
Prior Source runtime/ownership findings and their independent evidence remain
attributed to their original source; this comment-only correction does not
reinterpret them or waive any runtime assertions.

Standalone Rust formatting, exact old/new source delta, complete parent mode/
blob identity outside this one comment and lossless log/source retention are
source checks. No local Cargo, compiler, SDK, product, target/cache or endpoint
was used. Independent focused review and fresh required native strict/runtime
CI remain necessary before publication/merge. The Brain78f and Doctorca review
candidates remain unchanged; cross-integration requires their completed reviews
and a normal subsequent integration rather than implicit approval propagation.
