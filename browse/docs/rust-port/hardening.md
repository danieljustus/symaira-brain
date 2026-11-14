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

Run the bounded smoke with `scripts/fuzz_smoke.sh`. It copies each tracked seed
corpus to a temporary directory, runs the configured number of executions
(`FUZZ_RUNS`, default 64, maximum 4096), requires libFuzzer to report a
completed bounded run for every target, and checks the tracked seed digests
before and after. Do not pass `fuzz/corpus/*` directly to libFuzzer: libFuzzer
may write new interesting inputs there.

`scripts/fuzz_corpus_hashes.py --check` verifies `fuzz/corpus/hashes.json`.
Update the manifest (`--write`) only when deliberately changing a seed, then
review the seed and target together.

Every target also has a stable-Rust replay (`crates/*/tests/fuzz_seed_replay.rs`,
`crates/symbrowse-fetch/tests/dom_input_smoke.rs`) that runs the seeds, all of
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
the fuzz targets above; `h2`, `httparse` and `tungstenite` are reached only
through live HTTP/WebSocket traffic and are not fuzzed here. cargo-geiger could
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
- **Reviewed, not changed.** A first `state-headers` fuzz run found that
  `validate_name` accepts C1 control characters (U+0080–U+009F) while rejecting
  ASCII controls and `/`. They are not path syntax and no Go-recorded fixture
  pins either behavior, so the contract is left unchanged and the fuzz/replay
  assertion states the actual contract (single path component, no ASCII
  control). Revisit if names are ever echoed to a terminal unescaped.

## CI policy

`rust-ci.yml` and `rust-hardening.yml`, named by the finalization plan, do not
exist in this tree; only the native browse workflows and the root `ci.yml` jobs
do. Wiring the commands above into a `main`/schedule workflow is still open.
Until then this document is the only record of the hardening run; heavy tooling
(Miri, geiger, fuzz smoke) should stay out of required PR checks.

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

Not covered here: the Safari native gate (Remote Automation on the user's real
Safari, a maintainer step) and therefore RUST-013; Chrome/Firefox native
receipts are as recorded in the ledger. Miri was not rerun after the
fetch-only change because no Miri target depends on it.
