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
