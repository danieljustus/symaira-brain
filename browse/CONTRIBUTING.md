# Contributing to Symaira Browse

Thanks for helping improve Symaira Browse. Please read [AGENTS.md](../AGENTS.md) before making a change; it defines the repository rules. The per-topic documents under [docs/](docs) describe Browse's design contracts. The issue or milestone defines the permitted scope.

## Development workflow

1. Create a focused branch from the current default branch.
2. Keep the change limited to one issue when practical.
3. Add tests for behavior changes and preserve standalone-first operation.
4. Run the checks from `browse/`; Browse has no workspace-local Makefile:

   ```sh
   cargo fmt --all --check
   cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
   cargo test --workspace --locked
   ```

   Real-Chrome end-to-end tests stay opt-in behind `SYMBROWSE_E2E=1`. See the
   [CI workflow](../.github/workflows/ci.yml) for the repository's other jobs.

5. Do not commit binaries, credentials, local configuration, or generated reports.
6. Describe the behavior change, checks run, and any residual risk in the pull request.

## Design boundaries

- Build with the toolchain pinned in `rust-toolchain.toml` and `--locked`.
- Do not add compile-time dependencies on sibling Symaira repositories.
- Preserve stable JSON output contracts as they are introduced.
- Treat browser content as untrusted input and keep policy decisions explicit.

For security-sensitive changes, follow [SECURITY.md](../.github/SECURITY.md) instead of opening a public issue with exploit details.
