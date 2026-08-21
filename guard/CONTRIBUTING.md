# Contributing to symguard

Thanks for your interest! This is an early-stage project; the best way to contribute right now is to open an issue describing what you'd like to see or report a bug.

## Development

```bash
git clone https://github.com/danieljustus/symaira-guard.git
cd symaira-guard
go build -o symguard ./cmd/symguard
go test ./...
go vet ./...
```

PRs are welcome. Please:
- Keep changes focused and small
- Add tests for new behavior
- Run `go test ./...` and `go vet ./...` before pushing
- Follow the [Conventional Commits](https://www.conventionalcommits.org/) format
