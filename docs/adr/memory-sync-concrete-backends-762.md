# Concrete Memory sync backends remain behind the existing admission gate

Status: implemented source checkpoint, uncompiled and not runtime-approved.
Refs #762. The user delegates implementation decisions and their rationale.
This extends the immutable `c4a020e0cec77fd38060c591aa1fde244df4b796`
foundation; its original source inventory, 230 planned cases, four planned
controls and frozen Go inputs remain unchanged. No target, process oracle,
crypto operation, HTTP listener or native platform job ran in this stage.

Use the existing Memory Store and sync interfaces for concrete HTTP and relay
backends. There is no second database, server, credential store, scheduler,
master key or Guard conduct policy. PB-2026-09-09 revision4 remains binding.
CLI `memory sync` and server sync refusals remain Go-gated: implementing an
HTTP client is not permission to admit the complete HTTP/CLI surface.
The approved Store base is PR803 `6ae73a7eef3442e2b4211a56043941339fb5a1d6`;
separate BUSY/historical-schema successors are not silently integrated.

Reuse repository-pinned ureq3.4.1, base64.22.1, sha2.10.9, getrandom.3.3 and
JSON support. ureq provides the concrete HTTP backend without introducing a
second runtime or HTTP dependency. Enable platform certificate verification
and gzip explicitly; owned TLS tests may provide explicit roots. It does not
implicitly disable certificate verification. Go's HTTP/2, proxy/NO_PROXY,
DNS, TLS, cancellation, timeout and connection behavior still require actual
transport comparison; sharing a crate does not establish parity.

Pin RustCrypto aes-gcm0.11.1, pbkdf2.12.2, hmac.12.1 and zeroize1.9.0. These
implement AES256-GCM and PBKDF2-HMAC-SHA256 rather than a local cryptographic
primitive. aes-gcm uses minimal aes/alloc/zeroize features; PBKDF2 enables only
HMAC. The dependency receipt records checksums, official crates.io origins,
licenses and MSRVs. The locked graph adds16 packages and preserves all273
previous package identities and checksums. Initial missing-PBKDF2 offline
failure, original lock, authorized fetch resolution and final offline
resolution remain separate evidence. Dependency resolution is not compilation
or a security audit. The workspace Rust1.98 requirement exceeds the recorded
new-crate MSRVs; actual locked platform builds remain mandatory.

The concrete relay backend implements the Go600000-iteration key derivation,
v1 version/salt16/nonce12/ciphertext+tag16 layout, salt8 legacy format,
v1-failure legacy fallback and run-phase key cache. Salt/passphrase identities
use the frozen HMAC cache design. Owned injectable entropy supports actual
fault probes without editing production Go. Keys/cache fingerprints are wiped
on replacement/drop; passphrases are supplied transiently, never added to
result JSON or stored in the database. Cross-runtime decrypt/encrypt, known
vectors, entropy error precedence and real authentication controls remain
required. A local round trip cannot prove the external format.

Implement ordered typed wire decoding in the sync owner, not a Serde Value
map that discards duplicate members. The iterative scanner includes ignored
values in the10000-container depth boundary. Separate visible slice lengths
and backing slots preserve nonempty shrink/regrowth/null behavior; null/empty
resets allocation. Typed assignment retains null scalars, merges map fields,
uses Go-style malformed UTF8/surrogate replacement and continues recoverable
type errors. ApplyResult retains its actual custom snake/camel lookup behavior.
Byte arrays retain allocated-empty versus nil blob state and base64 error byte
offsets. SDK-derived code retains the Go Authors BSD license and exact source
map. These are source-derived implementations, not executed Go observations.

Encode outgoing fields in frozen declaration order with sorted metadata,
omitempty, HTML escaping, finite float32/64 formatting and wire timestamps.
Direct public-client allocated-empty request slices are not fully represented
by the orchestration's slice-only interface: plain empty slices currently
encode nil/null; relay empty outer slices encode allocated/empty. The runner
never sends an all-empty batch, and empty opposite plain groups originate as
nil in Go's database reads. Direct allocated-empty/null-pointer request cases
remain unadmitted until represented and proven explicitly. Existing Store
embedding serialization is also a separate exact float/state proof gate.

Keep the initial raw path distinct from redirect lexical cleaning. Manual
redirects preserve Go's status300 success, ten-request bound, body replay,
Bearer/Basic precedence, sensitive-header delegation and Referer ownership.
Pinned SDK1.26.7 makes body removal sticky: after301/302/303, later307/308
cannot restore the original body or Content-Type. An earlier uncompiled
source assumption that replayed it is retained separately as a static correction;
no original runtime failure is invented.
Success/error response limits are10MiB/64KiB; read only the first JSON response
rather than waiting for trailing body EOF. Pinned Decoder.readValue even
accepts a scalar's first value before reporting an invalid following byte;
inner Unmarshal rejects the full payload's trailing bytes. The earlier
uncompiled scalar-delimiter assumption is retained as a static correction.
Encrypted inner JSON consumes the whole payload. Use remaining caller/run/client deadlines, no cookie jar and
no automatic retry/transaction. Unsupported opaque/no-host, IDNA and IPv6-zone
request families fail closed. Raw headers, redirects, fragments/query quirks,
proxy and cross-platform system errors remain actual proof requirements.

Do not reproduce credential persistence through URL userinfo. Frozen Go's
runner keys sync_state and reports errors/results with the literal remote
URL (`syncclient/runner.go`), so userinfo can become durable or visible. This
is a static source finding, not a reproduced operator incident. Native HTTP
run validation refuses a credential-bearing identity before reading cursors;
use a separate Bearer token. The standalone client constructor still supports
owned Basic-auth byte probes without a Store. URL parse/policy/request-prefix
diagnostics redact authority userinfo, suppressing malformed credential-detail
errors. This is an explicit bounded divergence requiring actual no-write and
secret-absence controls. It does not claim to sanitize arbitrary credentials
placed in path/query or arbitrary peer error bodies, and does not alter frozen
Go or claim the Go-gated CLI has acquired this protection.

Remaining admission gates include exact malformed time/syntax/network/TLS
error messages, extreme time offsets/local-zone ownership, Ryu numeric bytes,
all source-inventoried Store/protocol/CLI cases, native Unix/Windows raw input
semantics, complete parent CLI/DB/MCP gates and independently reviewed native
three-OS CI. The current time parser deliberately refuses unproved malformed
and leap-second families rather than inventing Go diagnostic equivalence.
Go's nullable-memory panic, equal-clock relay pagination, unbound outer relay
identity and destructive tombstone continuation remain the original foundation's
explicit boundaries. No destructive behavior is silently fixed or admitted.

The additive backend plan preserves the original230 cases verbatim. Prepared
regressions and meaningful mutation plans are labeled unexecuted. Freeze the
source, then allocate owned resources for real process/HTTP/state/crypto gates;
record original failures before corrections, require different-author review,
and remove only the proven sync routing seam when complete. #762 stays open.
