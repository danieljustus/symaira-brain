# Honest fetch controls and executable provenance (#773)

Status: accepted for the bounded static-transport slice. RUST-007 and #773 remain
in progress; daemon dependency #772, native platform evidence and complete
pipeline parity still gate closure and default cutover.

## Decision

Keep the honest Rust transport, improve its declared HTTP controls against real
Go requests, and preserve the frozen compatibility implementation. Do not infer
full transport parity from static vectors, a successful version command, or a
cross-compiled executable. The historical oracle remains
`652453d1595fc302bd69c328e7da8a21dbee28b9` / v0.8.0. New process comparisons use a
separate, complete immutable Go module at
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` with Go 1.26.7; none of its Go source or
original fixtures change.

The real process corpus checks default/overridden honest headers, five methods,
empty method, zero timeout/body-limit defaults, POST redirects (301/302/303/307/308),
redirect-count boundaries, body and gzip size boundaries, HEAD, explicit gzip and
Range behavior, header/body deadlines, seven status codes, ephemeral cookies and
private HTTP proxy peers. Successful observations compare status, final URL,
body bytes, all response headers and protocol. Errors compare the typed category;
the receipt retains each implementation's literal message without claiming
byte-identical backend diagnostics. Named cookie jars and bounded br/zstd decoding
remain explicit native target features: the Go honest client does not implement
named sessions or arbitrary Content-Encoding decoding.

Disable reqwest's implicit environment proxy selection and snapshot the Go-style
HTTP_PROXY/HTTPS_PROXY/NO_PROXY configuration once per constructed client. Uppercase
nonempty values win over lowercase, ALL_PROXY is ignored, loopback bypass is
mandatory, and CGI HTTP proxy rejection precedes NO_PROXY bypass. Domain,
subdomain, optional-port, IP and CIDR bypasses are tested against actual Go
`net/http.ProxyFromEnvironment` in fresh processes (Go caches that snapshot
process-wide). These routing observations perform no network I/O and are counted
separately from HTTP exchanges. Windows environment names are case insensitive;
the impossible uppercase/lowercase conflict case is explicitly Unix-only.
Environment inputs are valid UTF-8; raw non-UTF-8 Unix environment parity and all
malformed URL parser differences remain outside this corpus. The preserved
malformed-proxy receipt documents a Go/Rust URL-rendering mismatch rather than
silently broadening the parity claim.

Validate the selected proxy peer with the same SSRF policy as a direct target,
including literal IPs that never invoke a DNS resolver. Repeat proxy selection and
peer validation at each redirect. The shared pinned resolver is installed for the
actual proxy dial as well as direct connections. Protected SOCKS5h routes use
local pinned target DNS instead of allowing the proxy to resolve an unchecked
address. Explicit AllowPrivate permits private peers. Policy/resolver builder
changes receive a fresh transport cache so a prior clone cannot retain obsolete
redirect closures or resolver state.

Split request types, header precedence and response decoding into focused modules;
keep production modules below the repository's 400-line limit. This gives future
transport fixes a small, independently reviewable owner without a second HTTP
implementation or a CLI dependency.

## Measurement and validation

Preserve the literal original 50-pair receipt and its identity correction note.
The original harness mislabeled the Go executable with the measurement checkout's
revision; the executable has no embedded VCS revision. Do not retroactively invent
one. A fresh owned immutable Go build and a clean optimized native build each
receive a digest-bound receipt with SDK metadata and hashes of immutable source
inputs. The benchmark now records unknown executable provenance as unknown, and
the comparator rejects unknown identities, mismatched receipts and dirty sources.

The required value gate stays exactly 30 paired samples for CLI, MCP, daemon and
fetch, nearest-rank p95, at most 10% regression in each workload, and at least 20%
size or RSS improvement against the established baseline. The historical baseline
is Darwin arm64; Linux paired measurements are supplemental same-host timing
proof, not six-platform release-value certification. Keep the existing wrong-body
fetch control and original fixture/control tests. Five executable wrappers mutate
actual native observations (missing case, body, header, error class and proxy
route); each must be rejected by the new process gate.

The native workflow runs Linux, macOS and Windows on both amd64 and arm64, with
pinned SDKs, real Go process comparisons, original controls, tests and strict lint.
A separately visible, allowed-to-fail Linux paired-value job uploads raw 30-pair
samples and comparison failures. That job cannot certify default cutover; Windows
paired daemon/fetch benchmarking remains unported, and native CI receipts must be
reviewed at the published immutable head before closing #773.

Receipts: `browse/port/evidence/fetch-control-773/`. Original failed observations
are retained. Final source-bound proof is added in an evidence-only successor
commit after the code, harness and documentation commit passes validation.

## Independent review corrections

The immutable cc8 source passed its declared 154-case corpus and the Linux paired
value gate, but independent review requested changes for two production defects.
Its complete review, 70 additional actual-Go routing observations, three actual
HTTP gzip observations, binaries/digests and original 30-pair receipt are retained.
A passing bounded corpus is evidence for its cases, not permission to dismiss
newly demonstrated behavior as an unspecified parser corner case.

NO_PROXY ports compare literally with Go's canonical port string: `080` must not
be treated as `80`. A bracketed IPv6 entry without a port separator is ignored as
an IP matcher, while `[IPv6]:` matches every port. IPv4-mapped IPv6 CIDRs normalize
their network and mask together, so `::ffff:93.184.216.0/120` has the IPv4 /24
meaning used by `net.IPNet.Contains`. The permanent actual-SDK corpus includes all
70 review observations, rather than rewriting the original failures.

Use MultiGzDecoder for bounded gzip decoding. All concatenated members contribute
to the same decoded-body limit, and corrupt later members are errors. Stopping at
the first member silently truncates valid content and incorrectly accepts both
oversized responses and invalid later checksums. The private real-Go/native
corpus now covers concatenated success, four size boundaries, corrupt second CRC
and invalid trailing bytes. Explicit gzip and Range behavior remains unchanged.

The subsequent immutable a073 review found three additional differences despite
231 passing declared cases. Its original review, ten additional actual SDK
routing cases, two real owned proxy exchanges and unchanged binary digests are
preserved under `independent-review-a073`. CIDR prefix syntax must contain only
ASCII decimal digits before numeric conversion: Go rejects `/+24`, `/+32` and
mapped `/+120`, while accepting unsigned leading zeroes such as `/024`.

Go's `canonicalAddr` also retains the *target* authority port spelling. Preserve
that spelling before WHATWG URL normalization, so a written `:080` compares only
with NO_PROXY `:080`, not `:80`. Reqwest's automatic proxy/redirect callbacks see
already normalized URLs and cannot recover raw Location authorities. A focused
bounded hop module therefore resolves raw Location headers, selects and validates
the actual proxy for each hop, and obtains a cached fixed-route transport. Direct
targets and proxy peers retain the existing shared DNS pinning and private-peer
policy. Redirects retain the same ten-request bound and common operation deadline,
method/body transformations, credential stripping, Referer handling and bounded
body draining. Host and final URL retain literal ports. Real HTTP observations
cover initial, relative, absolute and network-relative authority ports, with a
separate class for redirect policy errors. This does not certify every remaining
Go/WHATWG URL representation difference or full transport cutover.

Transport identity is a hashable struct of session, resolved proxy, private-peer
permission and allowlist patterns. Formatting those fields with delimiters can
map distinct valid strings to the same cache entry: a session or proxy containing
`;proxy=` previously selected the wrong peer and attached another session's jar.
The structural key preserves reuse for identical configurations and avoids this
ambiguity. Two permanent real-Go/native proxy exchanges demonstrate both routes;
a native three-request regression additionally proves that the two named jars
stay isolated and the original jar persists when that session returns.

The expanded process gate has 251 cases on Unix and 249 on Windows: 74 real HTTP
exchanges and 177/175 routing observations that open no socket. It retains every
original case and all five executable mutation controls. The ten review routes
are accompanied by actual initial/redirect port and method/body/header cases;
counts distinguish network exchanges from SDK-only routing. Fresh exact-source
parent tests, strict lint and optimized 30-pair measurements remain required
after this correction. Native six-target acceptance, complete pipeline parity,
Windows paired measurements and #772 remain open.

## Proxy absolute-form boundary and E011

The independent immutable522/1f82 review found that correct NO_PROXY selection,
Host and final URL did not preserve the absolute-form URI actually sent to an
HTTP proxy. Reqwest constructs that URI from a normalized URL; default ports and
leading zeroes disappear. Retain the complete original review, declared Browse-CWD
runner failure, five real proxy pairs (four failures), eight real Referer pairs
and executable snapshots under `independent-review-522`. Anchor the archive to
the Git repository root so the actual Browse-CWD CI invocation can run its gates.

Use a limited Hyper HTTP/1 send path only for an HTTP target over an HTTP(S)
proxy when its explicit port would otherwise change. This preserves the raw
absolute-form URI without a dependency fork/private override or a global client
replacement. The selected route is still policy-checked at every hop; protected
proxy dials use the existing pinned resolver and selected peer port, and HTTPS
proxies use the same native TLS backend, default certificate trust and hostname
verification. URL userinfo is excluded from the request target and uses the
existing request builder's authentication encoding. Go writes transport proxy
credentials after any caller Proxy-Authorization values; one shared hop helper
preserves those values/order for both raw and ordinary HTTP-proxy sends.

The existing typed client cache and named jar map remain authoritative. The raw
path uses the same named jar, including transitions to ordinary/direct requests;
unnamed requests acquire no persistent jar. Its connection is deliberately owned
by one response rather than pooled: cancelling, timing out, exceeding limits or
dropping that body aborts the driver and closes the socket. A small Body adapter
forwards the original length hints and trailers; reducing it to an opaque byte
stream would lose the early compressed-size check. The existing shared operation
deadline, retry policy, redirect processing and bounded response decoder apply.
No global pooling/performance claim follows from this selected path. Measure
current executables and retain its connection/HTTPS-root costs as explicit scope;
full773/native six-target/value acceptance still remain pending.

Five explicit direct dependencies (`http`, `hyper`, `hyper-util`, `native-tls`,
`tokio-native-tls`) expose versions already in the immutable lockfile and used by
reqwest. They justify this supported URI/IO/body boundary rather than inventing
HTTP parsing or copying a TLS stack. No frozen Go source or original fixture,
equality assertion, benchmark threshold or sample selection changes.

The delegated [E011 decision](https://github.com/danieljustus/symaira-brain/blob/6eca544a8db11c4aeba6d2a80b3a5680c34bc9ab/docs/adr/0003-issue-resolution-decisions.md#e011--strip-fragments-from-automatic-redirect-referer-headers)
specifically preserves removal of fragments from automatically derived redirect
Referer (RFC9110 section10.1.3). Frozen Go1.26.7 discloses the fragment; native
does not. An explicit caller-supplied Referer remains unchanged and byte-compatible.
The separate real process gate asserts the exact Go/native automatic difference,
seven ordinary equality cases and rejects mutants that disclose the automatic
fragment or strip an explicit fragment. This exception covers no other header,
request URI, proxy selection or authentication difference and never turns a
failed ordinary equality case into an accepted one.

The additional owned proxy gate covers raw request targets, methods, raw/ordinary
redirects, origin/proxy authentication, private-peer rejection, body/gzip limits
and deadlines. Native named-jar behavior is separately asserted, since the Go
honest client has no named jar. Actual HTTPS-proxy tests reject untrusted chains
and hostname mismatches; a disposable CA additionally proves trusted success on
Linux through per-process SSL_CERT_FILE. No operator trust-store write occurs.
The macOS/Windows root backends require their own owned trusted-CA proof and are
explicitly pending; cross-compilation or two matching TLS failures do not certify
a successful native trusted TLS connection there.

The root's partial static review of source38 also identified the inherited
Windows runner-path boundary: Git Bash receives RUNNER_TEMP with native
backslashes, while Bash mktemp and tar need an MSYS path. Normalize the selected
owned temporary root with cygpath before either command; convert executable and
Go-source paths back only at the native-process boundary. Preserve source38's
clean Linux proof unchanged. This focused runner correction requires a fresh
clean source replay and remains subject to actual Windows CI; a Linux replay
and static comparison with the accepted Guard runner are no Windows runtime proof.

Clean source e2's coordinated full30×4 release measurement passes the unchanged
value gates (Fetch p95 -59.04%, comparator size reduction41.59% against its
existing recorded Darwin-arm64 release baseline). The current Linux executables
are10,707,264 bytes native and26,611,127 bytes Go; the unchanged comparator's
size reference is explicitly different from that fresh Go build. A separate owned
local HTTP/HTTPS proxy probe preserves all120 exact Go/native response pairs
and every observed response interval. For30 sequential calls, ordinary proxy
routes use one connection in both clients; the normalization-sensitive explicit
port route uses30 native connections versus one Go connection. This includes
ordinary explicitly written `:80`; there is no observed frequency basis for
calling the route rare. Raw HTTP later-interval medians were0.633ms native versus
0.174ms Go, and raw HTTPS2.241ms versus0.241ms. These debug-seam, local-fixture
intervals include startup in the first observation and are not isolated Fetch
timings or a new acceptance threshold. Connection reuse/TLS cost remains an open
#773 optimization, alongside full pipeline/native-six-target cutover gates.
Keep the complete current measurement, the prior loaded failure and the new
pre-sample SDK-PATH bootstrap failure in `proxy-uri-e2`; no sample was discarded.

## Proxy authentication octets (#773 focused correction)

The immutable335/e2 independent full-layer review found one new P2: deriving
proxy Basic authentication through a fictitious reqwest origin request drops
a percent-escaped non-UTF8 password, and the raw path also drops a non-UTF8
username. An enforcing owned proxy returns Go200/native407 for all four byte
cases across both raw080 and ordinary81 routes; ASCII and valid UTF8 controls
return200 in both. Preserve the complete review, all38 capture cases, eight
enforcing cases and archived522 parent classification under
`independent-review-335`. Inherited origin-userinfo, H2 and trailer/framing gaps
are recorded there and are not included in this correction.

Keep a selected proxy's policy/dial URL separate from its original encoded
userinfo. WHATWG URL normalization discards empty userinfo and cannot represent
all Go percent-decoded octets. Split the retained userinfo at its first literal
colon, percent-decode each part directly to bytes, and append Go's Basic value
after caller Proxy-Authorization values in the shared per-hop helper. Apply
that helper to both HTTP-target routes, including HTTP and HTTPS proxy peers,
without changing origin auth, HTTPS-target CONNECT/SOCKS auth, DNS protection,
redirect bounds, deadline, cookie ownership, response decoding or transport cache.
Empty `@`/`:@` proxy userinfo also emits Basic Og== through this same retained
presence operation; its historical omission is distinguished from the new P2.

Two explicit supported codec dependencies, base64=0.22.1 and
percent-encoding=2.3.2, already exist at those exact versions/checksums in the
lockfile through reqwest/url. They avoid a custom credential codec without a
new package version or source. The process-only route example reads the selected
URL for its unchanged display contract; it never prints the retained credential
metadata. Synthetic proxy credentials in the owned gate are not operator secrets.

`fetch_control_proxy_auth.py` adds actual ordered-header and enforcing-proxy
observations for both target ports, caller append order, empty/ASCII/UTF8/raw-byte
userinfo, uppercase/lowercase and schemeless environment proxies, redirects, and
owned trusted HTTPS proxy peers. Existing251/249 comparisons and all existing
controls remain unchanged. Three additional real native-output mutants must
fail for truncated credentials, reversed caller order and proxy authentication
rejection. Linux-owned CA proof does not certify macOS/Windows trust stores; all
six native targets still require exact published-head CI. Four non-UTF8 origin
URL cases remain mismatched in the unchanged38-case supplemental probe. Full
#773, default cutover, unpooled normalization-sensitive route optimization and
all previously stated native/value gates remain open. Fresh current-source
release measurements are required after this production change; prior e2's
240/120 samples remain historical proof and are never relabeled as current.

Clean corrected source c7beb3's Linux author proof now passes114 ordered proxy
header exchanges, eight actual enforcing-peer exchanges and three new actual
mutation controls. The original38 supplemental inputs yield34 equal pairs and
four expressly retained inherited origin-auth differences. Fresh123 tests
(0 failures, one parent-owned ignore,26 summaries/seven asserted child cases),
strict lint, all251 old process cases/five controls, raw32/E0117+1/TLS4/jar4
and three old supplementary controls pass. Immutable34/16/152/415 source/harness/
release-Go manifests are retained under `proxy-auth-c7`; frozen Go and fixtures
remain unchanged. These are author observations awaiting independent review.

The coordinated single current30×4 window retains all240 samples and passes
the unchanged comparator: Fetch p95 -55.17%, all four workloads below+10%,
size reduction41.56% against the existing Darwin-arm64 baseline18,330,466B.
Current Linux releases are10,711,464B Rust and26,611,127B Go. All120 separate
owned HTTP/HTTPS wire pairs agree. Ordinary routes use one connection per30
requests in both clients; the explicit normalization-sensitive raw route still
uses30 native versus one Go connection. Later-response local medians on that
route are0.706ms/0.229ms native/Go HTTP and2.535ms/0.214ms HTTPS. Keep startup
observations and every raw interval, not just these summaries. This is a debug
seam local-fixture cost, not isolated Fetch latency, a frequency claim or another
acceptance threshold. The original loaded failure and all prior240/120 proof
remain historical; no sample was discarded. Complete#773/native6/default
cutover, inherited parity gaps and connection reuse optimization remain open.
