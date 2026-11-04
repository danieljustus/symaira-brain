# Symaira Brain (symbrain)
# Portable agent-context layer for AI harnesses

BINARY := symbrain
MODULE := github.com/danieljustus/symaira-brain
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
# `make rust-check` and every target it reaches are Go-free: Go oracle output is
# replayed from committed, sha256-sealed fixtures (scripts/oracle-freeze).
# Anything that still needs a Go toolchain is named `record-*` or `legacy-go-*`
# and is NOT part of the gate; those targets are deleted together with the Go
# sources.
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
EXTERNAL_RUN := SYMAIRA_EXTERNAL_BASE="$(SYMAIRA_EXTERNAL_BASE)" bash "$(CURDIR)/scripts/run-external-env.sh"

.PHONY: rust-coverage build build-rust parity-smoke parity-smoke-record-fixture init-differential oracle-freeze-check oracle-freeze-selftest workflow-policy-check rust-replay-checks usage-gateway-lifecycle-check rust-go-printable-check usage-oracle-check usage-opencode-check activity-cli-oracle-check policy-oracle-check xdg-oracle-check catalog-oracle-check audit-oracle-check patterns-activity-oracle-check mcp-oracle-check gateway-oracle-check broker-oracle-check managed-oracle-check skills-oracle-check skills-runner-oracle-check skills-config-toml-oracle-check skills-raw-oracle-check instructions-oracle-check adapters-oracle-check install-oracle-check profile-remove-oracle-check cli-oracle-check guard-oracle-check guard-doctor-oracle-check guard-scan-oracle-check guard-scan-oracle-test guard-scan-parity rust-guard-check db-memory-oracle-check rust-audit rust-deny rust-fast rust-check rust-fuzz-build rust-fuzz-smoke test test-race test-memory-large coverage lint actionlint fmt-check fmt vet clean record-oracle-check record-oracle-fixtures record-oracle-seal record-go-fallback-binary record-parity-smoke record-init-differential record-usage-opencode-historical legacy-go-build legacy-go-test legacy-go-test-race legacy-go-lint legacy-go-fmt legacy-go-fmt-check legacy-go-vet

## coverage: LEGACY Go coverage badge (needs Go; default goal is pinned by .github/tests; not part of the Go-free gate)
coverage:
	@set -eu; \
	 $(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_ARTIFACT_ROOT)"; \
	 tmp_dir="$$($(EXTERNAL_RUN) sh -c 'mktemp -d "$${TMPDIR:-/tmp}/symbrain.XXXXXX"')"; \
	 trap 'rm -rf "$$tmp_dir"' EXIT; \
	 profile="$${COVERAGE_PROFILE:-$(EXTERNAL_ARTIFACT_ROOT)/coverage.out}"; \
	 test_log="$${COVERAGE_LOG:-$$tmp_dir/test.log}"; \
	 if [ -z "$${COVERAGE_PROFILE:-}" ]; then \
		 if ! $(EXTERNAL_RUN) go test ./... -coverprofile="$$profile" > "$$test_log" 2>&1; then cat "$$test_log"; exit 1; fi; \
		 cat "$$test_log"; \
	 fi; \
	 $(EXTERNAL_RUN) python3 scripts/rust-coverage/go_oracles.py --baseline "$$profile" --out "$(EXTERNAL_ARTIFACT_ROOT)/coverage-combined.out" --receipt "$(EXTERNAL_ARTIFACT_ROOT)/coverage-oracles.json"; \
	 $(EXTERNAL_RUN) python3 scripts/rust-coverage/go_oracles.py --summary-only "$(EXTERNAL_ARTIFACT_ROOT)/coverage-combined.out" --out "$(EXTERNAL_ARTIFACT_ROOT)/coverage.json"; \
	 total="$$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["total"])' "$(EXTERNAL_ARTIFACT_ROOT)/coverage.json")"; \
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

## build: Compile the Rust symbrain binary (release)
build:
	$(EXTERNAL_RUN) env SYMBRAIN_VERSION="$(VERSION)" cargo build --release --locked -p symbrain-cli

## build-rust: Build every Rust workspace member
build-rust:
	$(EXTERNAL_RUN) env SYMBRAIN_VERSION="$(VERSION)" cargo build --workspace --locked

# ---------------------------------------------------------------------------
# Go-free fixture replay
# ---------------------------------------------------------------------------
PYTHON ?= python3
ORACLE_FREEZE := $(EXTERNAL_RUN) $(PYTHON) scripts/oracle-freeze/freeze.py
ORACLE_RECORD := $(EXTERNAL_RUN) $(PYTHON) scripts/oracle-freeze/record.py
REQUIRE_RAN := $(EXTERNAL_RUN) $(PYTHON) scripts/oracle-freeze/require_ran.py

## oracle-freeze-check: FAIL unless every committed Go-oracle fixture matches its sealed sha256 + provenance
oracle-freeze-check:
	$(ORACLE_FREEZE) verify

## oracle-freeze-selftest: negative controls for the fixture gate (mutated field, missing file, forged hash, ...)
oracle-freeze-selftest:
	$(EXTERNAL_RUN) $(PYTHON) -m unittest discover -s scripts/oracle-freeze -p 'test_*.py'

## workflow-policy-check: Go-free static policy checks of this Makefile, deny.toml and the gate closure
workflow-policy-check:
	$(EXTERNAL_RUN) $(PYTHON) -m unittest discover -s .github/tests -p 'test_*.py'

# Per-oracle names kept for muscle memory and CI: each verifies only its own
# sealed group, without Go. Regeneration is `make record-oracle-check`.
rust-go-printable-check: ; $(ORACLE_FREEZE) verify --group go-printable-table
usage-oracle-check: ; $(ORACLE_FREEZE) verify --group usage-oracle --group usage-request-oracle --group secret-oracle
usage-opencode-check: ; $(ORACLE_FREEZE) verify --group usage-opencode-dependency-update
activity-cli-oracle-check: ; $(ORACLE_FREEZE) verify --group activity-cli-oracle
policy-oracle-check: ; $(ORACLE_FREEZE) verify --group policy-oracle
xdg-oracle-check: ; $(ORACLE_FREEZE) verify --group xdg-oracle
cli-oracle-check: ; $(ORACLE_FREEZE) verify --group cli-oracle
catalog-oracle-check: ; $(ORACLE_FREEZE) verify --group catalog-oracle
audit-oracle-check: ; $(ORACLE_FREEZE) verify --group audit-oracle
patterns-activity-oracle-check: ; $(ORACLE_FREEZE) verify --group patterns-activity-oracle
mcp-oracle-check: ; $(ORACLE_FREEZE) verify --group mcp-oracle
gateway-oracle-check: ; $(ORACLE_FREEZE) verify --group gateway-oracle
broker-oracle-check: ; $(ORACLE_FREEZE) verify --group broker-oracle
managed-oracle-check: ; $(ORACLE_FREEZE) verify --group managed-oracle
skills-oracle-check: ; $(ORACLE_FREEZE) verify --group skills-oracle --group skills-install-oracle
skills-runner-oracle-check: ; $(ORACLE_FREEZE) verify --group skills-runner-oracle
skills-config-toml-oracle-check: ; $(ORACLE_FREEZE) verify --group skills-config-toml-oracle
skills-raw-oracle-check: ; $(ORACLE_FREEZE) verify --group skills-native-oracle
instructions-oracle-check: ; $(ORACLE_FREEZE) verify --group instructions-oracle
adapters-oracle-check: ; $(ORACLE_FREEZE) verify --group adapters-oracle
install-oracle-check: ; $(ORACLE_FREEZE) verify --group install-oracle-darwin --group install-oracle-linux --group install-oracle-windows
profile-remove-oracle-check: ; $(ORACLE_FREEZE) verify --group profile-remove-oracle-darwin --group profile-remove-oracle-linux
guard-oracle-check: ; $(ORACLE_FREEZE) verify --group guard-oracle --group guard-capability-oracle
guard-doctor-oracle-check: ; $(ORACLE_FREEZE) verify --group guard-doctor-oracle
guard-scan-oracle-check: ; $(ORACLE_FREEZE) verify --group guard-scan-oracle --group guard-grants-oracle --group guard-decide-oracle
# memory oracles are frozen by the memory workers; their fixture is outside this gate.
db-memory-oracle-check: ; @echo "db-memory-oracle: fixture frozen by the memory workers (rust/symbrain-memory/**); Rust replay runs in cargo test"

## guard-scan-oracle-test: Run Guard scan oracle fixture tests (python unittest, no Go toolchain)
guard-scan-oracle-test:
	./guard/scripts/guard-scan-oracle/run.sh test

## guard-scan-parity: replay the frozen Guard scan bytes against the freshly built native binary
guard-scan-parity: build-rust
	./guard/scripts/guard-scan-oracle/run.sh parity "$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain"

## usage-gateway-lifecycle-check: production usage discovery is lazy and per call (Rust gateway test; replaces the Go test)
usage-gateway-lifecycle-check:
	$(REQUIRE_RAN) --min 1 -- cargo test --locked -p symbrain-gateway --test usage_tests -- --exact production_usage_discovery_is_lazy_and_per_call

## rust-replay-checks: every Go-free fixture/contract replay that is not a plain cargo test
rust-replay-checks: oracle-freeze-selftest oracle-freeze-check workflow-policy-check usage-gateway-lifecycle-check guard-scan-oracle-test

# ---------------------------------------------------------------------------
# Parity against the frozen Go transcripts
# ---------------------------------------------------------------------------
PARITY_OS := $(shell uname -s | tr "[:upper:]" "[:lower:]")
PARITY_FIXTURE := rust/symbrain-cli/tests/fixtures/rust_differential_go.$(PARITY_OS).json

## parity-smoke: Replay the frozen Go transcripts of the black-box CLI differential against the Rust binary
parity-smoke:
	$(EXTERNAL_RUN) env SYMBRAIN_VERSION=dev cargo build --workspace --locked
	./guard/scripts/guard-scan-oracle/run.sh parity "$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain"
	$(EXTERNAL_RUN) env SYMBRAIN_VERSION=dev $(PYTHON) scripts/rust-differential.py replay "$(PARITY_FIXTURE)" "$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain"

## init-differential: Replay the frozen Go transcripts of `symbrain init` against the Rust binary
INIT_RUST_BINARY ?= target/debug/symbrain$(if $(filter Windows_NT,$(OS)),.exe,)
INIT_OS := $(shell uname -s | tr "[:upper:]" "[:lower:]")
INIT_FIXTURE := rust/symbrain-cli/tests/fixtures/init_differential_go.$(INIT_OS).json
init-differential:
	$(EXTERNAL_RUN) cargo build -p symbrain-cli --locked
	$(EXTERNAL_RUN) $(PYTHON) scripts/init-oracle/test_compare.py
	$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_ARTIFACT_ROOT)/init-oracle"
	$(EXTERNAL_RUN) $(PYTHON) scripts/init-oracle/compare.py replay --fixture "$(INIT_FIXTURE)" --rust-binary "$(if $(CI),$(INIT_RUST_BINARY),$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain$(if $(filter Windows_NT,$(OS)),.exe,))" --output "$(EXTERNAL_ARTIFACT_ROOT)/init-oracle/report.json"

## rust-guard-check: Guard model/static-kernel/capability fixtures (sealed) + the Rust workspace checks
rust-guard-check: guard-oracle-check
	$(EXTERNAL_RUN) cargo fmt --all --check
	$(EXTERNAL_RUN) cargo check --workspace --all-targets --all-features --locked
	$(EXTERNAL_RUN) cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	$(EXTERNAL_RUN) cargo test --workspace --all-targets --all-features --locked
	$(EXTERNAL_RUN) cargo test --workspace --doc --all-features --locked

## rust-coverage: Enforce the tracked Rust workspace line-coverage floor
rust-coverage:
	$(EXTERNAL_RUN) bash scripts/rust-coverage/run.sh "$(EXTERNAL_CARGO_TARGET_DIR)/coverage"

## rust-audit: Audit the committed lockfile without resolving or updating it
rust-audit:
	$(EXTERNAL_RUN) cargo audit --file Cargo.lock --deny warnings
	$(EXTERNAL_RUN) cargo audit --file fuzz/Cargo.lock --deny warnings

## rust-deny: Enforce dependency/license policy without changing Cargo.lock
rust-deny:
	$(EXTERNAL_RUN) cargo deny --locked --all-features check
	$(EXTERNAL_RUN) cargo deny --manifest-path fuzz/Cargo.toml --locked --all-features check

## rust-fast: Run focused Rust migration checks for the CLI, skills, and Guard
rust-fast:
	$(EXTERNAL_RUN) cargo fmt --all --check
	$(EXTERNAL_RUN) cargo test -p symbrain-cli -p symbrain-skills -p symbrain-guard-core --locked

## rust-check: Run the complete Go-free Rust quality gate
#
# Go oracle output is replayed from committed fixtures with provenance
# (scripts/oracle-freeze, migration/evidence/oracle-freeze-781). The gate FAILS
# when a fixture is missing, changed or unregistered; it never skips. No target
# below invokes a Go toolchain or Go binary (enforced by
# .github/tests/test_make_gate.py through `make -n`).
rust-check: rust-replay-checks
	$(EXTERNAL_RUN) cargo fmt --all --check
	$(EXTERNAL_RUN) cargo check --workspace --all-targets --all-features --locked
	$(EXTERNAL_RUN) cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
	$(EXTERNAL_RUN) cargo test --workspace --all-features --locked
	$(EXTERNAL_RUN) cargo test --workspace --doc --all-features --locked
	$(EXTERNAL_RUN) cargo audit
	$(EXTERNAL_RUN) cargo deny check
	$(EXTERNAL_RUN) cargo audit --file fuzz/Cargo.lock --deny warnings
	$(EXTERNAL_RUN) cargo deny --manifest-path fuzz/Cargo.toml --locked --all-features check

## rust-fuzz-build: Compile every MCP fuzz target with nightly libFuzzer
rust-fuzz-build:
	$(EXTERNAL_RUN) cargo metadata --locked --format-version 1 --manifest-path fuzz/Cargo.toml >/dev/null
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

## test: Run all Rust tests
test:
	$(EXTERNAL_RUN) cargo test --workspace --all-features --locked

## test-race: Alias of `test`; Rust has no -race flag (data races are compile errors)
test-race: test

## test-memory-large: memory storage measurements are owned by the memory workers (Go-only today)
test-memory-large: legacy-go-test-memory-large
MEMORY_STORAGE_SCALE ?= 1000
MEMORY_LARGE_TEST_TIMEOUT ?= 20m
.PHONY: legacy-go-test-memory-large
legacy-go-test-memory-large:
	$(EXTERNAL_RUN) env SYMBRAIN_MEMORY_STORAGE_SCALE=$(MEMORY_STORAGE_SCALE) go test -tags memory_large -timeout $(MEMORY_LARGE_TEST_TIMEOUT) -run 'TestEmbedding(StorageSize|BackupSize|Recommendation)$$' -count=1 ./internal/memory/db

## vet: Rust static analysis (clippy, warnings are errors)
vet:
	$(EXTERNAL_RUN) cargo clippy --workspace --all-targets --all-features --locked -- -D warnings

## lint: Deterministic lint gate (rustfmt check + clippy -D warnings)
lint: fmt-check vet

## fmt: Format Rust sources
fmt:
	$(EXTERNAL_RUN) cargo fmt --all

## fmt-check: Fail if rustfmt would change any Rust source
fmt-check:
	$(EXTERNAL_RUN) cargo fmt --all --check

# Same check as .github/workflows/actionlint.yml; needs actionlint and shellcheck on PATH.
actionlint:
	actionlint -color

## clean: Remove build artifacts
clean:
	$(EXTERNAL_RUN) rm -f "$(EXTERNAL_ARTIFACT_ROOT)/$(BINARY)"
	$(EXTERNAL_RUN) cargo clean

# ---------------------------------------------------------------------------
# Go-dependent helpers. NOT part of any gate. Deleted together with the Go sources.
# ---------------------------------------------------------------------------

## record-oracle-check: (needs Go) regenerate every Go oracle in a scratch worktree and compare byte-for-byte
record-oracle-check:
	$(ORACLE_RECORD) check --seal

## record-oracle-fixtures: (needs Go) deliberately re-record Go-generated fixtures and re-seal the manifest
record-oracle-fixtures:
	$(ORACLE_RECORD) record $(foreach g,$(GROUPS),--group $(g))

## record-oracle-seal: re-seal hashes after a manual fixture change (no Go needed)
record-oracle-seal:
	$(ORACLE_FREEZE) seal

## record-go-fallback-binary: (needs Go) build the Go binary used to re-record process transcripts
record-go-fallback-binary:
	@$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_GO_ARTIFACT_ROOT)"
	$(EXTERNAL_RUN) env CGO_ENABLED=0 go build -ldflags "-X main.version=dev" -o "$(abspath $(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go)" ./cmd/symbrain

## record-parity-smoke: (needs Go) re-record the black-box CLI differential transcripts
record-parity-smoke: record-go-fallback-binary
	$(EXTERNAL_RUN) $(PYTHON) scripts/rust-differential.py record "$(PARITY_FIXTURE)" "$(EXTERNAL_GO_ARTIFACT_ROOT)/symbrain-go"

## record-init-differential: (needs Go) re-record the `symbrain init` differential transcripts for this OS
record-init-differential:
	$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_ARTIFACT_ROOT)/init-oracle"
	$(EXTERNAL_RUN) $(PYTHON) scripts/init-oracle/compare.py record --repo-root "$(CURDIR)" --work-dir "$(EXTERNAL_ARTIFACT_ROOT)/init-oracle/record" --fixture "$(INIT_FIXTURE)"

## record-usage-opencode-historical: (needs Go) revalidate the frozen generator against its authentic immutable source
record-usage-opencode-historical:
	@test -n "$(HISTORICAL_USAGE_ROOT)"
	@test "$$(git -C "$(HISTORICAL_USAGE_ROOT)" rev-parse HEAD)" = b807eae643c627ae31166b90176d5a0de6f4e647
	@test -z "$$(git -C "$(HISTORICAL_USAGE_ROOT)" status --porcelain=v1 --untracked-files=all)"
	$(MAKE) -C "$(HISTORICAL_USAGE_ROOT)" usage-opencode-check

## legacy-go-*: the pre-migration Go gates, kept only until the Go sources are deleted
legacy-go-build:
	$(EXTERNAL_RUN) mkdir -p "$(EXTERNAL_ARTIFACT_ROOT)"
	$(EXTERNAL_RUN) env CGO_ENABLED=0 go build -ldflags "$(LDFLAGS)" -o "$(EXTERNAL_ARTIFACT_ROOT)/$(BINARY)" ./cmd/symbrain

legacy-go-test:
	$(EXTERNAL_RUN) go test ./...

legacy-go-test-race:
	$(EXTERNAL_RUN) go test -race ./...

legacy-go-vet:
	$(EXTERNAL_RUN) go vet ./...

legacy-go-lint: legacy-go-vet legacy-go-fmt-check

# gofmt walks the raw filesystem and does not respect Go module boundaries
# the way `go build ./...`/`go vet ./...` do, so a bare `gofmt .` would
# sweep into any nested Go module (its own go.mod) that lives in this repo.
# browse/ is one such nested module (source-intake receiving copy, see
# browse/SOURCE_PROVENANCE.md); add further -not -path exclusions here if
# another nested go.mod directory is introduced later.
# Keep file discovery in find so its POSIX `-exec ... {} +` batching stays
# below the platform's exec limit instead of expanding every path in make.
# Linked checkouts are independent trees, not sources owned by this checkout.
# Original evidence is immutable input data, not formatter-owned source.
GOFMT_FIND := find . -name '*.go' -not -path './browse/*' -not -path './.git/*' -not -path './.worktrees/*' -not -path './migration/evidence/*' -exec

## legacy-go-fmt: Format owned Go source files
legacy-go-fmt:
	$(GOFMT_FIND) gofmt -w -s {} +

## legacy-go-fmt-check: Fail if gofmt would change any owned source file
legacy-go-fmt-check:
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

