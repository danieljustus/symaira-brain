# Symaira Brain (symbrain)
# Portable agent-context layer for AI harnesses

BINARY := symbrain
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
# The repository is Rust-only. The historical Go behavior is replayed from
# committed, sha256-sealed fixtures (scripts/oracle-freeze); no target needs a
# Go toolchain.

EXTERNAL_ARTIFACT_ROOT := .
EXTERNAL_CARGO_TARGET_DIR := target
ifeq ($(strip $(CI)),)
ifeq ($(shell uname -s),Darwin)
EXTERNAL_WORKTREE_KEY := $(shell printf '%s' '$(CURDIR)' | shasum -a 256 | cut -c1-16)
SYMAIRA_EXTERNAL_BASE := $(or $(SYMAIRA_EXTERNAL_BASE),/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/builds/symaira-brain)
EXTERNAL_ARTIFACT_ROOT := $(SYMAIRA_EXTERNAL_BASE)/artifacts
EXTERNAL_CARGO_TARGET_DIR := $(SYMAIRA_EXTERNAL_BASE)/cargo-target/$(EXTERNAL_WORKTREE_KEY)
endif
endif
EXTERNAL_RUN := SYMAIRA_EXTERNAL_BASE="$(SYMAIRA_EXTERNAL_BASE)" bash "$(CURDIR)/scripts/run-external-env.sh"

.PHONY: rust-coverage build build-rust parity-smoke parity-smoke-record-fixture init-differential oracle-freeze-check oracle-freeze-selftest workflow-policy-check rust-replay-checks usage-gateway-lifecycle-check rust-go-printable-check usage-oracle-check usage-opencode-check activity-cli-oracle-check policy-oracle-check xdg-oracle-check catalog-oracle-check audit-oracle-check patterns-activity-oracle-check mcp-oracle-check gateway-oracle-check broker-oracle-check managed-oracle-check skills-oracle-check skills-runner-oracle-check skills-config-toml-oracle-check skills-raw-oracle-check instructions-oracle-check adapters-oracle-check install-oracle-check profile-remove-oracle-check cli-oracle-check guard-oracle-check guard-doctor-oracle-check guard-scan-oracle-check guard-scan-oracle-test guard-scan-parity rust-guard-check memory-oracle-check rust-audit rust-deny rust-fast rust-check rust-fuzz-build rust-fuzz-smoke rust-fetch-static-slice rust-fetch-fuzz-build rust-fetch-fuzz-smoke test test-race coverage lint actionlint fmt-check fmt vet clean oracle-freeze-seal

## coverage: Rust workspace line coverage (floor enforced) plus coverage.json and the badge files
coverage: rust-coverage
	@set -eu; \
	 summary="$(EXTERNAL_CARGO_TARGET_DIR)/coverage/summary.json"; \
	 total="$$(python3 scripts/rust-coverage/badge.py "$$summary" "$(EXTERNAL_ARTIFACT_ROOT)/coverage.json")"; \
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

## oracle-freeze-seal: re-seal hashes after a deliberate, reviewed fixture change
oracle-freeze-seal:
	$(ORACLE_FREEZE) seal

# Per-oracle names kept for muscle memory and CI: each verifies only its own
# sealed group.
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
memory-oracle-check: ; $(ORACLE_FREEZE) verify --group memory-sync-oracle --group memory-native-oracle

## guard-scan-oracle-test: Run Guard scan oracle fixture tests (python unittest, no Go toolchain)
guard-scan-oracle-test:
	./guard/scripts/guard-scan-oracle/run.sh test

## guard-scan-parity: replay the frozen Guard scan bytes against the freshly built native binary
guard-scan-parity: build-rust
	./guard/scripts/guard-scan-oracle/run.sh parity "$(EXTERNAL_CARGO_TARGET_DIR)/debug/symbrain"

## usage-gateway-lifecycle-check: production usage discovery is lazy and per call
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
INIT_ORACLE_COMMIT := $(shell sed -n 's/^PINNED_COMMIT_SHA = "\([0-9a-f]*\)"$$/\1/p' scripts/init-oracle/compare.py)
init-differential:
	@# The oracle tests read the pinned Go tree from git; CI checkouts are shallow.
	git cat-file -e "$(INIT_ORACLE_COMMIT)^{commit}" 2>/dev/null || git fetch --no-tags --depth=1 origin "$(INIT_ORACLE_COMMIT)"
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

## rust-fetch-static-slice: Run the Browse static-document slice (RUST-008, FETCH-006/007/008)
#
# A thin recipe over real tests: the sealed 28-vector render corpus (bytes),
# selector/cache/pipeline controls, HTTP-backed batch, Wayback and 404/410
# recovery (JSON-semantic) and the stable replay of the dom-input fuzz seeds.
# Browse is its own Cargo workspace, so the run happens inside browse/.
rust-fetch-static-slice:
	cd browse && $(EXTERNAL_RUN) cargo test -p symbrowse-fetch --locked \
		--test render_corpus --test static_controls --test pipeline_controls \
		--test selector_contract --test cache_contract --test batch_http \
		--test archive_http --test recovery_http --test dom_input_smoke

## rust-fetch-fuzz-build: Compile the Browse dom-input fuzz target with nightly libFuzzer
rust-fetch-fuzz-build:
	cd browse && $(EXTERNAL_RUN) cargo metadata --locked --format-version 1 --manifest-path fuzz/Cargo.toml >/dev/null
	cd browse && $(EXTERNAL_RUN) cargo +nightly fuzz build dom-input

## rust-fetch-fuzz-smoke: Run dom-input over a scratch copy of its tracked seeds
rust-fetch-fuzz-smoke: rust-fetch-fuzz-build
	@set -eu; \
	tmp=$$($(EXTERNAL_RUN) sh -c 'mktemp -d "$$TMPDIR/symbrowse-dom.XXXXXX"'); \
	trap 'rm -rf "$$tmp"' EXIT INT TERM; \
	cp browse/fuzz/corpus/dom_input/* "$$tmp/"; \
	cd browse && $(EXTERNAL_RUN) cargo +nightly fuzz run dom-input "$$tmp" -- -runs=2000 -max_len=65536 -rss_limit_mb=2048

## test: Run all Rust tests
test:
	$(EXTERNAL_RUN) cargo test --workspace --all-features --locked

## test-race: Alias of `test`; Rust has no -race flag (data races are compile errors)
test-race: test

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
