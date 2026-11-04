# Native `symbrain usage` without a Go fallback (#768)

`symbrain usage` used to hand the whole report to the Go binary whenever a
credential configuration was not yet proven natively (custom base URLs,
unusual OpenCode workspace spellings, ambiguous Claude/Copilot tokens, JWT
float overflow, Kimi file/device shapes, Windows home mismatch, automatic
Claude Keychain discovery). All of those predicates are deleted; this oracle is
the evidence that the native path answers them exactly as Go does.

* `gates_test.go.txt` — supplemental Go test (dropped into `internal/usage` of
  a disposable `git archive`, production Go untouched). It drives the real
  provider constructors and `BuildReport` for 425 cases with synthetic
  credentials in a disposable HOME, an injected Claude Keychain reader and a
  recording transport that first applies `net/http`'s own request validation
  (a real `http.Transport` with a failing dial) and then returns a canned
  reply. `TestNativeURLCorpus768` records ~2100 custom-base spellings
  (`trustedHTTPSURL`, `validatedUsageBase`, `http.NewRequest(...).URL.String()`).
* `run.sh` — regenerates `rust/symbrain-usage/tests/fixtures/usage_native_gates_<os>.json`
  (run once per OS; darwin and linux are committed because the Kimi platform
  label and symlink resolution are per OS). The URL corpus is
  `usage_url_corpus.json` (identical on go1.26.10 linux and go1.27.3 darwin).
* `cli.py GO_BINARY RUST_BINARY [--record FIXTURE]` — real CLI stdout/stderr/exit
  comparison (64 network-free scenarios) with a failing `SYMBRAIN_GO_BINARY`
  sentinel; `--record` writes `rust/symbrain-cli/tests/fixtures/usage_native_gates_cli.json`.

Replays (no Go needed): `cargo test -p symbrain-usage --lib native_gates`,
`... go_url_corpus`, `cargo test -p symbrain-cli --test usage_native_gates_cli`.

Nothing contacts a live endpoint and the system Keychain is only reached through
the injectable probe. Known, deliberate differences from Go are listed in the
migration report: sorted instead of random choice among several distinct
Claude/Copilot tokens (both are Go-possible outcomes), a constant OpenCode
`X-Server-Instance` UUID, and Punycode-without-NFC for non-ASCII custom hosts.
