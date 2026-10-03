# Independent full-layer review: corrected Fetch #773

Clean immutable candidate a0732ec57357fd0e1a60dbf1ca5f8c42d8c59c41;
reviewed source 8973726d531a946a6e4a041f0e590b9a16407bbe, with accepted main
31de72294521701fc4b1a0bce39f03cc34d72e7e integrated normally.
Verdict: REQUEST CHANGES. Three confirmed P2 findings below.

## P2: reject signed CIDR prefix grammar

browse/crates/symbrowse-fetch/src/honest/proxy.rs:121 accepts prefix.parse::<u32>(),
which accepts a leading plus. Actual Go net.ParseCIDR rejects that syntax, so
NO_PROXY=93.184.216.0/+24 with http://93.184.216.34 retains the proxy in Go but
bypasses it in Rust. Independent actual-process probes reproduce the same
failure for IPv6 /+32 and mapped IPv6 /+120. Ordinary /24 and /024 controls
match. Require the Go decimal-prefix lexical grammar before conversion; add
real SDK subprocess cases. These are config matcher differences with valid
HTTP target URLs, separate from the documented malformed URL rendering limits.
Literal outputs: /tmp/symaira-fetch773-fixed-independent-extra-routing.json.

## P2: preserve literal target URL port in NO_PROXY routing

proxy.rs:60 passes target.port_or_known_default() through u16 normalization.
The NO_PROXY entry port is now literal, but the target port is still normalized.
Go canonicalAddr retains an explicitly written target URL port: NO_PROXY=
example.com:080 plus http://example.com:080 bypasses in Go but proxies in Rust;
NO_PROXY=example.com:80 plus the same valid URL proxies in Go but bypasses in
Rust. IPv4 and bracketed IPv6 target variants reproduce the first difference.
Ordinary explicit :80 control matches. Preserve raw target port spelling in
routing, including the actual production path and redirect selection; a
probe-only correction must not hide native transport behavior. Include valid
leading-zero target ports in the actual routing/HTTP corpus. Same receipt as above.

## P2: make transport cache keys unambiguous

browse/crates/symbrowse-fetch/src/client.rs:108 formats session/proxy/private/
allowlist values into one delimiter string. Valid session and proxy values can
contain ';proxy=' and produce the same cache key for different configurations.
Two independent real owned proxy exchanges reproduce this: first session A,
proxy P+';proxy='+Q returns proxy-first and caches P; next session A+';proxy='+P,
proxy Q should return proxy-second. Go does; Rust reuses the first transport
and returns proxy-first, despite validating the current requested proxy. This
also retains the wrong cookie jar for a different named session. The source
already allows both strings; no unusual URL parse failure is involved. Use a
hashable typed tuple/struct for all transport-defining values and add two-proxy/
two-session behavioral coverage. Literal requests, status/body/header outputs,
clean source and executable hashes are retained at
/tmp/symaira-fetch773-fixed-independent-cache-collision.json. First request is
a successful matching control; second request demonstrates the wrong route.

## Full reviewed layers and independent verification

Read all changed production layers from the original Fetch increment as well
as the focused correction against cc8: public types/defaults/errors, transport
cache and invalidation, headers and gzip negotiation, bounded full-member
response decoding/charset handling, deadlines/retries/redirect boundaries,
environment snapshot/CGI/NO_PROXY precedence and frozen SDK matcher behavior,
SSRF target/proxy checks/shared DNS pinning and protected SOCKS DNS selection.
Review the actual Go honest transport, supplemental Go/native programs,
Python case comparator and five executable mutants, all 70 newly retained
routing shapes and seven multi-member gzip cases, six-platform workflow,
benchmark identity/build receipt code and unchanged value comparator, original
historical failure preservation, public ADR and matrix/plan scope limits.
Changed production modules remain below 400 lines; no new dependency or Go
production/frozen fixture mutation. Main integration is normal and source-bound.
No additional actionable finding beyond the three above in reviewed scope.

Fresh independent full corpus passes 231/231 (64 actual HTTP cases and167
route-only cases that open no network connection). All five wrappers execute
the real native process then intentionally mutate a case/body/header/error/
route; each fails its intended assertion/mismatch, not build infrastructure.
Separately rerun the original 70 actual routing probes: 70/70 now match.
Independently implement and execute the three original concatenated gzip HTTP
probes: success, decoded limit7 and corrupt second-member CRC all now match.
Additional ten routing probes yield three matching controls and seven actual
differences from the first two findings. The two-proxy cache probe adds the
third independently confirmed finding. Overlapping cases are kept separately
instead of inflating a single aggregate acceptance count.

Verify all28 fetch inputs, nine harness/workflow inputs, all415 immutable Go
inputs against frozen Git objects, and all146 Rust release-build inputs against
the claimed immutable source and current candidate. Probe digests are native
972882b60a0421c5cb974e47d6892792652900ec9c40483a5f725c198f007f0c and Go
d52a94fccfa0d07c571f7109c0f43a9771ab6f5b3ea207569f539945bf8c4401.
Original independent review and literal70/3 failure reports are retained
byte-identically. The author's clean source process receipts match the freshly
verified source/Go/probe hashes. Evidence-only successor changes no program inputs.

Independently execute the existing source-bound native test binaries: all117
parent tests pass, one deliberately ignored child remains visible; its parent
executes seven isolated children and requires a successful one-test summary.
Additionally execute all seven child forms directly and retain their literal
outputs. Separately verify authored117-parent/24-summary/49-fetch counts and
original lint/Go log digests. An initial reviewer driver omitted pinned Go PATH
and help_oracle reached the unrelated system Go command (Unknown option: build).
The unchanged source/binaries pass with Go1.26.7 and the lifecycle subreaper.
This is a reviewer tooling failure, not product behavior. The first source audit
also used len() on the numeric samples field; corrected audit verifies 30 and
all30 raw samples for every paired workload. Neither error is silently counted
as acceptance.

Verify both optimized measurement executables' literal digests and clean build
receipts, Go provenance DC (not measurement checkout), all240 raw samples
(30 per4 workloads per2 implementations), nearest-rank p95 and literal wrong-
body controls. Independently rerun unchanged comparator: pass. All four fresh
identity tests pass. Paired measurements were inspected, not independently
remeasured. Historical Darwin-arm64 size baseline and local Linux timing
remain distinguished; no six-platform release-value claim is accepted.

Native six-target CI and full #773 cutover remain pending; browser/TLS and
complete pipeline parity and daemon #772 still gate closure. Review performs
no source changes, new target builds, GitHub writes or publication. The clean
candidate and existing build target remain unchanged for the author's next
isolated corrections. JSON receipt and literal independent proofs use the
/tmp/symaira-fetch773-fixed-independent-* paths.
