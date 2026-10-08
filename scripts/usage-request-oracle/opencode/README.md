# OpenCode discovery recording oracle

This is a deterministic production-Go recording transport, **not live account
or endpoint evidence**. The generator drives `usage.NewOpenCodeProvider` and
`usage.RunStrategyChain` with dummy credentials and scripted responses. It never
uses the default HTTP transport. HOME is isolated, and both OpenCode environment
variables are replaced before provider construction.

## Run

From the repository root:

```sh
# Read-only freshness gate; also part of make rust-check.
make usage-opencode-check

# Deliberate regeneration, followed by the actual Rust replay.
bash scripts/run-external-env.sh env GOTOOLCHAIN=go1.26.7 go run ./scripts/usage-request-oracle/opencode -oracle-revision a92385d2deecc08d1fd96869908b81b7abd355fe -oracle-toolchain go1.26.7
bash scripts/run-external-env.sh cargo test --manifest-path "$PWD/Cargo.toml" -p symbrain-usage --locked
```

The generator rejects changed production behavior source or the wrong Go
runtime. Its behavior-source reference is the reachable release commit
`a92385d2deecc08d1fd96869908b81b7abd355fe`, passed explicitly by the Makefile
from the independent Rust trust anchor. The dependency-update corpus is written
to `rust/symbrain-usage/tests/fixtures/opencode_discovery_go_dependency_update_20261008.json`;
the original `opencode_discovery.json` remains byte-for-byte as the historical
record. The refreshed corpus retains identical cases, while its behavior-source
hashes exclude `go.mod` and `go.sum`: those files govern dependency resolution,
not the recorded provider behavior. The native gate still pins source/testdata
and generator/comparator hashes. Text source hashing normalizes CRLF to LF;
recorded request bodies are not normalized. Rust verifies exact hash inventories
and uses independent full-fixture and evidence-script digests, with negative
controls against changed hashes, expected results, and requests. `-check`
regenerates the oracle in memory and compares without overwriting the target.

## Compared contracts

- Entire executed GET/POST sequence, URL, body, and header values (case-insensitive
  header names); only dummy cookie and random request-instance values are folded.
- Empty strategy chain, workspace overrides and discovery, signed-out responses,
  HTTP errors, JSON-first/JS-literal parsing, and fallback decisions.
- Meter labels, used/limit/unit, reset presence, and exact integer-nanosecond
  reset offsets relative to each snapshot's own `fetched_at`.
- Encoded path components, invalid host/user escapes, invalid ports and accepted
  arbitrarily large numeric ports against the pinned Go `net/url` implementation.

## Explicit limits — do not close USE-001 from this corpus

The network-error case injects the recorded **input** (`boom`) verbatim. Rust's
raw transport error and Go's HTTP-client-wrapped error differ: this case verifies
only stable network class and original detail, not byte-identical error text.
The previous replay derived its input from the expected error and concealed this
mismatch; that result is superseded.

Authenticated live-response acceptance, production cancellation/body-read/size
boundaries, non-UTF-8 decoded workspace bytes, and full parser limits remain
outside this recording claim. Go iterates unnamed JSON-object children in random
map order, so multiple unordered unnamed windows need a separate contract decision;
this deterministic corpus does not manufacture an ordering guarantee. The existing
credential/live-fetch Go fallback and all production transport limits are unchanged.

Local fixture parity is not native-platform CI, released-artifact verification,
or cutover approval. Preserve the Go implementation.
