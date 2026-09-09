# Symaira Brain (symbrain)
# Portable agent-context layer for AI harnesses

BINARY := symbrain
MODULE := github.com/danieljustus/symaira-brain
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
# Keep the Go oracle on the module's declared toolchain, even when a newer Go is installed.
GO_ORACLE_TOOLCHAIN ?= $(shell awk '$$1 == "go" { print "go" $$2; exit }' go.mod)
GO_ORACLE_REF ?= HEAD
LDFLAGS := -X main.version=$(VERSION)

.PHONY: build build-rust parity-smoke rust-go-printable-check usage-oracle-check policy-oracle-check catalog-oracle-check audit-oracle-check patterns-activity-oracle-check mcp-oracle-check gateway-oracle-check broker-oracle-check managed-oracle-check skills-oracle-check instructions-oracle-check adapters-oracle-check install-oracle-check profile-remove-oracle-check guard-oracle-check rust-check rust-fuzz-build rust-fuzz-smoke test test-race test-memory-large coverage lint fmt-check fmt vet clean

## coverage: Run tests and write machine-readable coverage artifacts
coverage:
	@set -eu; \
	tmp_dir="$$(mktemp -d)"; \
	trap 'rm -rf "$$tmp_dir"' EXIT; \
	profile="$${COVERAGE_PROFILE:-$$tmp_dir/coverage.out}"; \
	test_log="$${COVERAGE_LOG:-$$tmp_dir/test.log}"; \
	go list ./... > "$$tmp_dir/packages"; \
	if [ -z "$${COVERAGE_PROFILE:-}" ]; then \
		go test ./... -coverprofile="$$profile" 2>&1 | tee "$$test_log"; \
	fi; \
	total="$$(go tool cover -func="$$profile" | awk '/^total:/ {gsub(/%/, "", $$3); print $$3}')"; \
	total="$${total:-0.0}"; \
	commit_sha="$$(git rev-parse HEAD)"; \
	{ \
		printf '{\n  "schema_version": 1,\n  "commit_sha": "%s",\n  "total": %s,\n  "packages": {\n' "$$commit_sha" "$$total"; \
		first=true; \
		while IFS= read -r package; do \
			[ -n "$$package" ] || continue; \
			coverage="$$(awk -v package="$$package" '$$1 == "ok" && $$2 == package { for (i = 1; i <= NF; i++) if ($$i == "coverage:") { value = $$(i + 1); sub(/%$$/, "", value); if (value ~ /^[0-9]+([.][0-9]+)?$$/) print value; exit } }' "$$test_log")"; \
			coverage="$${coverage:-0.0}"; \
			if [ "$$first" = true ]; then first=false; else printf ',\n'; fi; \
			printf '    "%s": %s' "$$package" "$$coverage"; \
		done < "$$tmp_dir/packages"; \
		printf '\n  }\n}\n'; \
	} > coverage.json; \
	printf '{\n  "schemaVersion": 1,\n  "label": "coverage",\n  "message": "%s%%",\n  "color": "blue"\n}\n' "$$total" > badge.json; \
	printf '%s\n' \
		'<?xml version="1.0" encoding="UTF-8"?>' \
		'<svg xmlns="http://www.w3.org/2000/svg" width="108" height="20" role="img" aria-label="coverage: '"$$total"'%">' \
		'  <title>coverage: '"$$total"'%</title>' \
		'  <linearGradient id="s" x2="0" y2="100%">' \
		'    <stop offset="0" stop-color="#bbb" stop-opacity=".1"/>' \
		'    <stop offset="1" stop-opacity=".1"/>' \
		'  </linearGradient>' \
		'  <clipPath id="r"><rect width="108" height="20" rx="3" fill="#fff"/></clipPath>' \
		'  <g clip-path="url(#r)">' \
		'    <rect width="58" height="20" fill="#555"/>' \
		'    <rect x="58" width="50" height="20" fill="#007ec6"/>' \
		'    <rect width="108" height="20" fill="url(#s)"/>' \
		'  </g>' \
		'  <g fill="#fff" text-anchor="middle" font-family="Verdana,Geneva,DejaVu Sans,sans-serif" font-size="11">' \
		'    <text x="29" y="15" fill="#010101" fill-opacity=".3">coverage</text>' \
		'    <text x="29" y="14">coverage</text>' \
		'    <text x="83" y="15" fill="#010101" fill-opacity=".3">'"$$total"'%</text>' \
		'    <text x="83" y="14">'"$$total"'%</text>' \
		'  </g>' \
		'</svg>' > badge.svg

## build: Compile the symbrain binary
build:
	CGO_ENABLED=0 go build -ldflags "$(LDFLAGS)" -o $(BINARY) ./cmd/symbrain

## build-rust: Build the incremental Rust entrypoint and its Go fallback
build-rust:
	@mkdir -p target/go
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" build -ldflags "$(LDFLAGS)" -o "$(abspath target/go/symbrain-go)" ./cmd/symbrain
	SYMBRAIN_VERSION="$(VERSION)" cargo build --workspace --locked

## parity-smoke: Compare migrated Rust CLI slices against the pinned Go oracle
parity-smoke: rust-go-printable-check
	@mkdir -p target/go
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" build -ldflags "-X main.version=dev" -o "$(abspath target/go/symbrain-go)" ./cmd/symbrain
	SYMBRAIN_VERSION=dev cargo build --workspace --locked
	python3 scripts/rust-differential.py target/go/symbrain-go target/debug/symbrain

## rust-go-printable-check: Ensure the pinned Go IsPrint table is current
rust-go-printable-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run scripts/generate-go-printable-table.go -check

## usage-oracle-check: Ensure native Usage fixtures remain derived from Go
usage-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/usage-oracle -check

## policy-oracle-check: Ensure the profile/policy oracle expectations are current
policy-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/policy-oracle -check

## guard-oracle-check: Ensure Guard model/static-kernel expectations match Go
guard-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./guard/scripts/guard-oracle -check
## catalog-oracle-check: Ensure the catalog oracle expectations are current
catalog-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/catalog-oracle -check

## audit-oracle-check: Ensure the audit oracle expectations are current
audit-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/audit-oracle -check

## patterns-activity-oracle-check: Ensure behavioral context expectations are current
patterns-activity-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/patterns-activity-oracle -check

## mcp-oracle-check: Ensure JSON-RPC framing expectations are current
mcp-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/mcp-oracle -check

## gateway-oracle-check: Ensure gateway behavior expectations are current
gateway-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/gateway-oracle -check

## broker-oracle-check: Ensure child lifecycle expectations are current
broker-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/broker-oracle -check

## managed-oracle-check: Ensure manifest/archive expectations are current
managed-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" test ./internal/managed -run TestRustManagedOracleFixture -count=1

## skills-oracle-check: Ensure skill model fixtures match the Go loader
skills-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/skills-oracle -check

## instructions-oracle-check: Ensure instruction fixtures match the Go implementation
instructions-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/instructions-oracle -check

## adapters-oracle-check: Ensure instruction adapter fixtures match the Go implementation
adapters-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/adapters-oracle -check

## install-oracle-check: Ensure install/uninstall expectations match the Go source
install-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/install-oracle -check

## profile-remove-oracle-check: Ensure profile removal expectations match the Go source
profile-remove-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/profile-remove-oracle -check

## rust-check: Run the complete fast Rust quality gate
rust-check: rust-go-printable-check usage-oracle-check policy-oracle-check guard-oracle-check catalog-oracle-check audit-oracle-check patterns-activity-oracle-check mcp-oracle-check gateway-oracle-check broker-oracle-check managed-oracle-check skills-oracle-check instructions-oracle-check adapters-oracle-check install-oracle-check profile-remove-oracle-check
	cargo fmt --all --check
	cargo check --workspace --all-targets --all-features --locked
	cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	cargo test --workspace --all-features --locked
	cargo test --workspace --doc --all-features --locked
	cargo audit --locked
	cargo deny check

## rust-fuzz-build: Compile every MCP fuzz target with nightly libFuzzer
rust-fuzz-build:
	cargo +nightly fuzz build frame-decoder
	cargo +nightly fuzz build jsonrpc-envelope

## rust-fuzz-smoke: Exercise both MCP fuzz targets without mutating tracked seeds
rust-fuzz-smoke: rust-fuzz-build
	@set -eu; \
	tmp=$$(mktemp -d); \
	trap 'rm -rf "$$tmp"' EXIT INT TERM; \
	mkdir -p "$$tmp/frame" "$$tmp/envelope"; \
	cp fuzz/corpus/frame_decoder/* "$$tmp/frame/"; \
	cp fuzz/corpus/jsonrpc_envelope/* "$$tmp/envelope/"; \
	cargo +nightly fuzz run frame-decoder "$$tmp/frame" -- -runs=10000 -max_len=1048577 -rss_limit_mb=2048; \
	cargo +nightly fuzz run jsonrpc-envelope "$$tmp/envelope" -- -runs=10000 -max_len=1048577 -rss_limit_mb=2048

## test: Run all tests
test:
	go test ./...

## test-race: Run all tests with the race detector
test-race:
	go test -race ./...

## test-memory-large: Run bounded embedding storage measurements explicitly.
## Override MEMORY_STORAGE_SCALE up to 10000 only when the disk budget is known.
MEMORY_STORAGE_SCALE ?= 1000
MEMORY_LARGE_TEST_TIMEOUT ?= 20m
test-memory-large:
	SYMBRAIN_MEMORY_STORAGE_SCALE=$(MEMORY_STORAGE_SCALE) go test -tags memory_large -timeout $(MEMORY_LARGE_TEST_TIMEOUT) -run 'TestEmbedding(StorageSize|BackupSize|Recommendation)$$' -count=1 ./internal/memory/db

## vet: Run go vet static analysis
vet:
	go vet ./...

## lint: Deterministic lint gate (go vet + gofmt check, matches CI)
lint: vet fmt-check

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
	cargo clean
