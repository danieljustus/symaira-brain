# Symaira Brain (symbrain)
# Portable agent-context layer for AI harnesses

BINARY := symbrain
MODULE := github.com/danieljustus/symaira-brain
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
# Keep fixture checks on the Go toolchain declared by this checkout.
GO_ORACLE_TOOLCHAIN := $(shell awk '$$1 == "go" { print "go" $$2; exit }' go.mod)
GO_ORACLE_REF ?= HEAD
LDFLAGS := -X main.version=$(VERSION)

EXTERNAL_ARTIFACT_ROOT := .
EXTERNAL_GO_ARTIFACT_ROOT := target/go
EXTERNAL_CARGO_TARGET_DIR := target
ifeq ($(strip $(CI)),)
ifeq ($(shell uname -s),Darwin)
EXTERNAL_WORKTREE_KEY := $(shell printf '%s' '$(CURDIR)' | shasum -a 256 | cut -c1-16)
SYMAIRA_EXTERNAL_BASE := $(or $(SYMAIRA_EXTERNAL_BASE),/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/builds/symaira-brain)
EXTERNAL_ARTIFACT_ROOT := $(SYMAIRA_EXTERNAL_BASE)/artifacts
EXTERNAL_GO_ARTIFACT_ROOT := $(EXTERNAL_ARTIFACT_ROOT)/go
EXTERNAL_CARGO_TARGET_DIR := $(SYMAIRA_EXTERNAL_BASE)/cargo-target/$(EXTERNAL_WORKTREE_KEY)
endif
endif
EXTERNAL_RUN := SYMAIRA_EXTERNAL_BASE="$(SYMAIRA_EXTERNAL_BASE)" bash $(CURDIR)/scripts/run-external-env.sh

.PHONY: build build-rust parity-smoke rust-go-printable-check usage-oracle-check policy-oracle-check xdg-oracle-check catalog-oracle-check audit-oracle-check patterns-activity-oracle-check mcp-oracle-check gateway-oracle-check broker-oracle-check managed-oracle-check skills-oracle-check instructions-oracle-check adapters-oracle-check install-oracle-check profile-remove-oracle-check guard-oracle-check guard-doctor-oracle-check guard-scan-oracle-check guard-scan-oracle-test rust-guard-check rust-audit rust-deny rust-fast rust-check rust-fuzz-build rust-fuzz-smoke test test-race test-memory-large coverage lint fmt-check fmt vet clean

## coverage: Run tests and write machine-readable coverage artifacts
coverage:
	@set -eu; \
	 $(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_ARTIFACT_ROOT)"; \
	 tmp_dir="$$($(EXTERNAL_RUN) sh -c 'mktemp -d "$$TMPDIR/symbrain.XXXXXX"')"; \
	 trap 'rm -rf "$$tmp_dir"' EXIT; \
	 profile="$${COVERAGE_PROFILE:-$(EXTERNAL_ARTIFACT_ROOT)/coverage.out}"; \
	 test_log="$${COVERAGE_LOG:-$$tmp_dir/test.log}"; \
	 $(EXTERNAL_RUN) go list ./... > "$$tmp_dir/packages"; \
	 if [ -z "$${COVERAGE_PROFILE:-}" ]; then \
		 $(EXTERNAL_RUN) go test ./... -coverprofile="$$profile" 2>&1 | tee "$$test_log"; \
	 fi; \
	 total="$$($(EXTERNAL_RUN) go tool cover -func="$$profile" | awk '/^total:/ {gsub(/%/, "", $$3); print $$3}')"; \
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
	} > "$(EXTERNAL_ARTIFACT_ROOT)/coverage.json"; \
	 printf '{\n  "schemaVersion": 1,\n  "label": "coverage",\n  "message": "%s%%",\n  "color": "blue"\n}\n' "$$total" > "$(EXTERNAL_ARTIFACT_ROOT)/badge.json"; \
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
		'</svg>' > "$(EXTERNAL_ARTIFACT_ROOT)/badge.svg"

## build: Compile the symbrain binary
build:
	$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_ARTIFACT_ROOT)"
	$(EXTERNAL_RUN) env CGO_ENABLED=0 go build -ldflags "$(LDFLAGS)" -o "$(EXTERNAL_ARTIFACT_ROOT)/$(BINARY)" ./cmd/symbrain

## build-rust: Build the incremental Rust entrypoint and its Go fallback
build-rust:
	@$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_GO_ARTIFACT_ROOT)"
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" build -ldflags "$(LDFLAGS)" -o "$(abspath $(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go)" ./cmd/symbrain
	$(EXTERNAL_RUN) env SYMBRAIN_VERSION="$(VERSION)" CARGO_TARGET_DIR="$(EXTERNAL_CARGO_TARGET_DIR)" cargo build --workspace --locked

## parity-smoke: Compare migrated Rust CLI slices against the pinned Go oracle
parity-smoke: rust-go-printable-check
	@$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_GO_ARTIFACT_ROOT)"
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" build -ldflags "-X main.version=dev" -o "$(abspath $(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go)" ./cmd/symbrain
	$(EXTERNAL_RUN) env SYMBRAIN_VERSION=dev CARGO_TARGET_DIR="$(EXTERNAL_CARGO_TARGET_DIR)" cargo build --workspace --locked
	./guard/scripts/guard-scan-oracle/run.sh parity "$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain"
	$(EXTERNAL_RUN) python3 scripts/rust-differential.py "$(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go" "$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain"

## rust-go-printable-check: Ensure the pinned Go IsPrint table is current
rust-go-printable-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run scripts/generate-go-printable-table.go -check

## usage-oracle-check: Ensure native Usage fixtures remain derived from Go
usage-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/usage-oracle -check

## policy-oracle-check: Ensure the profile/policy oracle expectations are current
policy-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/policy-oracle -check

## xdg-oracle-check: Ensure XDG and legacy path precedence expectations match Go
xdg-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/xdg-oracle -check

## cli-oracle-check: Ensure the frozen CLI command-tree expectations match Go
cli-oracle-check:
	@$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_GO_ARTIFACT_ROOT)"
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" build -ldflags "-X main.version=dev" -o "$(abspath $(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go)" ./cmd/symbrain
	$(EXTERNAL_RUN) env GOTOOLCHAIN=$(GO_ORACLE_TOOLCHAIN) CGO_ENABLED=0 go run ./scripts/cli-oracle -go-binary "$(abspath $(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go)" -check

## db-memory-oracle-check: Ensure the frozen memory SQLite facts match Go
db-memory-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./scripts/db-memory-oracle -check

## guard-oracle-check: Ensure Guard model/static-kernel expectations match Go
guard-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./guard/scripts/guard-oracle -check

## guard-doctor-oracle-check: Ensure the frozen guard doctor fixture matches Go
guard-doctor-oracle-check:
	./scripts/run-go-oracle.sh "$(GO_ORACLE_REF)" run ./guard/scripts/guard-doctor-oracle -check

## guard-scan-oracle-check: Ensure Guard scan fixtures and source bindings are current
guard-scan-oracle-check:
	./guard/scripts/guard-scan-oracle/run.sh check

## guard-scan-oracle-test: Run Guard scan oracle fixture tests
guard-scan-oracle-test:
	./guard/scripts/guard-scan-oracle/run.sh test

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

.PHONY: init-differential
INIT_RUST_BINARY ?= target/debug/symbrain$(if $(filter Windows_NT,$(OS)),.exe,)
init-differential:
	@set -eu; \
	 root="$$($(EXTERNAL_RUN) sh -c 'mktemp -d "$$TMPDIR/symbrain.XXXXXX"')"; source="$$root/source"; \
	 trap 'git worktree remove --force "$$source" >/dev/null 2>&1 || true; rm -rf "$$root"' EXIT INT TERM; \
	 git worktree add --quiet --detach "$$source" HEAD; \
	 $(EXTERNAL_RUN) cargo build -p symbrain-cli --locked; \
	 $(EXTERNAL_RUN) python3 "$$source/scripts/init-oracle/test_compare.py"; \
	 $(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_ARTIFACT_ROOT)/init-oracle"; \
	 $(EXTERNAL_RUN) python3 "$$source/scripts/init-oracle/compare.py" --repo-root "$$source" --rust-binary "$(if $(CI),$(INIT_RUST_BINARY),$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain$(if $(filter Windows_NT,$(OS)),.exe,))" --output "$(EXTERNAL_ARTIFACT_ROOT)/init-oracle/report.json"

## rust-guard-check: Check the existing Guard library against its Go oracles
rust-guard-check:
	$(EXTERNAL_RUN) env GOTOOLCHAIN=$(GO_ORACLE_TOOLCHAIN) go run ./guard/scripts/guard-oracle -check
	$(EXTERNAL_RUN) env GOTOOLCHAIN=$(GO_ORACLE_TOOLCHAIN) go test ./guard/internal/capability -run '^TestCapabilityOracle(Fixture|RejectsDrift)$$' -count=1 -v
	$(EXTERNAL_RUN) cargo fmt --all --check
	$(EXTERNAL_RUN) cargo check --workspace --all-targets --all-features --locked
	$(EXTERNAL_RUN) cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	$(EXTERNAL_RUN) cargo test --workspace --all-targets --all-features --locked
	$(EXTERNAL_RUN) cargo test --workspace --doc --all-features --locked

## rust-audit: Audit the committed lockfile without resolving or updating it
rust-audit:
	$(EXTERNAL_RUN) cargo audit --file Cargo.lock --deny warnings

## rust-deny: Enforce dependency/license policy without changing Cargo.lock
rust-deny:
	$(EXTERNAL_RUN) cargo deny --locked --all-features check

## rust-fast: Run focused Rust migration checks for the CLI, skills, and Guard
rust-fast:
	$(EXTERNAL_RUN) cargo fmt --all --check
	$(EXTERNAL_RUN) cargo test -p symbrain-cli -p symbrain-skills -p symbrain-guard-core --locked

## rust-check: Run the complete fast Rust quality gate
#
# cli-oracle, xdg-oracle, db-memory-oracle and guard-doctor-oracle were kept
# out of this list after PR #628's first CI run hung/failed on all four: the
# fixtures were calibrated to the recording machine, not a portable
# environment (#627 PATH leak, #630 isolation-root shape, #631 wiring an
# unverified oracle into the gate). #627 (PATH pinned to an empty directory)
# and #630 (isolation root resolved past symlinked temp-dir ancestors, both
# in scripts/cli-oracle and in the cli_tree_tests.rs consumer) are fixed and
# all four pass locally; per #631 they are wired back in this same change so
# CI proves them before merge, rather than being re-added on faith.
rust-check: rust-go-printable-check usage-oracle-check policy-oracle-check xdg-oracle-check guard-oracle-check guard-doctor-oracle-check guard-scan-oracle-check guard-scan-oracle-test catalog-oracle-check audit-oracle-check patterns-activity-oracle-check mcp-oracle-check gateway-oracle-check broker-oracle-check managed-oracle-check skills-oracle-check instructions-oracle-check adapters-oracle-check install-oracle-check profile-remove-oracle-check db-memory-oracle-check cli-oracle-check
	$(EXTERNAL_RUN) cargo fmt --all --check
	$(EXTERNAL_RUN) cargo check --workspace --all-targets --all-features --locked
	$(EXTERNAL_RUN) cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	$(EXTERNAL_RUN) cargo test --workspace --all-features --locked
	$(EXTERNAL_RUN) cargo test --workspace --doc --all-features --locked
	$(EXTERNAL_RUN) cargo audit
	$(EXTERNAL_RUN) cargo deny check

## rust-fuzz-build: Compile every MCP fuzz target with nightly libFuzzer
rust-fuzz-build:
	$(EXTERNAL_RUN) cargo +nightly fuzz build frame-decoder
	$(EXTERNAL_RUN) cargo +nightly fuzz build jsonrpc-envelope

## rust-fuzz-smoke: Exercise both MCP fuzz targets without mutating tracked seeds
rust-fuzz-smoke: rust-fuzz-build
	@set -eu; \
	tmp=$$($(EXTERNAL_RUN) sh -c 'mktemp -d "$$TMPDIR/symbrain.XXXXXX"'); \
	trap 'rm -rf "$$tmp"' EXIT INT TERM; \
	mkdir -p "$$tmp/frame" "$$tmp/envelope"; \
	cp fuzz/corpus/frame_decoder/* "$$tmp/frame/"; \
	cp fuzz/corpus/jsonrpc_envelope/* "$$tmp/envelope/"; \
	$(EXTERNAL_RUN) cargo +nightly fuzz run frame-decoder "$$tmp/frame" -- -runs=10000 -max_len=1048577 -rss_limit_mb=2048; \
	$(EXTERNAL_RUN) cargo +nightly fuzz run jsonrpc-envelope "$$tmp/envelope" -- -runs=10000 -max_len=1048577 -rss_limit_mb=2048

## test: Run all tests
test:
	$(EXTERNAL_RUN) go test ./...

## test-race: Run all tests with the race detector
test-race:
	$(EXTERNAL_RUN) go test -race ./...

## test-memory-large: Run bounded embedding storage measurements explicitly.
## Override MEMORY_STORAGE_SCALE up to 10000 only when the disk budget is known.
MEMORY_STORAGE_SCALE ?= 1000
MEMORY_LARGE_TEST_TIMEOUT ?= 20m
test-memory-large:
	$(EXTERNAL_RUN) env SYMBRAIN_MEMORY_STORAGE_SCALE=$(MEMORY_STORAGE_SCALE) go test -tags memory_large -timeout $(MEMORY_LARGE_TEST_TIMEOUT) -run 'TestEmbedding(StorageSize|BackupSize|Recommendation)$$' -count=1 ./internal/memory/db

## vet: Run go vet static analysis
vet:
	$(EXTERNAL_RUN) go vet ./...

## lint: Deterministic lint gate (go vet + gofmt check, matches CI)
lint: vet fmt-check

# gofmt walks the raw filesystem and does not respect Go module boundaries
# the way `go build ./...`/`go vet ./...` do, so a bare `gofmt .` would
# sweep into any nested Go module (its own go.mod) that lives in this repo.
# browse/ is one such nested module (source-intake receiving copy, see
# browse/SOURCE_PROVENANCE.md); add further -not -path exclusions here if
# another nested go.mod directory is introduced later.
# Keep file discovery in find so its POSIX `-exec ... {} +` batching stays
# below the platform's exec limit instead of expanding every path in make.
GOFMT_FIND := find . -name '*.go' -not -path './browse/*' -not -path './.git/*' -exec

## fmt: Format all Go source files
fmt:
	$(GOFMT_FIND) gofmt -w -s {} +

## fmt-check: Fail if gofmt would change any file
fmt-check:
	@set -eu; \
	tmp_dir="$$(mktemp -d)"; \
	trap 'rm -rf "$$tmp_dir"' EXIT; \
	if ! $(GOFMT_FIND) gofmt -l {} + >"$$tmp_dir/unformatted"; then \
		echo "gofmt failed" >&2; \
		exit 1; \
	fi; \
	if [ -s "$$tmp_dir/unformatted" ]; then \
		echo "gofmt needed on:"; \
		cat "$$tmp_dir/unformatted"; \
		exit 1; \
	fi

## clean: Remove build artifacts and test cache
clean:
	$(EXTERNAL_RUN) rm -f "$(EXTERNAL_ARTIFACT_ROOT)/$(BINARY)"
	$(EXTERNAL_RUN) go clean -testcache
	$(EXTERNAL_RUN) cargo clean
