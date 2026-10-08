# Independent full-layer review: Fetch #773 second correction

**Disposition: REQUEST CHANGES.** The three a073 findings are closed for their demonstrated cases, but the declared native CI runner fails before running the process oracle and valid HTTP proxy request targets still lose literal authority ports. The independently demonstrated automatic Referer fragment difference is a specifically authorized privacy decision, not a production fix request; the candidate still needs truthful public documentation and a separate executable accepted-divergence proof. No candidate files or GitHub state were modified.

## Immutable artifact and independence

Reviewed clean publication HEAD `52236234a197f00de89edbb185fc762a4f121161` in `/workspace/symaira-fetch773`, source `1f82e9cf8812c5b383a19349241787215a857493`, normal main ancestor `31de72294521701fc4b1a0bce39f03cc34d72e7e`. Frozen full Go oracle: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, actual Go 1.26.7. Source-to-publication delta is exactly 14 evidence files. Source and worktree were clean before and after review; all 415 candidate Go inputs equal the frozen bytes.

This reviewer independently executed the declared runner, an explicitly external one-line CWD-corrected runner, the actual Go/native corpus and controls, additional real HTTP/proxy comparisons, affected parent Rust tests, strict lint and original Go gates. The author’s measurements were independently hash-verified and their complete unchanged comparisons recomputed; no new timing measurement is claimed. The existing candidate-owned target was exclusively reused with debug=0, incremental=0, umask022 and the subreaper lifecycle wrapper. Owned HTTP/proxy fixtures, disposable HOME/XDG roots, an absent Go fallback and no operator credentials were used. No host browser or provider was contacted.

## Findings

### P2 — run the frozen-Go archive from the repository root

`browse/port/harness/run_fetch_control_773.sh:8`; `.github/workflows/browse-fetch-native.yml:27,51`.

The workflow explicitly runs in `browse`. From that directory `git archive <frozen> browse` applies the current Git prefix and searches for `browse/browse`. The actual unchanged declared invocation exited 2 with `fatal: pathspec 'browse' did not match any files`, followed by tar errors. No oracle or mutation control ran in that invocation. This affects the declared native process-comparison job rather than only the review environment; actionlint cannot detect it.

Use the repository root explicitly for the archive, preserving the frozen revision and remaining relative runner paths, then execute the unmodified corrected runner from the exact workflow CWD. Preserve the failure. This review’s outside-candidate runner changes only that archive command to `git -C "$(git rev-parse --show-toplevel)" archive ... browse`; it passed 251 comparisons and five controls. That passing fallback does not approve the candidate’s still-broken declared runner.

Evidence: `/tmp/symaira-fetch773-second-fix-independent-process.log` (original failure), `/tmp/symaira-fetch773-second-fix-independent-cwd-run.sh`, `/tmp/symaira-fetch773-second-fix-independent-cwd-process.json`, `...-process-controls.json`, `...-process.log`. Original runner SHA256 `df7f86906d56f5e1baecb341bdf5276883581826fab4ee99c5d75413a3136c04`; external corrected runner `9a24a66a3a286d12ca4fa36ac6d97e3652e918091e0c7e20181078bfdcdf1a02`.

### P2 — preserve the written authority in HTTP proxy absolute-form request targets

`browse/crates/symbrowse-fetch/src/honest/redirect.rs:139,144,163`.

The module reconstructs `current` and overrides Host with the raw port, but then gives reqwest the parsed WHATWG URL. An HTTP proxy receives the normalized absolute-form request target. Five real paired Go/native requests to an owned proxy produced four differences:

| Target authority port | Go request target | Native request target | Host |
|---|---|---|---|
| `80` | `http://93.184.216.34:80/path` | `http://93.184.216.34/path` | both retain `:80` |
| `080` | `http://93.184.216.34:080/path` | `http://93.184.216.34/path` | both retain `:080` |
| `00080` | `http://93.184.216.34:00080/path` | `http://93.184.216.34/path` | both retain `:00080` |
| `81` | `http://93.184.216.34:81/path` | identical | both retain `:81` |
| `00081` | `http://93.184.216.34:00081/path` | `http://93.184.216.34:81/path` | both retain `:00081` |

These are valid HTTP URLs and genuine proxy exchanges, not a malformed-parser case. A proxy handling, signing, logging or returning the absolute URI sees different bytes; our echo proxy returns different successful body bytes/Content-Length. The declared authority tests only echo Host/Referer and therefore pass without comparing this request target. Literal NO_PROXY selection and returned final URL are repaired, but that does not establish full literal-port behavior at the proxy transport boundary.

Repair the HTTP proxy request-target boundary without losing TLS/SSRF/DNS pinning, per-hop route selection, limits or cookie isolation; append real initial/redirect absolute-form observations and retain the original failures. Do not assert complete repair based only on Host or route-only observations.

Evidence: `/tmp/symaira-fetch773-second-fix-independent-extra-proxy.py`, `.json`, `.log`. Result: 1/5 exact, 4/5 demonstrated differences. Both process binaries are the same source-bound binaries used by the fresh corpus.

## Specifically authorized Referer divergence

`honest/redirect.rs:112` strips the fragment from an automatically derived redirect Referer. Actual Go 1.26.7 `net/http/client.go` retains it. Of eight additional genuine HTTP pairs, seven match exactly (explicit fragment-bearing Referer, same/cross-host URL userinfo, direct/redirect Host header, HEAD and lowercase custom method), while an initial `#private-fragment` produces Go Referer ending in that fragment and native Referer without it. Both reach the same final URL/status; echoed header/body lengths differ.

Parent maintainer explicitly decided during this review to preserve native removal: RFC9110 section10.1.3 forbids fragments/userinfo in generated Referer, and copying the Go behavior could disclose client fragments. This is an authorized behavior decision rather than permission to ignore other differences. The immutable candidate predates that decision and currently claims preserved Referer handling without documenting it. Add public ADR/matrix/scope text and separately assert the exact actual Go leakage/native sanitization bytes. Keep all existing equality cases/assertions and original frozen Go production/fixtures unchanged. Explicit caller-supplied Referer remains untouched in this candidate; that case is separately proven equal and must not be described as already sanitized.

Evidence: `/tmp/symaira-fetch773-second-fix-independent-extra-http.py`, `.json`, `.log`; 8 inputs, 7 equality observations and one authorized automatic-Referer divergence. This exception covers no raw-port/proxy or other behavior.

## Full layers inspected and finding closure

Read production `client.rs`, `types.rs`, `honest.rs` and all four `honest/{headers,proxy,redirect,response}.rs` modules, `lib.rs` split/reexports, both changed HTTP/cache tests and process example. Inspected all a073→1f82 production/test/harness/documentation changes, original cc8/a073 full independent reports/repros, complete source/provenance/CI delta and all 14 publication-only files. Verified previously reviewed inherited daemon server/path modules remain byte-identical to a073; its independently reviewed lifecycle boundaries and current parent tests were reused. Also inspected the CLI cache-directory split/help test cleanup and MCP owned fixture/session seam; no production authority widening was introduced there.

The typed TransportKey separates session/proxy/private permission/allowlist; fixed-route clients still share the correct jar only for a named session. The actual permanent collision pair reaches two distinct owned proxies; the native three-request test proves jar isolation and return persistence. Signed CIDR prefixes now require unsigned ASCII decimal syntax; actual Go route cases reject `+24/+32/+120` and preserve leading-zero unsigned forms. Raw target ports survive NO_PROXY selection, Host, final URL and relative/absolute/network-relative redirects. The additional proxy finding above describes the remaining boundary, not a reopening of the now-passing earlier NO_PROXY cases.

Inspected shared policy/pinned resolver used for target and actual proxy dial; selected proxy IPs are checked before connection, redirects repeat allowlist/private/proxy checks, protected SOCKS5h uses local pinned target resolution, builder policy/resolver changes reset the transport cache. Explicit private opt-in remains visible. The new module enforces ten total requests, caller’s common deadline across hops/retries/read, bounded redirect draining, sticky sensitive-header removal and Go1.26.7 method/body transformation. Actual POST302/307, credential and raw authority cases pass. Manual SDK inspection confirmed Go’s sticky body drop (a 302→307 hypothesis is not a finding). Response decoding uses bounded multi-member gzip and shared decoded limit, with explicit gzip/Range metadata behavior; named sessions and br/zstd are declared native extensions. No new dependency, module authority or Go escape hatch was added. Touched production modules are below400 lines.

Read Go supplement and native seam, Python corpus/followups/mutations, exact environment construction, SDK proxy source, source byte verification and comparisons. Errors compare typed class and retain literal diagnostics; successes compare status/final URL/body/all response headers/protocol. Routing-only cases open no socket and are counted separately. Inspecting intended mutant diagnostics confirms rejection came from missing IDs or changed body/header/error/route, not from runner infrastructure. Native six-target matrix, SDK pins, workflow permissions/uploads, separate allowed-to-fail Linux value job and all planning/ADR/evidence scope limits were reviewed. Static transport is still partial; no full773 closure/default cutover is supported.

## Independently executed verification

- Original declared process runner: **FAIL exit2** before Go archive. External one-line CWD correction: **251/251** (74 actual HTTP,177 routing-only); **5/5 actual executable mutants rejected**, current clean publication HEAD reported.
- Extra HTTP: **7/8 equality**, one explicitly authorized automatic Referer divergence. Extra proxy: **1/5 equality**, four actual request-target failures retained.
- `cargo test --locked -p symbrowse-fetch -p symbrowse-daemon -p symbrowse-cli --all-targets`: **119 passed,0 failed,1 ignored across25 summaries**, no compilation. The passing isolated-environment parent executes its ignored child in all seven environments and asserts each child’s exit0/`1 passed`; therefore the child is not an unexecuted acceptance hole. Fetch51/0/0.
- Affected three-package all-target/all-feature strict Clippy, fmt, actionlint for both Browse workflows, matrix **88 contracts/17 items**, and **4 identity tests** pass.
- Actual pinned Go1.26.7 original fixtures **28 static/11 profiles/15 robots**, uncached **10 original fetch-package groups** and legacy benchmark tests pass; **2 manual macOS NVMe tests** remain explicit skips. Author also retained separate Go1.26.6 fixture checks, not independently re-executed here.
- Independently checked **30 fetch**,**10 harness/workflow**,**148 native release**,**415 Go** input hashes against immutable Git objects and candidate files as applicable; fresh oracle manifests agree. Verified all415 current Go bytes equal frozen oracle,12 evidence digests,14 embedded raw-log digests,14 evidence-only publication files and byte-preserved original a073 review/receipt.
- Digest-checked actual native probe `5469c1e7b2ea7f1d3680aa40829bce7a5b109e371c58504e8ad0b03c53697cf6`, fresh Go probe `8e92be70d9b908c11f47720b1bf34c5717b327c246686dbb71e03b336f820786`, native release `6384a60885922ed76902432712fd4ff2a82f63b0430cd8b06f0defeaa1834853`, Go release `221b047aa18e2c4eb421dd970de18e6e82372059c492f6bd52ac337c95c5a5f6`.
- Independently recomputed both complete **240-sample** comparisons and nearest-rank p95 from every raw sample; immutable release digests agree. Quiet full30×4 **PASS**, loaded full30×4 **BLOCKED** (MCP+51.81%,daemon+145.55%). No threshold edits, discarded samples or rewriting of the failed receipt. Quiet Go/Rust p95ms: CLI7.741956/4.328724,MCP8.320250/4.206590,daemon20.641629/20.406333,fetch2.617087/0.997628. Native10,598,888 bytes versus same-host Go26,611,127 (60.171% improvement); historical Darwin baseline42.179% is explicitly a different platform. These are verified author/root measurements, not new reviewer timings.

Machine receipt, manifests, runtime hashes and all review-artifact digests: `/tmp/symaira-fetch773-second-fix-independent-review-receipt.json`; detailed independent verification `/tmp/symaira-fetch773-second-fix-independent-provenance.json`. Logs retain original declared failure, external workaround, source-bound process successes, additional failures and strict/original checks. No target process remained active when released; target/source are available to the assigned author.

## Remaining acceptance

Correct the declared runner and HTTP proxy absolute-form authority handling; document/test the specifically authorized automatic Referer divergence without weakening ordinary assertions. Independently review the resulting clean full correction and regenerate source-bound evidence. Fresh native six-target CI at the final published immutable head, Windows paired measurements, complete remaining static/pipeline controls and #772 acceptance still gate #773 closure/default cutover. Current Linux subset and quiet supplemental value receipt do not certify those missing gates.
