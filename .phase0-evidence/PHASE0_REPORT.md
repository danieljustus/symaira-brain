# Brain phase-0 baseline — 2026-09-13

- **cwd/root:** `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-brain/.worktrees/rust-integration-20260913` (asserted before metadata/tests)
- **HEAD/source SHA:** `0b585d52915a824664e1377d0a995dff3f5405cd`
- **Go oracle:** explicitly pinned `GO_ORACLE_REF=0b585d52915a824664e1377d0a995dff3f5405cd`; `scripts/run-go-oracle.sh` inspected; declared Go `1.26.7`; `GOMAXPROCS=2`; `CGO_ENABLED=0`.
- **Cargo preflight:** `cargo metadata --manifest-path <absolute worktree>/Cargo.toml --no-deps --format-version 1`; workspace root and all selected package manifests/src paths resolved inside this worktree. Rust `1.98.0`.
- **target isolation:** `git check-ignore` showed no ignore match for `target-run`; existing `target-run` outputs were preserved. Focused Rust rerun used `<worktree>/target/migration-run` (ignored by `.gitignore`) and preserved it.

## Commands/results

- `./scripts/run-go-oracle.sh 0b585d... build -o .phase0-evidence/symbrain-go ./cmd/symbrain` — **exit 0**.
- `./scripts/run-go-oracle.sh 0b585d... test ./...` — **exit 1**. This first run used XDG paths under the worktree; failures are predominantly test isolation-contract conflicts (tests set HOME but expect XDG unset), e.g. missing `profiles/*.toml`, config path mismatch, and `TestCmdMcp_StdioHandshake` timeout. It is retained as diagnostic evidence, not a source PASS/FAIL attribution. A clean-env retry was attempted but produced no artifact/exit due the shared terminal environment interruption; therefore the root Go suite remains **unverified cleanly**.
- Focused Rust (`CARGO_TARGET_DIR=<worktree>/target/migration-run`, `TMPDIR=/private/tmp`, absolute manifest):
  - `symbrain-policy --test typed_decode_tests` — **24 passed, exit 0**.
  - `symbrain-mcp --test gateway_protocol_tests` — **16 passed, exit 0**.
  - `symbrain-managed --test install_tests` — **6 passed, exit 0**.
  - `symbrain-skills --test render_tests` — **11 passed, exit 0**.
  - `symbrain-cli --test mcp_cli_tests` — **6 passed, 2 failed, exit 101**.
  - `symbrain-memory --all-features` — **exit 0**, no tests executed.

## Actionable Rust failures

`rust/symbrain-cli/tests/mcp_cli_tests.rs`: `usage_subprocess_lists_and_calls_native_tool_without_go_fallback` panics at line 394 with `usage audit: Os { code: 2, kind: NotFound }`; `native_mcp_audit_creates_redacted_jsonl_without_stdout_pollution` panics at line 184: `audit log was not created`. The other six CLI contract tests passed.

## Next retained delta

No uncommitted source delta exists at this assigned HEAD. The next retained implementation delta in Brain history is `a02adef` (`test(managed): exclude optional cores from Fix mismatch selection`), immediately below the current documentation-only cutover commit. Do not integrate narrative docs as code; retain the two CLI failures as the next executable blocker. No source/lockfile/CI/register files were modified; only ignored/allowed evidence and build outputs were created.
