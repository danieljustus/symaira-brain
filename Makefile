# Symaira Brain (symbrain)
# Portable agent-context layer for AI harnesses

BINARY := symbrain
MODULE := github.com/danieljustus/symaira-brain
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
LDFLAGS := -X main.version=$(VERSION)

.PHONY: build test test-race lint fmt-check fmt vet clean

## build: Compile the symbrain binary
build:
	CGO_ENABLED=0 go build -ldflags "$(LDFLAGS)" -o $(BINARY) ./cmd/symbrain

## test: Run all tests
test:
	go test ./...

## test-race: Run all tests with the race detector
test-race:
	go test -race ./...

## vet: Run go vet static analysis
vet:
	go vet ./...

## lint: Run linters (golangci-lint if available, otherwise go vet)
lint:
	@command -v golangci-lint >/dev/null 2>&1 && \
		golangci-lint run ./... || \
		(echo "golangci-lint not installed, falling back to go vet" && $(MAKE) vet)

## fmt: Format all Go source files
fmt:
	gofmt -w -s .

## fmt-check: Fail if gofmt would change any file
fmt-check:
	@test -z "$$(gofmt -l .)" || (echo "gofmt needed on:"; gofmt -l .; exit 1)

## clean: Remove build artifacts and test cache
clean:
	rm -f $(BINARY)
	go clean -testcache
