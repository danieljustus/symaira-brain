# Independent full-layer review: honest fetch #773

Immutable clean candidate `cc8a1673bead86b1d34b6f84ea9bcbf5ced86017`;
parent corrected daemon source `6c3bf1f93d2760b17f4f936076d26f0f4c00fe1d`.
Verdict: REQUEST CHANGES. Two confirmed P2 findings below; candidate unchanged.

## P2: preserve Go NO_PROXY matcher grammar

`browse/crates/symbrowse-fetch/src/honest/proxy.rs` normalizes bypass port text to
u16, interprets bracketed IPv6 without a port as an IP matcher, and normalizes
IPv4-mapped target IPs before testing IPv6 CIDR matchers. Actual Go SDK matcher
semantics differ: compare nonempty port strings literally; bare bracketed IPv6
is not a valid IP matcher; IPv4-mapped IPv6 CIDRs can contain an IPv4 target.

Seventy fresh actual Go/native route-only process comparisons (no network) give
62 matches and eight differences. NO_PROXY=example.com:080 / :00080 bypasses in
Rust but retains proxy in Go for canonical HTTP port80; the same happens for
93.184.216.34:080 and [2001:4860::1]:080. NO_PROXY=[2001:4860::1] also improperly
bypasses in Rust. NO_PROXY=::ffff:93.184.216.0/120 bypasses the matching IPv4 URL
in Go but retains the proxy in Rust. Ordinary :80, domain suffix, trailing-dot,
IPv6 CIDR and other control entries match. Preserve original full receipt
`/tmp/symaira-fetch773-independent-extra-routing.json`. Add these shapes to the
actual SDK process corpus and follow SDK SplitHostPort/IPNet behavior. These
are matcher differences, not the separately documented malformed-URL limits.

## P2: decode all concatenated gzip members and reject invalid tails

`browse/crates/symbrowse-fetch/src/honest/response.rs` retains flate2 GzDecoder,
which stops after the first member. The frozen Go net/http transport reads the
whole concatenated stream. The source author's additional actual observation
was independently reproduced in three fresh real HTTP cases: successful Go
body firstsecond versus Rust first; decompressed size limit7 yields Go too_large
versus Rust200/first; a corrupted second-member checksum yields Go error versus
Rust200/first. All literal observations are retained in
`/tmp/symaira-fetch773-independent-gzip-extra.json`. Use the bounded multi-member
reader and preserve full-stream size and decode errors; add success, size
boundaries and corrupt-tail process cases. This fixes an inherited transport
bug in the currently reviewed body/gzip contract, not a newly introduced Go bug.

## Inspected layers and verification

Read every changed production layer: public request/default/error types, cache
keys/invalidations, headers and gzip advertisement, redirects and deadlines,
bounded decoding/charset normalization, proxy snapshot/CGI/NO_PROXY precedence,
SSRF peer checks/shared DNS pinning and protected SOCKS DNS behavior. Inspect
all new supplemental Go/Rust programs, Python comparators/controls, frozen-source
checks, benchmark executable identities/build receipts/comparator, six-target
workflow and allowed-to-fail supplemental value job, matrix/plan/ADR changes.
Trace against actual frozen Go fetch and SDK HTTP proxy implementation. Domain
boundaries and legacy browser compatibility gates remain. No other actionable
findings in reviewed scope. All changed production files are below400lines.

Fresh independent clean-head gate:154/154 observations, separately counted as
57 actual HTTP exchanges and97 route-only selections. Five fresh wrappers run
the actual candidate then mutate missing case/body/header/error/proxy route;
verify each failed for its intended assertion or observed mismatch, not a build
or infrastructure failure. All candidate and415 frozen Go source hashes, probe
binary hashes and authored clean-source proof match exactly. Original Go source
and frozen fixtures remain unchanged. Go SDK1.26.7 explicitly selected.

The first independent command pointed at Browse-local target instead of the
actual root target and failed before running native comparisons. Original log
`/tmp/symaira-fetch773-independent-wrong-target-path.log` retained; correct
immutable binary rerun passed154/154 and all five intended controls. This
path error is tooling evidence, not a product failure or native acceptance.

Author's49 fetch tests and117 scoped parent-test/one explicitly re-executed
child receipt checked, as were strict lint/format/actionlint and original
historical gates. The author's initial Go GUI-PATH build error is preserved;
pinned SDK rerun passed. These counts are not described as a second independent
full test execution. Independently run unchanged benchmark comparator on the
source-bound30-pair optimized report: pass, all4 workloads complete, literal
wrong-body controls rejected, no threshold changes. Validate both actual
executable digests, source revisions and clean build receipts; Go provenance
correctly stays DC rather than measurement checkout. This is local Linux
supplemental value evidence; historical Darwin-arm64 baseline size and
remaining six-platform/Windows paired lifecycle limitations are explicit.

Fresh native six-target CI remains necessary after corrections. #773 and daemon
#772 remain open; neither browser/TLS compatibility nor complete fetch pipeline
is claimed ported by this bounded slice. No push, PR, merge or source edits
performed by this reviewer. Original failure receipts remain unchanged.
