# Independent specification/parity review — integrated PR #813 corrections

## Candidate and scope

- **Reviewed integrated candidate:** `3276b54282ac6a5c696796dbe7b894e1790fa967` (tree `2379241a4a9edc43261ff967ca7a8b57e05d70de`), clean at review start; base `2f6880815a4aa65c4a2d7810240ef0cfd1effe4d`; origin `https://github.com/danieljustus/symaira-brain.git`.
- **Frozen Go oracle:** `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`.
- Reviewed integrated changes for #834 (startup), #835 (YAML), and #856 (Windows render locks). The final verdicts below bind to integrated HEAD `3276`, not the earlier standalone worker heads.
- Read-only review; no product-source edits, builds, or GitHub operations. A small local Python control exercised the ACL validator. Existing evidence receipts/logs were inspected rather than rerunning heavy gates.

## Verdicts

| Area | Candidate-bound verdict | Finding / remaining gate |
|---|---|---|
| **#834 startup** | **REQUIRED CHANGE** | Ordinary-MCP classifier reachability, memory/audit/history startup effects, and audit file-vs-environment precedence are corrected. The Windows ACL validator can still accept a DACL that denies the owner the very read/list access the policy requires; see F1. Native Windows ACL execution remains pending. |
| **#835 YAML** | **PASS for the approved target contract and local parity evidence** | The only serializer exceptions remain 12 leading-LF string rows and 2 literal `<<` metadata-key rows. No broader YAML normalization or output exclusion found. Full cross-platform/native and release acceptance remain pending. |
| **#856 render lock** | **PASS on static contract review and retained host process evidence; Windows acceptance pending** | Windows uses rooted `create_new`/no-follow acquisition, a 30-second bound, no delete sharing, and delete-on-close; the Unix advisory-lock path is unchanged. The exact-head local process test exercised both operation orders across the six targets. No current-candidate Windows-native run was found, so do not call the Windows gate approved. |

## F1 — required fix: evaluate effective Windows owner read/list access

**Files/lines:**

- `scripts/skills-native-oracle/startup_privacy.py:73-91`, especially `:85-91`.
- Add the regression to `scripts/skills-native-oracle/test_startup_controls.py:174-197`.

`validate_windows()` sets `owner_read` when it finds *any* owner allow ACE with `FILE_READ_DATA`/`FILE_LIST_DIRECTORY` (or generic read/all). It ignores applicable owner deny ACEs and their position. A canonical DACL with an explicit owner deny for mask `1` before a later owner allow for mask `1` passes this function, although the deny prevents the current owner from reading/listing the object under Windows access-check ordering. This contradicts the documented fail-closed requirement for missing owner read/list access.

I executed the current validator against that minimal deny-before-allow control; it returned success (`accepted deny-before-allow DACL`). The current `owner-denied-only` mutation only removes the owner allow and therefore does not catch this interaction.

**Required change:** validate effective owner read/list rights for the observed object (or conservatively reject any applicable owner deny covering those rights), and add a negative control with owner DENY-before-ALLOW that must fail through the actual policy function. Keep the Windows-native ACL gate pending until it runs on the exact repaired candidate.

## Findings by area

### #834 startup

- `rust/symbrain-cli/src/mcp_cli.rs:288-335` parses the global TOML and validates `audit.enabled` before reading `SYMBRAIN_AUDIT_ENABLED`; the call occurs before `build_backends` at `:82-90`. `rust/symbrain-cli/tests/support/mcp_profile_startup.rs:112-203` covers malformed TOML, invalid audit value/type, valid/missing config and environment precedence.
- Disabled profile memory does not open the embedded store in `rust/symbrain-gateway/src/lib.rs:144-160`; the startup process test asserts no memory DB/JWT state. Current Go patterns default is false in `internal/config/config.go:157-165,198-200`, and the target Rust startup adds no Go episode writer.
- The ordinary MCP classifier is reachable: `scripts/skills-native-oracle/run.py:134-142` routes every MCP pair through `startup_pair`, called at `:223-228`; `ordinary_startup.py:45-67` validates owner-bound startup deltas before constructing comparison views. The full protocol/catalog and unowned filesystem/cache state remain compared.
- Thus the prior ordinary-MCP and config/environment findings appear corrected. F1 is the remaining source-level blocker for this candidate’s Windows privacy claim.

### #835 YAML

- The implementation stays scoped: Go wraps only leading-LF strings and literal `<<` metadata keys in `internal/skills/skill/frontmatter_encode.go:15-27`; Rust retains the private Skills encoder and map-order compatibility rather than changing workspace YAML users.
- Independent byte checks against the current checkout confirmed: 740 scalar/context rows; 38 typed documents in both input and Go output; 12 leading-LF target rows plus 2 merge-key target rows (14 distinct IDs); input/output hashes match `yaml835-documents-provenance.json`; all 9 listed current Go source hashes match.
- The parent YAML handoff’s `encode_documents_test.rs` hash predates the integrated hardening. HEAD `3276` adds schema-error propagation and zero/false/missing/count/type/duplicate-ID negative controls. The current file hash is `f26499dd3289abb9d5b54403b78b5fdc8551000e805386f89835fa72e2a0cf32`; it is present in `integrated-skills-tests.receipt.json`, whose run exited 0, and the retained log shows the 740-row and 38-document tests plus the new validator controls passed. No stale handoff hash was treated as the current source identity.
- No additional serializer scope expansion or unapproved exception was found. Native/platform matrix remains a separate pending gate.

### #856 Windows render lock

- `rust/symbrain-skills/src/materialize_fs.rs:95-153` uses the capability-rooted Windows branch with `create_new(true)`, `FollowSymlinks::No`, `FILE_SHARE_READ | FILE_SHARE_WRITE` (no delete sharing), `FILE_FLAG_DELETE_ON_CLOSE`, and a 30-second deadline. The `#[cfg(not(windows))]` fs2 advisory-lock implementation at `:50-93` is unchanged.
- `rust/symbrain-skills/src/materialize_lock_windows_tests.rs:5-50` covers a preserved pre-existing lock, timeout, competing acquisition, unlink denial while held, cleanup on drop, and next-owner acquisition.
- The exact-head `integrated-cli-tests.receipt.json` records HEAD `3276`, clean source, exit 0. Its retained output includes `skills_render_lock::frozen_go_and_native_status_release_render_locks_in_both_orders`; the test requires 12 target/order observations (six targets × two operation orders). This is host execution, not Windows-native evidence.
- `original-windows-native.json` is bound to the earlier pre-lock candidate `d24af6e43cc349fb8fd76025f7d266f2674841c3` and has `native_surface_gate_passed: false`; it is retained failure evidence, not a result for HEAD `3276`. Exact-head Windows render-drift/Native Skills jobs are still required before claiming #856 acceptance.

## Evidence boundaries and execution note

Reviewed retained artifacts under `/Volumes/1TB_NVMe_SN850X/Dev/evidence/symaira-brain/gh-coding-complete-20261009-175832/pr813-integration-t_11cd8f23/`, including the integrated Go/Python/vet/Clippy/Skills/CLI receipts, `inventory.json`, and the original Windows run. Parent source/log receipts support the listed local gates; this review did not rerun them.

The runtime’s first default terminal cwd resolved to the coordinator worktree rather than the requested isolated path; that first command was a read-only identity/status probe and made no changes. All subsequent shell commands, including the ACL control, used the exact assigned subagent worktree path.

**Overall:** not ready for an unconditional spec approval until F1 is fixed. YAML’s scoped parity review passes. Lock source and host interop evidence pass review, but Windows-native acceptance remains an explicit blocker.