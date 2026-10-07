# Independent full-layer review: PR #794

Disposition: **changes requested**. One reproducible stderr-byte contract defect remains; no additional safety defect was confirmed in the changed production paths. This is an independent review of the complete scoped change, not an approval inherited from an earlier reviewer or from the author's test results.

- Repository: danieljustus/symaira-brain.
- Immutable base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
- Immutable candidate: `d7d0acda1ec5d07c8cfb18f8b6ba67060a4d608c`.
- Worktree: `/workspace/symaira-pr794`; clean before and after review. Candidate sources were not edited; no commits, pushes or merges were performed.
- Applicable guidance: root AGENTS.md and docs/handoffs/end-work-cloud.md, including the independent full-layer review and fresh exact-head native acceptance gates.

## Finding: preserve invalid UTF-8 in flag-name diagnostics

Severity: low; required contract correction before this candidate satisfies #793.

Location: `rust/symbrain-cli/src/skills_sync_flags.rs:26`, with diagnostic emission at lines 35 and 85.

The new parser derives `arg` and `name` from `raw.to_string_lossy()`, then interpolates them into the bad-syntax and undefined-flag errors. On Unix this changes invalid UTF-8 argument bytes to the encoded replacement character. Frozen Go prints these error operands as their original bytes. The earlier repair correctly preserves raw target/scope values, but does not preserve unknown flag names or malformed flag spellings.

Actual process reproduction (isolated HOME/XDG, empty PATH, no configured Go fallback):

- argv suffix bytes `--bad\xff`: both exit 2 and emit empty stdout. Go's first stderr line is `flag provided but not defined: -bad` followed by byte `ff`; Rust emits bytes `ef bf bd` instead.
- `--bad\xff=x`, `-\xff`, `---\xff`, malformed `--=\xff`, overlong-invalid `--bad\xc0\xaf`, and unknown raw flags after valid target/dry-run flags also differ.
- A valid literal U+FFFD flag name matches. Invalid bytes must not be conflated with that valid rune.

Recommended correction: continue ASCII flag matching without losing the original OsStr/byte spans, and write raw diagnostic operands to stderr on Unix. Preserve Go normalization (including triple-dash inputs), equals truncation for undefined-name messages, and its full malformed-spelling message. Do not quote or escape these operands: actual Go emits their bytes directly. Add isolated native and actual-Go regressions covering inline unknown names, malformed syntax, invalid sequences, literal U+FFFD, and valid flags preceding an unknown raw name.

Literal observations are in `/tmp/symaira-pr794-independent-probes.json`; they include complete stdout/stderr hex, exit codes, argv hex and matched binary SHA-256 identities. This file is independent evidence, not a rewritten frozen fixture.

## Full-layer inspection

All changed production files were read in full or with their complete relevant surrounding paths:

- `rust/symbrain-cli/src/skills_sync_flags.rs`: whole-vector normalization, Go boolean spellings, missing values, repeated flags, help, positional and double-dash termination, raw value extraction, Unicode trim boundaries, target/scope validation and pre-filesystem ordering.
- `rust/symbrain-cli/src/skills_cli.rs`: caller routing, output extraction interaction, static/dynamic configuration eligibility, OpenCode user/project roots, enumeration limits, deferred malformed/future-schema fallback, rejection precedence, and construction of sync options. Existing source protection, conflict policy, mode selection and CLI output behavior were traced.
- `rust/symbrain-skills/src/install/marker.rs`: public MarkerState compatibility, noncreating directory probe, missing versus rejected versus malformed classification, common bounded no-follow reader, schema preservation and refusal to overwrite malformed/rejected/future-schema markers.
- `rust/symbrain-skills/src/install/status_compare.rs`: rejected marker rows, read-only comparison, unmanaged classification and error reporting.

Supporting code inspected: `symbrain-cli/src/lib.rs` routing/normalization; `symbrain-core` output extraction/quoter contracts; skills `load_read.rs`, `install/replace.rs`, `destination.rs`, `status.rs`, `ops.rs`, `sync.rs`, module exports and related existing bounds/install tests; frozen Go `cmd/symbrain/cmd_skills.go` parser and sync ordering. The added `skills_preflight_native.rs` was read entirely. Migration/contract-matrix changes, the current and historical handoff distinctions, existing historical review, retained failure records and the current raw-target receipt were checked.

Safety and behavioral conclusions:

- Sync parse failures return before resolving or scanning filesystem roots, fixing the previous missing-value loop.
- The byte-preserving boundary trim correctly stops at malformed UTF-8 and preserves scope's original diagnostic padding; all independently replayed retained cases match.
- Marker classification preserves the ordinary malformed/future-schema fallback and scans past those conditions to detect a later rejected marker. Classified unsafe markers override dynamic-config fallback.
- The shared final open uses Unix NONBLOCK, FollowSymlinks::No and metadata from the opened handle. Nonregular files, oversized files and post-open growth are rejected with bounded reads. No weaker marker read is introduced by this change.
- Existing-only capability directory walking avoids creating absent or renamed marker directories; every MarkerState caller was traced, and rejected states cannot become writable through ensure_writable.
- Unix/Windows conditional branches and Windows capability/no-follow assumptions were inspected. Native Windows/macOS runtime acceptance remains a separate CI gate; this Linux review does not claim those results.
- Enumeration is bounded and marker reads retain the existing per-file limit; no aggregate marker budget is claimed. Those wider #476/#764 requirements remain separate. Existing Go-owned dynamic configuration, unsupported status variants and second-link fallback are not claimed as fully ported by this review.
- No dependency, credential, network, permission, Brain/Guard boundary or frozen Go/fixture production change is introduced in this scoped patch.

## Independent verification performed

1. Verified all **376** candidate source/manifest hashes in the current raw-target receipt against the checked-out candidate. Verified both process binaries against its SHA-256 identities (Go `a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`, Rust `1957d79efdb899f01afe18101fd86f0c1ebd366fbf2c24919773b8883875b361`).
2. Independently executed all **198** retained raw argv probes against both real binaries with isolated temporary roots and four-second child deadlines: **198 matched** complete stdout/stderr/exit.
3. Executed **9** additional raw flag-name/syntax probes: **1 matched, 8 differed**, all manifestations of the single finding above.
4. Ran `cargo test --locked -p symbrain-cli --all-features --test skills_preflight_native` using the worktree-owned target, umask 022 and the session subreaper: **1 parent test passed, 0 failed, 0 ignored**. Its bounded child checks passed. Log: `/tmp/symaira-pr794-independent-native-test.log`.
5. `git diff --check <base> <head>` passed; contract-matrix keys were unique; HEAD and clean status were rechecked after verification.

The author's larger affected-package suites were not rerun unnecessarily and are not relabeled as this reviewer's executions. Native three-OS CI and current required checks were not accepted from an obsolete revision. After the raw-name correction, obtain review of the updated immutable candidate and fresh exact-head acceptance before marking the draft ready or merging. This review does not close #793 or any broader migration issue.
