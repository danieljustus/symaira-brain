# Symaira Brain (symbrain)
# Portable agent-context layer for AI harnesses

BINARY := symbrain
MODULE := github.com/danieljustus/symaira-brain
VERSION ?= $(shell git describe --tags --always --dirty 2>/dev/null || echo "dev")
LDFLAGS := -X main.version=$(VERSION)

.PHONY: build test test-race coverage lint fmt-check fmt vet clean

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

## test: Run all tests
test:
	go test ./...

## test-race: Run all tests with the race detector
test-race:
	go test -race ./...

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
