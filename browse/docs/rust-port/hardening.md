# Rust hardening gates

RUST-015 hardens the Rust port without declaring browser or MCP parity that has
not been implemented. The gates cover the deterministic boundaries that already
exist: daemon/MCP JSON frames, flow YAML, state headers and authenticated
payloads, URL policy, robots/CDX/Retry-After text, HTTP body decoding, and
cleaned HTML/selector boundaries.

All commands run from `browse/`, and every cargo-driven command goes through
`dev-external` so build output stays outside the repository.

## Fuzzing

The fuzz package is an isolated Cargo workspace at `fuzz/` with its own
committed `Cargo.lock`. `libfuzzer-sys` is exact-pinned (`=0.4.13`), and the
package is never a member of the production workspace. Targets (cargo-fuzz
name, corpus directory under `fuzz/corpus/` uses underscores):

| Target | Untrusted input |
| --- | --- |
| `daemon-mcp-frames` | daemon request frames (`decode_frame`) and raw MCP stdio framing (`serve_with_proxy` with a refusing proxy) |
| `flow-yaml` | flow YAML parse, validation and dry-run planning |
| `state-headers` | state-file magic, versioned JSON headers, legacy and authenticated bodies; state-name validation |
| `urls` | SSRF guard with a deterministic private resolver, allowlist parsing and matching |
| `fetch-text` | robots.txt, Wayback CDX JSON, Wayback URLs, `Retry-After` |
| `http-body-decode` | gzip/brotli/zstd content decoding (output stays within the limit) and charset transcoding |
| `dom-input` | HTML5 parse, cleanup, render, ranking and bounded selectors |
| `httparse-response` | HTTP/1 response heads, header blocks and chunk-size lines through the pinned `httparse` (what hyper/reqwest parse from remote servers) |
| `h2-client` | server-side bytes of an in-memory HTTP/2 connection through the pinned `h2` client (frame decoder, HPACK, flow control); the exchange must finish within a deadline |
| `tungstenite-frames` | WebSocket frames read by an established client socket and a client opening-handshake response through the pinned `tungstenite` (the version the Chrome, Firefox and Safari engines link), with message/frame caps |

Run the bounded smoke with `scripts/fuzz_smoke.sh`. It copies each tracked seed
corpus to a temporary directory, runs the configured number of executions
(`FUZZ_RUNS`, default 64, maximum 4096; `FUZZ_TOOLCHAIN` selects the nightly, default `nightly`), requires libFuzzer to report a
completed bounded run for every target, and checks the tracked seed digests
before and after. Do not pass `fuzz/corpus/*` directly to libFuzzer: libFuzzer
may write new interesting inputs there.

`scripts/fuzz_corpus_hashes.py --check` verifies `fuzz/corpus/hashes.json`.
Update the manifest (`--write`) only when deliberately changing a seed, then
review the seed and target together.

Every target also has a stable-Rust replay (`crates/*/tests/fuzz_seed_replay.rs`,
`crates/symbrowse-fetch/tests/dom_input_smoke.rs`, and
`crates/symbrowse-fetch/tests/parser_dependency_replay.rs` for `httparse-response` and `h2-client`) that runs the seeds, all of
their prefixes and a deterministic byte-mutation sweep in the normal test run.
Keep each target body and its replay in sync.

## Property tests

`symbrowse-core/tests/hardening_properties.rs` uses exact-pinned `proptest` for
bounded YAML totality, authenticated state round trips, state-name path safety,
allowlist label boundaries, and total IP classification. These tests exercise
pure code only and do not claim filesystem, subprocess, browser, DNS, or MCP
transport parity.

## Miri scope and explicit skips

Run `scripts/rust_miri.sh`. It invokes only the pure property target and the
pure daemon protocol unit tests. The following tests are intentionally excluded
from Miri because they require filesystem, process, socket, browser, or network
facilities:

- `symbrowse-core`: `config_contract`, `state_vectors`, `key_resolver`,
  `policy_guard`, `fuzz_seed_replay`, journal store tests, profile discovery
  tests, state-store tests, and custom-pattern file tests.
- `symbrowse-daemon`: lifecycle/session registry tests and all server/client
  socket or autostart tests.
- `symbrowse-fetch`: HTTP/archive tests, `fuzz_seed_replay` and cache/pipeline
  store tests.
- `symbrowse-mcp`: raw transport/proxy tests and `fuzz_seed_replay`.
- `symbrowse-engine-chrome` and `symbrowse-engine-safari`: launch, attach,
  subprocess, socket, and real-browser tests.

Adding a new pure Miri target requires adding it explicitly to the script; no
broad `--workspace` invocation is used as a substitute.

## Dependency and unsafe inventory

`security/rust-hardening-inventory.json` (relative to `browse/`) is generated
by `scripts/rust_hardening_inventory.py` (`--write` regenerates, `--check`
fails when stale). It contains every resolved production (normal and build)
dependency of the workspace from `cargo metadata --locked`, every owned
`unsafe` occurrence in workspace Rust sources including tests, examples and the
fuzz package, the `unsafe_code` lint policy of every crate root, and the
explicit status that transitive unsafe use requires the cargo-geiger run. The
script exits non-zero on any owned `unsafe` or any crate without a
`deny`/`forbid(unsafe_code)` root attribute. Checks:

```text
cargo audit
cargo audit --file fuzz/Cargo.lock
cargo deny check
cargo deny --manifest-path fuzz/Cargo.toml --config ../deny.toml check
scripts/rust_geiger.sh
```

`deny.toml` lives at the repository root. cargo-deny finds it for the workspace
check but not for `--manifest-path fuzz/Cargo.toml` (it then falls back to the
default config and rejects every license), so the fuzz check passes
`--config ../deny.toml` explicitly. The root `Makefile` runs from the
repository root and needs no flag.

`scripts/rust_geiger.sh` runs the pinned cargo-geiger version against each real
crate manifest (the root manifest is virtual) and writes one JSON report per
production package under `<target dir>/hardening/geiger/`. The reports are CI
artifacts; they are not hand-written. The script fails on any unsafe code in a
workspace-owned package. An advisory or license failure is a blocker; an unsafe
dependency is accepted only when the report identifies it and the entry is
reviewed here. Owned `unsafe` is not accepted by this milestone.

### Transitive unsafe review (2026-11-14)

cargo-geiger scanned 239 distinct packages (host target, all features): the
10 workspace crates contain 0 unsafe items; 145 third-party crates.io packages
contain unsafe, 29 forbid it, 94 contain none. All 145 resolve from the
crates.io registry only (`cargo deny` sources ok), have no RustSec advisory
(`cargo audit` clean) and carry allowed licenses. They fall in these accepted
categories: allocator/collection and string primitives (`bytes`, `hashbrown`,
`smallvec`, `slab`, `indexmap`, `memchr`, `regex-automata`, `string_cache`,
`zerovec`), concurrency/async runtime (`tokio`, `futures-*`, `parking_lot`,
`mio`, `socket2`, `signal-hook-registry`), OS/FFI bindings (`libc`, `rustix`,
`nix`, `errno`, `core-foundation*`, `security-framework`, `openssl*`,
`native-tls`), crypto/hash primitives (`aes`, `chacha20`, `sha1`, `sha2`,
`polyval`, `zeroize`, `getrandom`, `rand`), compression/text decoders
(`brotli*`, `flate2`, `ruzstd`, `encoding_rs`, `simd-adler32`, `utf-8`), and
protocol/serialization crates (`hyper`, `h2`, `httparse`, `tungstenite`,
`html5ever`, `serde*`, `url`, `idna`). This is a category-level acceptance of
widely used, advisory-clean crates, not a line-level audit. The dependencies
that consume attacker-controlled bytes (`html5ever`, `serde_json`, `brotli`,
`flate2`, `ruzstd`, `encoding_rs`, `url`, `idna`, `yaml-rust2`) are reached by
the fuzz targets above; `h2`, `httparse` and `tungstenite` are fuzzed directly
since 2026-11-15 (see below). cargo-geiger could
not parse `signal-hook-registry` 1.4.8 (`syn` error), so its count is missing
from the report.

## Findings fixed during the 2026-11-14 gate run

- **Owned unsafe in tests.** `chrome_native.rs` and `oracle_replay.rs`
  (symbrowse-daemon) wrapped `std::env::set_var` in `unsafe` blocks with
  `#[allow(unsafe_code)]`. Both now re-execute their own test binary through
  `common::isolated_environment_root`, which sets `HOME`/`XDG_*` and the
  hermetic policy variables on a child process, and both carry
  `#![deny(unsafe_code)]`. Verified against real headless Chrome: the oracle
  replay still matches 112/113 steps (1 documented known gap).
- **Missing fuzz coverage.** Only `dom-input` existed although this document
  and the finalization plan list the frame, flow, state, URL and HTML
  boundaries. Added `daemon-mcp-frames`, `flow-yaml`, `state-headers`, `urls`,
  `fetch-text` and `http-body-decode` (new public `#[doc(hidden)]`
  `fuzz_decode_content`/`fuzz_normalize_charset` wrappers over the previously
  private body decoder), seeds, the digest manifest and stable replays.
- **Missing scripts and inventory.** `fuzz_smoke.sh`, `fuzz_corpus_hashes.py`,
  `rust_miri.sh`, `rust_geiger.sh`, `rust_hardening_inventory.py` and
  `security/rust-hardening-inventory.json` were referenced but absent.
- **cargo-deny on the fuzz manifest.** `fuzz/Cargo.toml` declared its path
  dependency without a version, tripping `bans.wildcards = "deny"`. All path
  dependencies now carry `version = "=0.0.0"`, matching the workspace.
- **C1 control characters in state names (#1061).** A first `state-headers`
  fuzz run found that `validate_name` accepted C1 control characters
  (U+0080–U+009F) while rejecting ASCII controls and `/`. No Go-recorded
  fixture pins either behavior; because names are listed and logged, it now
  rejects every `char::is_control` character, and the fuzz/replay assertions
  state that contract.

## CI policy

`.github/workflows/browse-hardening.yml` (added 2026-11-15; the plan's
`rust-ci.yml`/`rust-hardening.yml` names were never created) runs the gates.
Triggers: pull requests and pushes to `main` touching `browse/**` or the
workflow, a weekly schedule (Mondays 06:17 UTC) and manual dispatch. Tools are
installed with `cargo install --locked --version` at the versions listed under
Evidence and cached; Miri and the fuzz smoke use the dated toolchain
`nightly-2026-10-29`.

| Job | Gates | Runs on |
| --- | --- | --- |
| `build-gates` | `cargo fmt --check`, `cargo check`, `cargo nextest run` (includes every fuzz-seed replay and the property tests), doc tests, `cargo hack check --each-feature --no-dev-deps` | PR, main, schedule, dispatch |
| `supply-chain` | `cargo audit` (+ `fuzz/Cargo.lock`), `cargo deny check` (+ fuzz manifest with `--config ../deny.toml`), `rust_hardening_inventory.py --check`, `fuzz_corpus_hashes.py --check` | PR, main, schedule, dispatch |
| `miri` | `scripts/rust_miri.sh` | main, schedule, dispatch |
| `geiger` | `scripts/rust_geiger.sh` (fails on owned unsafe; uploads the JSON reports) | main, schedule, dispatch |
| `fuzz-smoke` | `scripts/fuzz_smoke.sh` with `FUZZ_RUNS=4096` | main, schedule, dispatch |

Not repeated there because they already run elsewhere: `cargo clippy
--workspace --all-targets --all-features -- -D warnings` and `cargo test
--workspace` (`ci.yml` job `browse-native` on main/schedule/dispatch;
`browse-daemon-native.yml` runs the clippy gate on pull requests and the daemon
tests on ubuntu-24.04, ubuntu-24.04-arm and macos-15). Everything runs on
`ubuntu-latest`; no gate here is macOS-specific (the platform-specific
key-store and daemon behavior is covered by `browse-daemon-native.yml`).
Miri, geiger and the fuzz smoke stay out of pull requests because of their
cost; their inputs (seeds, replays, lockfiles) are still checked on every PR by
`build-gates` and `supply-chain`. The workflow is linted with `actionlint`; it
has not run on GitHub yet (it lands with this change), so the first hosted run
is the proof for the Linux-specific install steps.

## Environment notes

- Unix-socket tests fail with a long `TMPDIR` (macOS `sun_path` limit).
  `dev-external` forces a long external `TMPDIR`; under it five tests fail
  (`daemon::cancellation` ×4, `cli::mcp_frozen_go_frames`) with
  `path must be shorter than SUN_LEN`. All runs below therefore use
  `dev-external env TMPDIR=/tmp/gbr cargo …`. This is environmental only.
- cargo-nextest 0.9.143 is available, so no `cargo test` substitution was
  needed except for doctests, which nextest cannot run.

## Evidence — 2026-11-14

Base `11c58f88a`, macOS arm64, cargo/rustc 1.98.0 (pinned in
`rust-toolchain.toml`); nightly `1.101.0-nightly (92b5b8c87 2026-10-29)` for
fuzz and Miri. Tools: cargo-nextest 0.9.143, cargo-hack 0.6.45, cargo-audit
0.22.2 (RustSec DB, 1509 advisories), cargo-deny 0.20.2, cargo-geiger 0.13.0,
cargo-fuzz 0.13.2. No Safari was launched or automated.

RUST-015 (final run after all fixes; every exit code 0):

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo check --workspace --all-targets --all-features` | clean |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --workspace --all-features` | 435 passed, 0 failed, 3 skipped (the three `#[ignore]` child-process fixtures) |
| `cargo test --workspace --doc --all-features` | 9 doctest binaries, 1 passed, 0 failed |
| `cargo hack check --workspace --each-feature --no-dev-deps` | 10/10 packages |
| `scripts/rust_miri.sh` | `hardening_properties` 5/5 (19 s) and daemon `protocol::` 3/3 (368 s) under Miri; run before the fetch-only wrapper change, core/daemon library sources unchanged since |
| `cargo audit` / `--file fuzz/Cargo.lock` | no vulnerabilities (307 / 296 crates) |
| `cargo deny check` / fuzz manifest with `--config ../deny.toml` | advisories, bans, licenses, sources ok; 18 duplicate-version warnings each (`bans.multiple-versions = "warn"`), no ignores |
| `scripts/rust_geiger.sh` | 239 packages scanned; owned unsafe 0; 145 third-party packages with unsafe, triaged above |
| `scripts/rust_hardening_inventory.py --write` then `--check` | 295 production packages (10 workspace), 0 owned unsafe, 10/10 crates with `deny(unsafe_code)` |
| `scripts/fuzz_smoke.sh` (`FUZZ_RUNS=4096`) | all 7 targets completed 4096 runs; tracked seed digests unchanged |
| `scripts/fuzz_corpus_hashes.py --check` | 7 targets, 85 seeds, ok |
| Longer fuzz (not a gate) | 40–120 s each: `daemon-mcp-frames` 143k, `fetch-text` 473k, `flow-yaml` 489k, `state-headers` 1.66M, `urls` 700k, `http-body-decode` 754k executions, no crash after the C1-control assertion fix |

Stop rule: no owned unsafe (0 occurrences, every crate root and the two
formerly unsafe test binaries deny it); no untriaged advisory or license
violation; every untrusted parser listed above has a fuzz target and replay.

RUST-017:

| Command | Result |
| --- | --- |
| `cargo nextest run -p symbrowse-engine-chrome -p symbrowse-engine-safari -p symbrowse-engine-firefox` | 45 passed, 0 skipped (the Safari tests use fake osascript scripts only; no Safari launch) |
| `python3 port/harness/run.py --suite browser-transport --native-targets` | exit 0, `browser-transport suite passed`. Run as given: the suite only runs three cargo tests (`selection_is_exhaustive`, `retired_compat`, `compat_retired`); `--native-targets` is read only by the `chrome-full`/`safari`/`all` suites, so no browser (and no Safari) is launched |
| `cargo test -p symbrowse-cli --test compat_retired` | 6 passed: retired compat returns the typed `transport_retired` error for config, env, flag, daemon and MCP, and static/browser selection is unaffected |

2026-11-15 (RUST-017 close-out, macOS arm64, Chrome 156.0.8078.80, Firefox 160.0):

| Command | Result |
| --- | --- |
| `python3 port/harness/run.py --suite browser-transport --native-targets` | exit 0; CFG-006 selection tests, the typed-unavailable cases, then the shared native corpus: 30 verified cases (Chrome 9, Firefox 9, Firefox adapter 8, typed unavailable 2, Safari bundle-version info 1, Safari corpus skipped); Safari corpus skipped because the user's normal Safari was running (recorded in `target/native-evidence/browser-transport.json`) |
| `cargo test -p symbrowse-core --test config_contract` / `-p symbrowse-cli --test transport_selection --test compat_retired` / `-p symbrowse-daemon --test transport_status` | pass |

The first block above predates this close-out: `--native-targets` now runs the
real-engine corpus for `browser-transport`.

Not covered here: the Safari native gate (Remote Automation on the user's real
Safari, a maintainer step) and therefore RUST-013; Chrome/Firefox native
receipts are as recorded in the ledger. Miri was not rerun after the
fetch-only change because no Miri target depends on it.

## Direct fuzzing of h2, httparse and tungstenite (2026-11-15)

Decision: add thin direct targets (`h2-client`, `httparse-response`,
`tungstenite-frames`) instead of recording an accepted boundary. The three
crates are upstream-fuzzed, but the exact pinned versions parse bytes chosen by
remote parties through paths our existing targets do not reach, and the targets
are small enough to fit the existing seed/digest/replay layout.

Upstream fuzzing (checked 2026-11-15, not a substitute for the above):

| Crate | Upstream fuzzing |
| --- | --- |
| `httparse` | OSS-Fuzz project [`httparse`](https://github.com/google/oss-fuzz/tree/master/projects/httparse) (libFuzzer, ASan); in-repo targets `parse_chunk_size`, `parse_headers`, `parse_request`, `parse_response` and the `*_multspaces` variants ([fuzz/fuzz_targets](https://github.com/seanmonstar/httparse/tree/master/fuzz/fuzz_targets)) |
| `h2` | OSS-Fuzz project [`hyperium`](https://github.com/google/oss-fuzz/blob/master/projects/hyperium/build.sh) builds `fuzz_client`, `fuzz_e2e` and `fuzz_hpack` ([fuzz/fuzz_targets](https://github.com/hyperium/h2/tree/master/fuzz/fuzz_targets)) |
| `tungstenite` | OSS-Fuzz project [`tungstenite-rs`](https://github.com/google/oss-fuzz/tree/master/projects/tungstenite-rs); in-repo targets `parse_frame_header`, `read_message_client`, `read_message_server` ([fuzz/fuzz_targets](https://github.com/snapview/tungstenite-rs/tree/master/fuzz/fuzz_targets)) |

Where our code feeds attacker-controlled bytes into them (`cargo tree -i`):

- `httparse` and `h2` are reached through `hyper`/`reqwest` (`symbrowse-fetch`
  uses `hyper` HTTP/1 and `reqwest` with `http2`). Every response head, header
  block, chunk-size line and HTTP/2 frame is chosen by the remote server. No
  existing target reaches them: `http-body-decode` only sees the already
  de-framed body.
- `tungstenite` 0.30.0 (through `async-tungstenite` 0.35.0) carries the CDP,
  WebDriver-BiDi and Safari-driver sockets of the three engine crates; frame
  content is page- and browser-influenced. `chromiumoxide` additionally links
  `tungstenite` 0.28.0 for its own CDP connection; the direct target covers
  0.30.0 only, and 0.28.0 is covered by the upstream fuzzing above.

Targets (seeds in `fuzz/corpus/{httparse_response,h2_client,tungstenite_frames}`,
digests in `fuzz/corpus/hashes.json`, replays in
`crates/symbrowse-fetch/tests/parser_dependency_replay.rs` and
`crates/symbrowse-engine-chrome/tests/fuzz_seed_replay.rs`):

- `httparse-response`: `Response::parse`, `Request::parse`, `parse_headers`,
  `parse_chunk_size`; asserts totality, determinism and that no parser reports
  consuming more bytes than given.
- `h2-client`: the input is the complete server side of an in-memory duplex
  connection (closed after the last byte); one request is sent, the body is
  drained and trailers read; a 10 s deadline turns a hang into a failure.
- `tungstenite-frames`: first byte selects an established client socket
  reading frames (message/frame size capped at 64 KiB, at most 64 reads) or the
  client opening handshake reading a server response.

Accepted limits: the targets exercise the crates, not the whole
reqwest/hyper stack (connection reuse, redirects, TLS, HTTP/2 negotiation), which
stays covered by the local-server integration tests (`tests/http.rs`,
`archive_http.rs`, `recovery_http.rs`). The h2 target drives a client connection
only; server-side `h2` is not used. Handshake success cannot be reached
(the `Sec-WebSocket-Accept` value depends on a random key), so the handshake
path fuzzes response parsing and rejection, not an established session.

## Accepted RUST-015 boundary: transitive unsafe (2026-11-15)

Decision (product decision, recorded here as the RUST-015 acceptance level):
unsafe code in third-party dependencies is accepted **per category** — a
per-crate category review (above, "Transitive unsafe review") plus the
continuously enforced gates `cargo audit`, `cargo deny` (advisories, licenses,
sources, no ignores) and `cargo geiger` — and **not line by line**. Line-by-line
review of third-party `unsafe` is out of scope for RUST-015 and the port. The
item's stop rule only forbids *owned* unsafe, untriaged advisories/licenses and
unfuzzed untrusted parsers, so this boundary does not weaken it. What keeps the
acceptance honest: owned unsafe is 0 and fails CI (`rust_geiger.sh`,
`rust_hardening_inventory.py --check`, `deny(unsafe_code)` in every crate root);
any new third-party crate with unsafe, any new advisory and any license/source
change surfaces in `cargo deny`/`audit` and in the geiger package count, and
must be triaged here before it lands; dependencies that read attacker bytes are
fuzzed directly or through our targets (previous section).

## Evidence — 2026-11-15 (RUST-015 close-out)

Base `509deacd1`, macOS arm64, rustc 1.98.0 (pinned); nightly
`1.101.0-nightly (92b5b8c87 2026-10-29)` for fuzz and Miri. Tools: cargo-nextest
0.9.143, cargo-hack 0.6.45, cargo-audit 0.22.2 (1509 advisories), cargo-deny
0.20.2, cargo-geiger 0.13.0, cargo-fuzz 0.13.2. No Safari was launched or
automated. Every RUST-015 acceptance command was run from `browse/` through
`dev-external` with a short `TMPDIR`:

| Command | Result |
| --- | --- |
| `cargo fmt --all --check` | clean |
| `cargo check --workspace --all-targets --all-features --locked` | clean |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | clean |
| `cargo nextest run --workspace --all-features --locked` | 470 passed, 0 failed, 3 skipped (the `#[ignore]` child fixtures); includes the three new replays |
| `cargo test --workspace --locked` | 474 passed, 0 failed, 3 ignored |
| `cargo test --workspace --doc --all-features --locked` | clean |
| `cargo hack check --workspace --each-feature --no-dev-deps` | 10/10 packages, `Cargo.lock` unchanged afterwards (see the `--locked` note below) |
| `scripts/rust_miri.sh` | `hardening_properties` 5/5 (36 s), daemon `protocol::` 3/3 (368 s) |
| `cargo audit` / `--file fuzz/Cargo.lock` | no vulnerabilities (307 / 296 crates) |
| `cargo deny check` / fuzz manifest with `--config ../deny.toml` | advisories, bans, licenses, sources ok; duplicate-version warnings only (`bans.multiple-versions = "warn"`), no ignores |
| `scripts/rust_geiger.sh` | 239 packages scanned; owned unsafe 0; 145 third-party packages with unsafe (unchanged from 2026-11-14, same category triage) |
| `scripts/rust_hardening_inventory.py --write` then `--check` | 295 production packages, 0 owned unsafe, 10/10 crates with an `unsafe_code` policy; `security/rust-hardening-inventory.json` unchanged (the new `h2`/`httparse` edges are dev-dependencies of `symbrowse-fetch`) |
| `scripts/fuzz_smoke.sh` (`FUZZ_RUNS=4096`) | all 10 targets completed 4096 runs; seed digests unchanged before and after |
| `scripts/fuzz_corpus_hashes.py --check` | 10 targets, 125 seeds, ok |
| Longer fuzz (not a gate), 60 s each | `h2-client` 1.15M, `httparse-response` 5.56M, `tungstenite-frames` 0.99M executions, no crash |
| `actionlint .github/workflows/browse-hardening.yml` | clean |
| `python3 docs/rust-port/validate.py` | ok |

Notes:

- `cargo hack --no-dev-deps` cannot be combined with `--locked` (it rewrites the
  manifests, and dropping `symbrowse-fetch`'s new `h2`/`httparse`
  dev-dependencies changes the lock graph). The workflow therefore runs it
  without `--locked` and asserts `git diff --exit-code` on the lock and
  manifests afterwards.
- Test flakes under host load (load average about 20 from parallel workers),
  neither caused by this change: `symbrowse-engine-safari`
  `osascript_runner_kills_descendant_holding_stdout` used a 300 ms runner
  deadline and failed when the fake `osascript` helper had not yet recorded the
  descendant pid (fake script only, no Safari). The deadline is now 1500 ms with
  a 6 s elapsed bound, and the test passed 8/8 reruns. `symbrowse-mcp`
  `proxy::tests::*` (daemon-frame read `WouldBlock`) and one nextest run of
  `incompatible_owner_requires_stop_ack_but_same_owner_remains_restartable`
  failed once each in a loaded run and passed in all reruns (3/3 isolated, then
  the complete nextest and `cargo test --workspace` runs above).
- The new workflow has only been linted (`actionlint`), not run on GitHub. The
  Linux install steps (`cargo install --locked` of the pinned tools, the dated
  nightly) are covered by the first hosted run; the tool version probes were
  checked against the local binaries' `--version` output formats.

Open points closed: CI workflow (`browse-hardening.yml`), direct fuzzing of
`h2`/`httparse`/`tungstenite`, and the transitive-unsafe acceptance boundary
(sections above). Stop rule: owned unsafe 0; no untriaged advisory or license
violation; every untrusted parser in the production dependency graph that reads
attacker-controlled bytes has a fuzz target and replay.

## Evidence — 2026-11-15 (DMN-006, SEC-007)

macOS arm64, rustc 1.98.0, base `e1df254b9`. No Safari was launched or
automated; the daemon runs use the static engine only.

| Row | Command | Result |
| --- | --- | --- |
| DMN-006 | `cargo test -p symbrowse-daemon --locked --test deadlines` | 12 passed, 0 failed |
| SEC-007 | `cargo test -p symbrowse-cli --locked --test secret_redaction` | 2 passed (15-secret corpus, 16 process runs, scanner self-check) |
| SEC-007 | `cargo test -p symbrowse-core --locked --lib secret_text` | 2 passed |
| both | `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings` | clean |
| both | `cargo test --workspace --locked` | 452 passed, 0 failed, 3 ignored child fixtures |
| ledger | `python3 docs/rust-port/validate.py` | ok: 88 contracts, 17 work items |

Both rows are `parity` for darwin-arm64 and linux-amd64/arm64. The tests use
only std Unix sockets, the built binary and temp dirs; `ci.yml` `browse-native`
(ubuntu-latest) runs `cargo test --workspace` and `browse-daemon-native` runs
the daemon tests on ubuntu-24.04, ubuntu-24.04-arm and macos-15. Linux runs
were not observed locally.

**DMN-006 oracle.** The Go daemon is gone, so the contract is the Go source at
`57c26c76e` (`server.go` `serveConn`/`handleLine`, `client.go` `requestOnce`,
`TestOperationTimeoutKeepsConnectionUsable`,
`TestClientReadTimeoutOperationTimeout`) plus the sealed capture
`tests/fixtures/daemon_oracle/go_timeout.json`. Pinned behavior: the read
deadline is re-armed per frame and covers the whole frame (expiry closes the
connection silently); the operation deadline answers `operation_timeout`,
keeps the connection usable and never waits for the handler; a vanished client
is noticed only when the response write fails, and is bounded by the operation
deadline (40 abandoned operations exceed the 32 workers and the pool
recovers); stopping the server cancels cooperative handlers and does not wait
for deaf ones. Finding fixed: the Rust server had only the per-read socket
timeout, so a client trickling one byte per interval held a worker
indefinitely; the frame read now has Go's absolute deadline
(`server/connection.rs`).

**SEC-007 findings, all fixed at the source.** The first corpus run leaked in
six places: the action journal wrote URL userinfo passwords and `token=` /
`api_key=` query values; `config show` printed the CDP endpoint password;
TOML parse errors quoted the offending source lines (including secret lines
next to the error) and rejected values; CLI errors echoed argv
(`unknown command "password=..."`, URLs with passwords); MCP
`Unknown tool`/`Method not found` echoed the name; `fetch_batch` failed items
echoed the target URL. `symbrowse_core::secret_text::scrub` (URL userinfo
password plus secret-named `name=value` pairs only) now guards the journal,
`config show`, CLI stderr and the stdout of failed runs. The corpus test fails
on the unfixed tree.

Accepted limits: a successful result that echoes the URL the caller itself
supplied (`read`/`open` `final_url`, frontmatter `url`) is data and unchanged;
a secret pasted as a plain value into an unrelated flag (no `name=` shape, no
URL userinfo) cannot be recognised and is not covered; the browser engines'
own profile files were out of scope (static engine only).
