# Independent Fetch773 proxy URI review — REQUEST CHANGES

Reviewed clean final **335073aab4bc49b6a4497b38115ed5d8a688374a**, validated source **e2ecf4026806dd4d9fd219f15f931848b1fd56d9**, transport implementation **38e11d8560dc515811e92fddde1d16c7c96c2055**, at `/workspace/symaira-fetch773-proxy-uri`. Rejected parent522/source1f82 remains immutable: **52236234a197f00de89edbb185fc762a4f121161** / **1f82e9cf8812c5b383a19349241787215a857493**. Actual main31 **31de72294521701fc4b1a0bce39f03cc34d72e7e** is an ancestor. Frozen Go **dcddcef0df5789123c7c9a7ebe6e01f10e941f2c**, SDK **1.26.7**. No source edits, GitHub writes or integration of newer main during review.

Reviewer authored historical522 corrections, **did not author this new proxy transport/runner patch**. This independently reviews the successor and its complete reachable HTTP transport interactions; historical parent's original independent review and all failures are retained rather than self-approved. One actionable P2; no additional new finding after source review and actual probes. Native-platform gates and complete#773 cutover remain open.

## P2 — Preserve proxy credential octets when deriving transport authentication

Location: `browse/crates/symbrowse-fetch/src/honest/proxy_uri.rs:31–36`, invoked for both raw and ordinary HTTP proxy sends by `honest/redirect.rs`.

`authorize()` builds a fictitious origin request to reuse reqwest's URL-userinfo decoding. That decoding requires a UTF-8 username and treats a non-UTF-8 password as absent. Go `url.Parse` preserves the percent-decoded credential bytes and `Transport` Base64-encodes those bytes. Thus a valid accepted proxy URI `http://user:%ff@127.0.0.1:<owned-port>` produces **`Basic dXNlcjr/`** in Go but **`Basic dXNlcjo=`** in the candidate: the password silently becomes empty. It occurs on both a raw `http://93.184.216.34:080/...` target and an ordinary `:81` target. For `%ff:password@`, the new raw route sends no derived Proxy-Authorization. Caller-header cases also retain a wrong appended credential rather than the Go byte sequence.

Actual source-bound proof is `/tmp/symaira-fetch773-proxy-uri-independent-auth-extra.json` (38 process pairs,18 equality/20 differences; full synthetic credentials and raw response body bytes retained) and `...-independent-auth-required.json` (eight real authentication-enforcing proxy exchanges). All four non-UTF-8 credential cases return Go **200** versus native **407**; four ASCII/valid-Unicode controls return **200/200**. No external proxy or operator credential was contacted.

Actual archived522-parent execution distinguishes introduced behavior from existing deficits: the password previously became U+FFFD via reqwest proxy decoding, while the new common helper truncates it to empty; the raw path now drops a non-UTF-8 username's auth instead of the previous replacement-byte auth. The old implementation was already not Go-equal for these bytes; this new code takes over that credential derivation and adds a further lossy behavior. Empty `@`/`:@` proxy userinfo and non-UTF-8 **origin** credentials are separately retained inherited gaps, not additional new findings. The current equality corpus only exercises valid UTF-8 credentials and does not detect this condition.

Derive Basic transport auth from the selected proxy's original userinfo with Go-compatible percent-decoded **bytes**, preserving userinfo presence, the first literal separator, empty username/password and caller-first/derived-last wire order. Do not silently substitute, drop or truncate credential bytes. Retain the actual failures; extend the bounded Go/native corpus and intended negative control. Do not rewrite frozen Go or relax existing assertions.

## Original rejected findings closed with exact inputs

- Original Browse-CWD runner failure is closed: independently invoked the **declared tracked runner from `browse`**, which archives with Git explicitly rooted at repository top. Full runner passed. Static Windows temporary-root normalization occurs before Bash mktemp/tar; this is a source-based correction, not an actual Windows runtime result here.
- Exact original five proxy `/path` port spellings **80/080/00080/81/00081** now **5/5** byte-equal, independently executed unchanged inputs (`...-independent-original5.py/.json/.log`).
- Original eight HTTP cases remain **7/8** equality. The sole automatic Referer fragment divergence is separately asserted as authorized **E011**, published decision6eca544a8db11c4aeba6d2a80b3a5680c34bc9ab. Explicit caller Referer fragments remain equal. The existing equality corpus was not rewritten; three actual-output new controls reject target normalization, automatic fragment leakage and explicit fragment stripping.

## Full source and ownership review

Read applicable root AGENTS and all successor changed production, dependency edges, tests, runner/harness, workflow and ADR/matrix changes. Production: `honest.rs`, `honest/{proxy_uri,proxy_io,proxy_body,redirect}.rs`, `types.rs`; reachable existing `client.rs`, `honest/{headers,proxy,response}.rs`, Request/options, typed cache/session selection, shared pinned DNS, body decoder/error mapping and retry/deadline ownership. New tests `tests/proxy_uri.rs` and protected proxy resolver unit test. Historical source/fixtures remain manifest-identical to their retained independently reviewed parents; no claim to have independently authored or approved those parents.

Reviewed new fixture/transport proof in `fetch_control_{proxy_fixture,raw_proxy,raw_proxy_negative,tls_fixture}.py`, original process/controls/followups, declared `run_fetch_control_773.sh`, native workflow, precise Go oracle production and SDK proxy-auth/HTTP2 source, ADR, matrix, build/source manifests, preservation/archive receipts and quiet/cost drivers. The five new explicit dependencies already existed at those exact locked package versions; every lockfile package version/source/checksum is unchanged. Small three-module split respects the file-size guideline and isolates socket, response lifetime and request construction.

- Selection limits the manual Hyper HTTP/1 path to HTTP targets over HTTP(S) proxies when an explicit literal target port would normalize, including normal written`:80`. Direct, SOCKS and HTTPS targets retain reqwest. No unsupported frequency claim.
- Policy and redirect checks happen before actual peer access. Protected raw dials reuse the client's shared approved-address pinned resolver; private results are rejected before any accepted connection. TLS uses native default chain/hostname verification; no certificate bypass or operator trust modification. Owned Linux CA tests establish trusted success, untrusted and wrong-host rejection.
- Shared operation deadline/retry/body limits remain above the manual send. Response owns the connection task; drop/timeout/limit/cancellation abort it. Actual tests prove socket closure while client stays alive. Body adapter forwards original size hints and trailer frames.
- Common redirect method/body/header handling retains literal ports, sticky sensitive-header stripping, origin userinfo precedence, hop limits and bounded draining. Transport-derived proxy auth is re-created per HTTP hop rather than persisted into HTTPS origin headers. Named jars share existing session identity; ephemeral requests do not acquire persistent cookie storage.
- The new route deliberately lacks pooling. That cost is measured and explicitly left open; ordinary cache/client pooling is not claimed to apply to it.

## Independent executed validation

All commands used owned existing target, subreaper, umask022 and pinned SDK environment. Sources stayed clean.

| Verification | Independent result |
| --- | --- |
| Tracked declared process runner from actual Browse CWD |251/251, five intended original mutations rejected|
| Supplementary raw proxy corpus |32 exact pairs|
| Referer boundary |seven exact pairs plus one separately exact E011 divergence|
| Owned HTTPS proxy Linux proof |four exact pairs|
| Native named jar integration |four assertions|
| New actual-output controls |three intended mutations rejected|
| Ordinary CLI/daemon/Fetcher tests |123 passed,0 failed,one parent-owned ignored entry;26 summaries|
| Isolated child path probes |seven actual children, each parent asserts success and one test executed|
| Fetch tests within above |55 including shared resolver and live cancellation/deadline/limit cleanup|
| Strict all-target/all-feature Clippy, fmt, actionlint |pass|
| Frozen fixture generation |28 vectors;11 profiles+15 robots checked unchanged|
| Actual SDK Go `go test -count=1 ./internal/fetch/...` |all10 packages pass|
| Contract matrix validation |88 contracts,17 acyclic work items|
| Benchmark-harness unit controls |seven pass,two platform skips explicitly retained|
| Binary identity controls |four pass|

Literal fresh logs: `/tmp/symaira-fetch773-proxy-uri-independent-{process,tests,clippy,fmt,actionlint,go-fixtures,benchmark-tests,identity,matrix}.log`; process JSON/control reports use the same prefix. An initial external matrix command used a nonexistent checker filename and returned2 after the other checks passed; the actual matrix validator `docs/rust-port/validate.py` was then executed successfully. This is not concealed as a product or gate failure.

Meaningful further actual-peer probes:
- **38 auth** pairs plus **eight enforcing** pairs: P2 above; parent comparison preserved.
- **18 body/framing** pairs:14 equality; four differences (chunked/trailer response header presentation and duplicate identical Content-Length presentation) are **also identical in the actual archived522 parent**, so retained as inherited full-parity limits. Short bodies/chunks, conflicting lengths, no-content and informational final response behavior agree. The first malformed-peer wrapper stopped on real Go idle-channel diagnostic stderr; that first runner/log is retained. Follow-up captures exact stdout/stderr/exit instead of discarding diagnostics.
- **Three owned h2-advertising HTTPS proxy** pairs: actual Go1.26.7 returns `http2: unencrypted HTTP/2 not enabled`; native raw080/:80 succeeds HTTP/1.1, ordinary81 succeeds HTTP/2.0. This is an existing Go/native HTTP2-proxy gap plus the explicitly scoped new HTTP/1 route; it is **not** represented as three parity passes or a new confirmed regression. TLS peer source, binary/source hashes, exact outcomes and per-process disposable CA policy retained.

## Provenance and measurement independently verified

`...-independent-provenance.json`:34 Fetch source files,14 harness/workflow files,152 Rust release inputs,415 frozen Go inputs all match immutable Git objects and current bytes. Four current executable digests verified;53 copied originals and55 handoff-retained evidence files match their original hashes; every byte/digest of all42 archived522 native executables reverified. Original parent probe was extracted only into reviewer-owned `/tmp`. All frozen Go/fixtures unchanged. All56 e2→335 changed files are evidence/docs only.

Current debug probe **ef2dc9ac7d95e7be58499cf70acf9e8e64cdee0ddc13adb6682a741b938a0920**; real Go process probe **8e92be70d9b908c11f47720b1bf34c5717b327c246686dbb71e03b336f820786**. Native release **1900720a4c0b518f80747f7cfc8202b96b1778124072a6eafc138d6c67c4042f**; clean frozen Go release **221b047aa18e2c4eb421dd970de18e6e82372059c492f6bd52ac337c95c5a5f6**. Fresh gate's rebuilt frozen Go probe matches original Go process digest. Source-bound successful build logs/SDK metadata are retained.

Independently recomputed **all240** preserved30×4 raw observations, nearest-rank p95 and integer median representation, all pass statuses and both Fetch negative controls; binary/build identities and load-receipt report/comparator digests match. Invoked **unchanged comparator** independently: **PASS**, Fetch p95 **−59.04%**, all four workloads≤10% regression. No new benchmark window, sample exclusions or thresholds. Existing41.59% size comparison references historical Darwin-arm6418,330,466B; separately verified current Linux native10,707,264B / Go26,611,127B.17 quarter-second load observations are retained; they document the coordinated run, not six-platform acceptance.

All **120** separately measured local-proxy wire pairs recomputed equal. Each ordinary route uses1 accepted connection per30 calls in both clients; raw native uses30 versus Go1. Later-response medians: rawHTTP native0.633ms/Go0.174ms; rawHTTPS native2.241ms/Go0.241ms. This debug-seam process-visible interarrival measure excludes startup only in the explicitly named later-response median, retains the startup observation, and makes no global workload claim. Additional unchanged ordinary-route measurements retained. `...-independent-value-check.json` contains exact recomputation.

Disposition: **REQUEST CHANGES for one P2**; original two522 findings resolved. Preserve immutable335/e2, original raw credentials/failure boundaries and binaries before correction. Candidate/target are released after final hash/clean/user scan. Native six-target CI, Windows value, trusted-CA success on macOS/Windows, unpooled transport cost, inherited remaining HTTP parity, complete#773 and dependency#772 remain open. This Linux review grants no cutover or foreign-platform acceptance.
