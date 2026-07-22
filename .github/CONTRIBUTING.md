# Contributing to symbrain

Thanks for your interest in contributing! This document covers the basics
of getting set up and what's expected in a pull request.

## Prerequisites

- Go 1.26 or newer
- `CGO_ENABLED=0` (the release build is CGO-free)

## Dev setup

```bash
git clone https://github.com/danieljustus/symaira-brain.git
cd symaira-brain
make build       # CGO_ENABLED=0 go build -o symbrain ./cmd/symbrain
make test        # go test ./...
```

Other useful targets:

```bash
make test-race   # go test -race ./...
make lint        # golangci-lint if available, else go vet
make fmt         # gofmt -w -s .
make fmt-check   # fail if gofmt would change any file
make vet         # go vet ./...
```

Before opening a PR, run the same checks CI runs:

```bash
go vet ./... && go test -race ./... && go build -o symbrain ./cmd/symbrain
```

## Code style

- Format with `gofmt -s` (`make fmt-check` enforces this in CI).
- Follow the architectural conventions, package layout, and boundary
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
  review; CI runs the same checks.
- Fill in the PR template's Testing and Checklist sections — reviewers
  use them to verify what was actually exercised.
