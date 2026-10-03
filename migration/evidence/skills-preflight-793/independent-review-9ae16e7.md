# Independent full-layer corrected review: PR #794

Disposition: **approved for the scoped code change, subject to fresh exact-head native CI and normal integration gates**. No actionable defect remains in this reviewed candidate. This review is independent of the correction's author and of the author's test results.

Base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
Reviewed immutable code HEAD: `9ae16e787ea05544db54c773a0599db6daa3712c`.
Earlier reviewed HEAD: `d7d0acda1ec5d07c8cfb18f8b6ba67060a4d608c`.
Worktree: `/workspace/symaira-pr794`, clean. No source changes, commits, pushes, merges or GitHub state changes were made by this reviewer.

## Finding closure and complete scoped assessment

The original independent full-layer review is retained in `migration/evidence/skills-preflight-793/independent-review-d7d0acda.md`, together with original failed process observations. Its production coverage remains applicable to the unchanged marker reader, caller routing, install/status behavior and read/write boundaries. I reviewed the entire corrective diff and rechecked its interaction with those paths.

The parser now matches known ASCII flags through original byte spans, preserves normalization and equals boundaries, and emits unquoted raw diagnostic operands directly on Unix. Windows emits the Go-compatible UTF-16 replacement representation; span endpoints are ASCII flag boundaries in the encoded representation. Missing-value messages use only known ASCII names. The earlier undefined-name and malformed-syntax defect is closed: the original nine reviewer cases and all expanded invalid-sequence/literal-replacement cases now match actual Go stdout, stderr and exit codes.

The new shared process helper preserves environment isolation, tempfile-backed stdout/stderr capture, a three-second deadline and child reaping on timeout. The existing preflight checks were moved without weakening assertions. Added tests distinguish invalid bytes from valid U+FFFD, cover equals truncation, triple-dash normalization, bad syntax and valid preceding flags. The new ADR explains the contract and retains the original evidence rather than rewriting it.

The full-layer conclusions from the original review remain: parsing precedes filesystem access; raw target trimming and original scope diagnostics match Go; classified rejected markers override dynamic-config/ordinary-malformed fallback; ordinary malformed/future-schema fallback remains; the common reader is bounded, same-handle, final no-follow and Unix nonblocking; existing-only directory probes do not create absent directories; writable-marker checks reject unsafe and future-schema states. Trust/confinement, source protection, conflict and copy/symlink policies are preserved. No aggregate marker budget or complete all-target/config migration is claimed, and Brain/Guard, credential, dependency and frozen Go boundaries are unchanged.

## Independent executions on the corrected candidate

- Verified **378 candidate source/manifest SHA-256 values** against the checkout and both real process binary hashes against the corrected receipt.
- Independently executed **315/315** real immutable-Go/native-Rust argv comparisons with isolated temporary HOME/XDG roots, empty PATH, no fallback, four-second subprocess bounds and exact complete stdout/stderr/exit matching. Revalidated retained Go observations and asserted that these parse probes created no skill data tree. Receipt: `/tmp/symaira-pr794-corrected-independent-probes.json`.
- Go binary SHA-256: `a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`.
- Rust binary SHA-256: `ac00a051ec5a53dbe0a3eb194b29974b724f562c47041eeb23b07fe78cdd2961`.
- Independently ran `cargo test --locked -p symbrain-cli --all-features --test skills_preflight_native --test skills_raw_flag_names` with worktree-owned output, umask 022 and subreaper: **3 parent tests passed, 0 failed, 0 ignored**. Log: `/tmp/symaira-pr794-corrected-independent-tests.log`.

The author's 446-pass broader suite is not relabeled as this reviewer's execution. This Linux review does not claim native macOS/Windows acceptance. Fresh checks must apply to the final published evidence-only HEAD as required by the handoff; main changes require the normal strict integration gate. #793 closes only after verified merge; #764/#476 and the full migration remain separate.
