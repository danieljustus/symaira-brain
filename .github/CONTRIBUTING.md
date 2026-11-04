# Contributing to symbrain

Thanks for your interest in contributing! This document covers the basics
of getting set up and what's expected in a pull request.

## Prerequisites

- [rustup](https://rustup.rs); the toolchain pinned in `rust-toolchain.toml`
  is installed automatically.
- For `make rust-check`: `cargo install cargo-audit --version 0.22.2 --locked`
  and `cargo install cargo-deny --version 0.20.2 --locked`.

## Dev setup

```bash
git clone https://github.com/danieljustus/symaira-brain.git
cd symaira-brain
make build       # release symbrain binary
make test        # cargo test --workspace
```

Other useful targets:

```bash
make lint        # rustfmt check + clippy -D warnings
make fmt         # cargo fmt --all
make fmt-check   # fail if rustfmt would change any file
make rust-check  # complete gate, as in CI
make parity-smoke init-differential   # frozen Go-transcript replays
```

See the [CI workflow](workflows/ci.yml) for the browse, Swift, GUI, fuzz and
coverage jobs.

## Code style

- Format with rustfmt (`make fmt-check` enforces this in CI).
- Follow the architectural conventions, crate layout, and boundary
  rules in [AGENTS.md](../AGENTS.md) — this file only covers setup and
  process; AGENTS.md is the source of truth for how the code is
  organized and why.

## Pull request process

- Branch from `main`; use a short descriptive branch name.
- Keep commits focused — one logical change per commit, with a clear
  commit message describing what changed and why.
- Reference the issue a PR addresses (e.g. `Closes #123`) where one
  exists.
- Make sure `make test` and `make lint` pass locally before requesting
  review. CI runs the complete Rust gate on Linux and macOS plus the browse,
  module, and GUI jobs; see [the workflow](workflows/ci.yml).
- Fill in the PR template's Testing and Checklist sections — reviewers
  use them to verify what was actually exercised.
