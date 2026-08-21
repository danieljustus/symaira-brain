# Symaira Guard (symguard)
# Local-first security gateway for AI agents

BINARY := symguard
MODULE := github.com/danieljustus/symaira-guard
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
LDFLAGS := -X main.version=$(VERSION)

.PHONY: build test lint clean vet fmt

## build: Compile the symguard binary
build:
	go build -ldflags "$(LDFLAGS)" -o $(BINARY) ./cmd/symguard

## test: Run all tests
test:
	go test ./...

## vet: Run go vet static analysis
vet:
	go vet ./...

## lint: Run linters (golangci-lint if available, otherwise go vet)
lint:
	@command -v golangci-lint >/dev/null 2>&1 && \
		golangci-lint run ./... || \
		echo "golangci-lint not installed, falling back to go vet" && \
		$(MAKE) vet

## fmt: Format all Go source files
fmt:
	gofmt -w -s .

## clean: Remove build artifacts
clean:
	rm -f $(BINARY)
